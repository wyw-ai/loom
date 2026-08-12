# GUI 轻量化与 Agent 运行可见性：问题分析与改造方案

状态：已实施（分支 `feat/gui-lightweight-runtime-visibility`，2026-08-09）

实施记录：

| 批次 | 内容 | 提交 |
| --- | --- | --- |
| B1 | token 统计修复（usage 走 run.close + 持久累计 + manifest 语义 + 口径拆分）；超长消息截断（trigger 4k / event 2k / assignment 4k / 上下文 500 字符 + `loom message get`；server 64KiB body 上限） | `fix(usage): make token stats durable...` |
| B2 | journal 快照压缩（含 TOP2 启动全量重放）、SQLite PRAGMA、delivery 双向索引、inbox 轮询 15s→60s；侧边栏双区 + onboarding 第 5 步 + 空态引导 + coach marks | `perf(server): journal snapshots...` |
| B3+B4 | inbox.status / delivery.cancel / delivery.expedite / presence.changed + worker 队列联动 + AgentActivityBanner；wake 三预设下拉 + turnInputStyle minimal/structured 双轨（默认 minimal） | `feat(runtime+gui): agent activity banner...` |
| B6 | gui-web Web 直连(webBridge)、全页面响应式(<768px)、PWA 可安装 | `feat(gui-web): full mobile support...` |
| B7 | `loom-mobile` React Native + Expo 原生 App(连接/频道/聊天/DM/run 状态条) | loom-mobile 仓库(未 commit,待用户审阅) |

日期：2026-08-08

已确认决议（2026-08-08）：

1. §5.3 消息模板：**双轨**，`turnInputStyle: minimal | structured`，默认
   `minimal`；
2. §4 横幅范围：只显示当前 channel/thread 相关 agent；
3. §1 协议变更：允许，但必须**向后兼容**——全部为增量可选字段 + 新增方法；
   旧 server/daemon/gui 与新版本混跑必须继续工作，客户端对新方法做
   feature-detect 并优雅降级；
4. B2 性能批次：**全做**，包括 TOP2（启动全量重放 journal）也要解决；
5. 移动端**一步到位**：gui-web 完整移动适配（全页面响应式 + 脱离 Tauri
   直连 server 的 Web 模式 + PWA 可安装）**并且** `loom-mobile` 以
   React Native + Expo 建原生 App（复用 TS 协议层）。

范围：用户反馈的 8 个问题——GUI 新用户引导、手机端可用性、服务端性能、
server 内信息架构、agent 运行生命周期可见性、wake policy 配置形态、
agent 收到超长消息、token 统计不准。每个问题先定性（bug / 体验设计问题），
给出证据（文件+行号），再给修复或设计方案。

---

## 0. 总览：定性与优先级

| # | 问题 | 定性 | 优先级建议 |
| --- | --- | --- | --- |
| 8 | token 统计不准 | **bug（3 个叠加）** | P0：默认路径根本没有数据 + 口径错误 |
| 7 | agent 收到超长消息 | **bug（截断缺口）+ 设计问题** | P0：与 #6 一起改 prompt 组装 |
| 3 | 服务端性能差 | **bug 级反模式若干** | P1：锁/广播/轮询三板斧 |
| 5 | agent 生命周期不可见 | 体验缺失（需新增少量 RPC） | P1：横幅是本轮体验改造核心 |
| 6 | wake policy 配置奇怪 | 体验设计问题 | P1：预设下拉 + 极简消息模板 |
| 4 | server 内信息架构 | 体验设计问题 | P2：导航减负 |
| 1 | 新用户引导不到位 | 体验设计问题 | P2：与 #4 一起做 |
| 2 | 手机端几乎不可用 | 体验设计问题（架构性） | P2/P3：先做一档"可用"，完整适配另立项 |

依赖关系：#5 横幅需要 #3 中的部分 server 改造（queue 可查询、run 通知）；
#6 与 #7 都落在 `compose_envelope_prompt` 一条链路上，应一次改完。

---

## 1. 问题 8：token 统计不准 —— 三个叠加的 bug

### 1.1 根因 A（最severe）：默认配置下 usage 元数据根本不进消息流

- `token_usage` 元数据只由 `build_turn_meta` 写入
  （`crates/cli/src/cmd/agent_serve.rs:8807-8831`），调用点只有两个：
  1. auto-publish 路径（`agent_serve.rs:8372`）——受
     `LOOM_AGENT_AUTO_PUBLISH_FINAL` 控制，**默认关闭**
     （`agent_serve.rs:8860-8869`）；
  2. `publish_failed_turn_notice`（`agent_serve.rs:8572`）——只有失败通知。
- 正常流程里 agent 是自己执行 `loom message send` 发布回复，worker 无法在
  这条消息上附加 usage 元数据。
- GUI 的 `usageStore.applyMessage` 只从 `message.metadata.token_usage` 读数
  （`apps/gui-web/src/store/usageStore.ts:91-107`、
  `apps/gui-web/src/ipc/types.ts:529-543`）。
- **结论**：默认配置下，GUI 统计的样本大多来自失败通知和极少数路径，
  正常成功 turn 的 usage 大面积丢失。统计"不准"首先是"根本没统计到"。
  （worker 侧确实也把 usage 写进了 run trace 的 `agent.usage` status frame
  （`agent_serve.rs:8314-8355`、`8437-8452`），但 `usageStore` 不消费
  trace 帧，只消费 message metadata。）

### 1.2 根因 B：增量/累计口径未对齐 provider 语义

- `build_turn_meta` 把 provider 报的 usage 一律当**本 turn 增量**，
  用 `accumulate_usage` 按 scope 在 worker 进程内存累加
  （`agent_serve.rs:8813-8823`、`3101-3107`）。
- 但 provider session 是 per-scope `--resume` 复用的：部分 CLI 在 resume
  会话上报告的是**会话累计值**（而非本次调用增量）。累计值再被累加 =
  平方级膨胀。`usage.rs::extract_token_usage_from_text` 只取输出里最后一条
  JSON 快照（`crates/agent-runtime/src/usage.rs:132-148`），没有
  "该 provider 报的是增量还是累计"的标注。
- `usage-manifest.toml` 只定义字段路径映射（claude/codex/copilot/opencode
  有效，qoder/kimi/zcode 是空占位，`usage-manifest.toml:55-117`），
  没有语义（delta vs cumulative）字段。
- 叠加问题：`normalized_total()` 把 input+output+cache_creation+cache_read
  +reasoning 全部相加（`usage.rs:112-129`）；若 provider 的
  `input_tokens` 已含 cache read（Anthropic 语义下不含、OpenAI 语义下
  `prompt_tokens` 含 `cached_tokens`），跨 provider 口径直接错位。

### 1.3 根因 C：worker 内存累计 + GUI 按 actor 求和的聚合裂缝

- `usage_totals` 在 worker 进程内存里（`agent_serve.rs:3101`），worker
  重启即清零——GUI 里 cumulative 会"跳回去"。
- GUI `useScopeUsageSummary` 把同 scope 所有 actor 的 cumulative 相加
  （`usageStore.ts:129-159`），注释声称非重叠但无校验；
  `shouldReplace()` 只按 `updatedAt/messageId` 排序，重放/乱序会留旧值
  （`usageStore.ts:52-63`）。
- 无 provider usage 时 fallback 是"ASCII 4 字符≈1 token、非 ASCII 1 字符
  =1 token"的估算（`usage.rs:50-74`），`estimated=true` 标记存在但 GUI
  聚合时不区分真实值与估算值。

### 1.4 修复方案（确认后实施）

1. **usage 数据流改走 server 权威**（修根因 A/C）：
   - `run.close` RPC 增加可选 `usage` 字段，worker 在 turn 结束时把最终
     usage 快照随 run 关闭上报；server 把 usage 落进 Run 记录（journal
     持久化，重启不丢）。
   - GUI usage store 改为消费 `run.updated` 通知/`run.list` 查询，
     message metadata 通道保留兼容但不再是主通道。
   - 聚合下沉 server：新增 `usage.summary`（按 scope/actor/时间窗聚合），
     GUI 不再自行求和。
2. **usage-manifest 增加语义标注**（修根因 B）：每个 provider 条目加
   `semantics = "delta" | "session_cumulative"`；worker 对 cumulative
   provider 记录上一快照做差分后再累加。缺省按 delta 并打
   `semanticsUnknown` 标记。
3. **口径拆分**：`TokenUsage` 保留分项，total 不再无脑五项相加；按
   provider 家族定义 total 公式（manifest 可配），cache read 单列展示。
4. **估算值隔离**：`estimated=true` 的样本在 GUI 单独渲染（"≈"前缀/
   灰色），聚合时分开累计，不与真实值混算。

---

## 2. 问题 7：agent 收到超长消息 —— 截断缺口清单

### 2.1 现状（有预算机制，但四处漏水）

已有机制：bootstrap 首 turn 最多回放 20 条、每条 800 字符截断
（`agent_serve.rs:6035-6078`、`compact_message_body` `6591-6598`）；
resume turn 用 pending delivery + `contextTokenBudget`（默认 900 token、
上限 8000，`agent_serve.rs:3238-3252`）裁剪，超出写 `unreadGap`
（`6099-6189`）。

漏水点（按严重度）：

| 漏点 | 证据 | 说明 |
| --- | --- | --- |
| **触发消息本体不截断** | `render_trigger_body_block` `agent_serve.rs:7155-7160` 直接注入全文 | 用户/上游 actor 发一条 200KB 消息，全量进 prompt |
| **event payload 不截断** | 同上 `7161-7168`、`render_context_event_block` `7197-7211` 用 pretty JSON 全量 | 大 payload 事件直接撑爆 |
| **assignment JSON 不截断** | `assignment_context_for_prompt` `7395-7424` | 整段 pretty JSON 无上限 |
| **profile prompt files 上限过大** | 128KiB/文件，多文件拼接（`agent_serve.rs:5346`） | 每 turn 注入 |
| 发送侧无消息长度上限 | GUI/CLI 均未见限制 | 巨型消息进入事实图后处处放大 |
| token 预算用字符估算 | `usage.rs:50-74` | CJK 文本偏差大，预算失真 |

### 2.2 修复方案

1. **prompt 侧统一"每块预算"**：给 trigger body、event payload、
   assignment JSON 各设截断上限（建议：trigger body 单条上限与上下文一致
   接受 spec 配置；超出时保留头部 + `……(全文 N 字符，loom message get
   <id> 查看)` 尾注）。这与问题 6 的消息模板改造是同一处代码
  （`compose_envelope_prompt` / `render_turn_input_contract_with_names`）。
2. **上下文回放条目从 800 字符降为 500 字符**（对齐用户期望的形态），
   截断尾注统一为"更多内容通过 `loom message get <msgId>` 查看"。
3. **发送侧软上限**：`message.send` 服务端加可配置 body 上限（如 64KB，
   超出拒绝并提示走 artifact），GUI composer 加字符计数提示。
4. 估算器保持启发式（够用），但预算默认值建议 900 → 1500，配合每块
   硬上限后总量仍可控。

---

## 3. 问题 3：服务端性能差 —— TOP 反模式清单

按严重度排序（全部有代码证据）：

| # | 问题 | 证据 | 修复方向 |
| --- | --- | --- | --- |
| 1 | **单 SQLite 连接 + `Mutex<Connection>`**，读写重放全串行；只开 WAL，无 `busy_timeout`/`synchronous` 调优 | `crates/server/src/journal.rs`（`JournalStorage::Sqlite{conn: Mutex<Connection>}`、`append`/`replay` 均 `conn.lock()`） | 读写分离（读池 + 单写线程）；补 PRAGMA |
| 2 | **启动全量重放 journal 进内存**，`Inner` 持全量 HashMap（messages/deliveries/runs/…） | `store.rs::Store::open` → `replay()` → `apply_replay` | journal 快照 + 分段；冷热分离；至少加载进度日志 |
| 3 | **内存全表扫描**：`values().filter().cloned().collect()` 遍布 list_*；`message_delivery_recipients`（~4256）每条消息全量遍历 | `store.rs` 多处 | 按 scope/recipient/state 建二级索引 map |
| 4 | **广播扇出带全量对象 + 双路重复**：`MessageCreated` 对 scope+thread 双广播，每收件人克隆完整 payload | `ws.rs::fanout`、`subscribe.rs::broadcast_to_scope/all` | payload 摘要化（id+scope+meta），克隆一次共享 `Arc` |
| 5 | **订阅表一把 `RwLock<Inner>`**，广播发送持读锁遍历所有连接，无背压 | `subscribe.rs` | 拆锁 + 快照后发送 + 慢连接淘汰 |
| 6 | **N 个 agent 每 15s 轮询 inbox**（固定背景 QPS，且每次拉 200 条 pending） | `agent_serve.rs`（`inbox_poll_every = 15s`、`drain_pending_inbox` ~3905） | WS 已有实时通道，轮询降频到 60-120s 仅作对账；或 server 推 delivery 游标 |
| 7 | **SQLite 业务表无二级索引**（deliveries 按 recipient/state、messages 按 target 查询无索引） | `journal.rs` schema 无 `CREATE INDEX` | 补复合索引 |

建议本轮先做 1/3/4/6（最小改动、收益直接），2/5/7 视数据量增长排期。
另：#5 横幅需要的 `inbox.count`/`run` 通知也顺带受益于 3 的索引化。

---

## 4. 问题 5：agent 运行生命周期不可见 —— 横幅设计

### 4.1 现状能力盘点

| 需要 | 现状 | 缺口 |
| --- | --- | --- |
| "谁在运行" | server 有 `run.open/close/cancel/list/get`（`proto/methods.rs` RUN_*；`store.rs::open_run/close_run`），`RunUpdated` 事件按 run.scope 广播 | 无按 actor 全局订阅；GUI 需订阅所有相关 scope 或轮询 |
| "多少消息排队" | 真队列 `pending_triggers` 在 **worker 进程内存**（`agent_serve.rs`）；server 侧近似 = 未 ack 的 durable delivery，可用 `inbox.list` 查（`handlers/mod.rs::inbox_list`） | 无 per-actor pending 计数 RPC；worker 内存队列与 server delivery 有短暂偏差 |
| "是否离线" | `connection/list` 返回当前持有 live inbox 的 actor（`subscribe.rs::connected_actor_ids`） | 无 presence 变更推送，GUI 只能轮询 |
| "停止" | `run.cancel` 存在且会唤醒 worker 取消 provider 进程（`handlers/mod.rs::run_cancel`、worker `handle_run_cancel_message` `agent_serve.rs:4638`） | 已可用 |
| "撤销排队消息" | 只有 `delivery.ack`（消费确认），**无 cancel/撤销** | 需新增 `delivery.cancel` |
| "立即插话/提前" | 无重排机制 | 需新增（见下） |
| "共 N 条 / 未读 M 条" | `message.list` + `inbox.list` 可组合算 | 需要 server 端计数接口避免拉全量 |

### 4.2 新增协议面（草案）

1. `inbox.count` — 入参 actorId（可批量），返回
   `{pending, oldestPendingAt}`；GUI 横幅收起态用。
2. `delivery.cancel` — 撤销一条（或某 actor 全部）pending delivery：
   server 置 delivery 为 `cancelled` 终态并广播 `DELIVERY_UPDATED`；
   worker 在 dispatch 前校验 delivery 仍 pending，已撤销的触发从
   `pending_triggers` 剔除（挂载点：`begin_or_enqueue` /
   `pop_batch_locked`）。
3. `delivery.expedite` — "立即发送"：把该 delivery 标记为高优先级并通知
   worker；worker 侧将对应 trigger 移到队首（若目标 scope 空闲则立即
   dispatch）。挂载点同上；若 agent 配置了 `cancel_and_requeue` 且用户
   明确选择，也可触发打断。
4. `presence.changed` 通知 — actor 的 worker 连接建立/断开时广播，替代
   GUI 轮询 `connection/list`。
5. `run.updated` 已有——GUI 增加按 actor 过滤的订阅（或 server 补
   `run.subscribe {actorIds}`）。

### 4.3 横幅 UI 设计（聊天框上方，默认收起）

**收起态**（一行，仅在有事发生时出现；全空闲时不渲染）：

```text
▸ ⚙ 2 个 agent 运行中 · 5 条消息排队中
```

**展开态**（每个"有状态"的 agent 一行主状态 + 可展开的排队消息清单）：

```text
▾ Agent 运行状态
  ● Developer   [运行中 2m14s] 1 条处理中 [停止]   3 条待处理 [撤销全部] [展开]
  ○ Reviewer    [空闲]
  ◌ Tester      [离线] 2 条消息待处理 [撤销全部] [展开]
  ⏸ Researcher  [排队等待中] 前方 1 个 turn [撤销全部]
    └ (展开后，每条一行，超长省略)
      [09:31] canfuu: 帮我把登录页的样式改成…      [立即发送] [撤销]
      [09:32] canfuu: 另外顺便看下报错日志里那个…  [立即发送] [撤销]
```

状态机（横幅显示的每个 agent 状态）：

| 状态 | 判定 | 可用操作 |
| --- | --- | --- |
| 运行中 | 有非终态 Run 且 worker 在线 | [停止]=run.cancel；排队消息 [撤销全部/展开] |
| 空闲 | worker 在线、无 run、无 pending | 无 |
| 离线 | `connection/list` 无该 actor | pending 消息 [撤销全部/展开]（delivery 仍在 server，可撤） |
| 排队等待中 | worker 在线、无 active run、但有 pending（如 debounce 窗口内/等待前序 scope） | [撤销全部/展开] |
| 状态未知 | 有非终态 Run 但 worker 连接已断（崩溃残留） | 提示 + [强制关闭 run] |

排队消息行操作：
- **[立即发送]** → `delivery.expedite`；
- **[撤销]** → `delivery.cancel`；
- **[撤销全部]** → 批量 `delivery.cancel`。

数据流：收起态计数用 `inbox.count` + `run.updated`/`presence.changed`
推送增量维护；展开时才 `inbox.list` 拉明细（避免常驻拉全量）。

范围规则：横幅只显示**与当前 channel/thread 相关**的 agent（成员中的
agent + 最近在此 scope 有 run 的 agent），避免全局噪音；顶栏可另给一个
全局入口（后续迭代）。

---

## 5. 问题 6：wake policy 配置奇怪 —— 预设下拉 + 极简消息模板

### 5.1 现状

`WakeSpec` 暴露 5 个原始字段：`coalesce`、`debounceMs`、`replyReminder`、
`onHumanMessageWhileBusy`、`contextTokenBudget`
（`proto/methods.rs:3056-3113`）；GUI 以原始表单呈现（checkbox + number +
两个 select，`apps/gui-web/src/components/settings/AgentComponents.tsx:
1153-1211`；默认值 `wake-utils.ts:1-15`）。对用户这是实现细节泄漏。

### 5.2 方案：一个下拉框，三个预设

GUI 只显示一个下拉（保留"高级"折叠区放原始字段，预设即原始字段的映射，
协议无需改动）：

| 预设 | 映射 | 适用 |
| --- | --- | --- |
| **排队 + 合并**（默认） | `coalesce=true, debounceMs=750, busy=queue` | 常规助手 |
| **排队 + 逐条处理** | `coalesce=false, debounceMs=0, busy=queue` | 每条消息都必须独立成 turn 的流程型 agent |
| **打断 + 追加** | `coalesce=true, debounceMs=250, busy=cancel_and_requeue` | 高交互、人随时改主意的结对场景 |

选了预设后，下面一行灰字说明该模式的行为（如"新消息会打断正在运行的
turn，与未处理消息合并后重新开始"）。

### 5.3 每种 policy 的 user message 模板（极简纯文本）

按用户给的样式定稿（替换现 v1 JSON header + fenced body 的"重"形态，
或作为 `turnInputStyle: minimal | structured` 可配置，默认 minimal——
**这点需要你确认**：v1 JSON contract 是 2026-07 刚落的，直接替换还是
双轨保留？）。

**排队+合并**（一个 turn 吃掉最早 5 条未读，发送后全部 ack 为已读）：

```text
待处理消息清单：
[未读] 2026-08-08 09:31:02 canfuu(Human)[id=actor_h_01][msgId=msg_101]:
帮我把登录页的样式改成深色主题，注意对比度……（消息最多展示500个字符，更多内容通过 loom message get msg_101 命令查看）
[未读] 2026-08-08 09:31:40 Developer(Agent)[id=actor_a_02][msgId=msg_102]:
好的
[未读] 2026-08-08 09:32:15 canfuu(Human)[id=actor_h_01][msgId=msg_103]:
顺便看下报错日志
当前channel(#dev) 共 342 条消息，待处理 8 条。
规则：一次性回应以上全部消息；回复用 loom message send；无需回应则 loom run ignore。
```

**排队+逐条**（每 turn 一条）：

```text
新消息：
[未读] 2026-08-08 09:31:02 canfuu(Human)[id=actor_h_01][msgId=msg_101]:
帮我把登录页的样式改成深色主题……
当前channel(#dev) 共 342 条消息，待处理 7 条（本条之外）。
规则：只处理本条消息；回复用 loom message send；无需回应则 loom run ignore。
```

**打断+追加**（被打断重启的 turn，标注打断上下文）：

```text
你上一轮处理被新消息打断，以下为合并后的待处理清单：
[处理中被打断] 09:31:02 canfuu(Human)[id=...][msgId=msg_101]:
帮我把登录页的样式改成深色主题……
[新消息] 09:33:20 canfuu(Human)[id=...][msgId=msg_104]:
等一下，主题色改用品牌蓝
当前channel(#dev) 共 344 条消息，待处理 2 条。
规则：以最新消息为准处理冲突；回复用 loom message send。
```

实现要点：
- 每条 500 字符截断 + `loom message get <msgId>` 尾注（与问题 7 的
  统一截断策略共用实现）；
- 每批最多 5 条（可配 `WAKE_COALESCE_MAX` 现为 10，改预设映射为 5）；
- "共 N 条 / 待处理 M 条"来自 4.2 的 `inbox.count` + message 计数；
- 时间用本地时区格式化（现有 time context 段可因此收缩）。

---

## 6. 问题 4：server 内信息架构 —— 以 channel + actors 为中心减负

### 6.1 现状

侧边栏 8 个顶级入口：Home、All Channels、DM、Threads、Inbox、Tasks、
Runs、Actors/Settings（`components/layout/Sidebar.tsx:117-125`）。actors
配置藏三层深（Settings → Agents → 某 agent → 4 个 tab；
`SettingsView.tsx:57-86`、`AgentComponents.tsx` detailTabs）。

### 6.2 方案（纯前端改造，不动协议）

1. **侧边栏改双区**：
   - 主区（常驻）：**Channels（含频道列表直接内嵌）+ DM + Actors**；
   - 折叠区"更多"：Threads / Inbox / Tasks / Runs（保留能力，降权入口；
     Tasks/Runs 的信息以徽标/横幅形式回流到聊天上下文里——问题 5 的横幅
     就是 Runs 的主要消费面）。
2. **Actors 提升为一级入口**（不再叫 Settings）：进入即 agent 卡片列表
   （状态点 + 在线/离线 + 今日 turn 数），点击直达配置；wake 预设下拉
   （§5）放在卡片摘要层，不必进四级 tab。
3. **频道列表默认展开在侧边栏**（像 Slack），"All Channels"页降级为
   浏览/加入页。
4. Home 视图与 channel 视图合并判断：无未读时直接落到上次频道。

---

## 7. 问题 1：新用户引导不到位

### 7.1 现状

Onboarding 只有"prep/identity/server/host"4 步环境配置
（`OnboardingView.tsx:37,739-759`），做完直接落进空 workspace；用户要
自行理解 identity/server/host/actor/channel/provider 六个概念、自行创建
agent、自行发现"@agent 才会唤醒"。没有任何发第一条消息的引导。

### 7.2 方案

1. **onboarding 加"第 5 步：创建你的第一个 agent"**：检测本机已装的
   provider CLI（daemon 已有 discovery 能力，`agent-runtime/src/
   discovery.rs`），一键用检测到的 provider 生成默认 agent（默认名 +
   默认 wake 预设"排队+合并"），并自动建一个 #general 频道把 agent 拉进去。
2. **空状态引导**：频道空态不再是空白，而是三步卡片
   （"① @你的agent 打个招呼 → ② 看它如何回复 → ③ 需要时停止/插话（指向
   横幅）"）；输入框 placeholder 提示"输入 @ 唤醒 agent"。
3. **首次到达关键界面的一次性 coach mark**（横幅、actors 页、wake 预设
   下拉各一条，本地存储去重）。
4. 引导文案里屏蔽 host/machine 概念——单机默认场景下 host 步骤自动完成
  （已有"启动本机 Host"按钮，改为默认勾选）。

---

## 8. 问题 2：手机端几乎不可用

### 8.1 现状定性：架构性缺口，不是样式 bug

- 布局是桌面多栏 grid + 固定宽度 + `touch-action:none` 的 resize handle
  （`design/globals.css:66-83, 24-31, 99`），仅个别 `@media 640px`；
- **更根本**：`ipc/bridge.ts` 全部走 Tauri `invoke/listen`，无 Tauri 时
  直接 reject（`src/ipc/bridge.ts:1-70`）——gui-web 目前根本无法脱离
  桌面壳以纯 Web 方式使用；
- `loom-mobile` 目录是空壳（只有 .git）。

### 8.2 方案（分两档，建议本轮只做第一档）

**第一档（本轮）：让 gui-web 能在手机浏览器"可用"**
1. bridge 加 Web fallback：`hasTauriRuntime()` 为假时直连 loom-server
   的 WS JSON-RPC（协议就是 ws://…/rpc，桌面壳只是代理；需要补一个纯
   TS 的 RPC client + 事件流适配，Tauri 独占能力如"启动本机 host"在
   Web 下隐藏）。
2. 响应式一档断点：`<768px` 时单栏化——侧边栏抽屉化、detail panel 全屏
   叠层、composer 吸底、禁用 resize handle；触控目标 ≥44px。
3. 该档验收标准：手机浏览器上能登录 server、看频道消息、发消息、
   在横幅上停止/撤销。配置类页面（actors 详情）第一档不适配。

**第二档（另立项）：loom-mobile 原生/PWA**——等第一档验证交互模型后再投入。

---

## 9. 实施批次建议（待你确认取舍）

| 批次 | 内容 | 改动面 |
| --- | --- | --- |
| B1（bug 修复） | §1 token 统计（usage 走 run.close + manifest 语义标注 + 口径拆分）；§2 超长消息（每块截断上限 + 500 字符 + 尾注） | agent-runtime、cli/agent_serve、proto、server、gui usageStore |
| B2（性能） | §3 的 1/3/4/6 项（SQLite 读写分离、二级索引 map、广播摘要化、轮询降频） | server、cli |
| B3（生命周期横幅） | §4 新增 inbox.count / delivery.cancel / delivery.expedite / presence 通知 + 横幅 UI | proto、server、cli/agent_serve、gui-web |
| B4（wake 预设 + 消息模板） | §5 下拉预设 + 极简 user message 模板 | gui-web、cli/agent_serve |
| B5（IA + 引导） | §6 侧边栏减负 + §7 onboarding 第 5 步与空态引导 | gui-web |
| B6（移动一步到位） | §8 Web bridge fallback + 全页面响应式 + PWA；`loom-mobile` React Native + Expo 原生 App | gui-web、loom-mobile |

### 已拍板（见文档头部决议）

原待确认的 5 个问题均已确认，答案记录在文档头部"已确认决议"。
