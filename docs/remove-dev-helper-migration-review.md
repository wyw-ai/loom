# Remove dev-helper Migration — Review & Gap Analysis

> 对 `docs/remove-dev-helper-migration-design.md` 的方案 review。基础数据来自远端 `canfeng@11.158.213.187` 上的 agent / service / 适配器实际实现（`/home/canfeng/canfeng-projects/joi-auto-dev/joi-auto-dev/`、`~/.config/joi/`、`~/.local/state/joi-agent/`、运行中的进程）。

## 0. TL;DR

原方案的"概念分层"——`event` / `workspace` / `artifact` / `action` / `agent runtime` / `service runtime`——本身是正确的，对 dev-helper / dev-helper-bridge / stg / scope-projects / dispatch-state 的归位也基本合理。

但是在**从设计落到可执行**的层面，方案存在以下几类问题，必须补齐才能真的"彻底替换"现有远端能力：

1. **能力盲区**：readonly-repos、`git clone --shared` 缓存、delivery workspace 自动 provision、a1 kbase repo-notes、`context-share kv-*` 后端等几个目前在跑的关键链路，方案要么没提，要么只在一句话里带过。如果不正面解决，迁移后会出现"上下文链路断裂"。
2. **协议级语义没收敛**：trigger 幂等、session 失效、单实例并发、work_item_id 提取这几件事，设计只点了名称（"Trigger idempotency"、"provider session"）但没规定 server / agent runtime / agent skill 三方谁负责、字段是什么、判等规则是什么。落到实现就会再造一份 dev-helper 等价物。
3. **数据迁移路径缺失**：现役有活跃 session、dispatch cursor、scope-projects、stg KV、a1 kbase 镜像等真实状态。方案只在风险表里写"dry-run"，没有具体迁移脚本设计，会导致切换时丢上下文（例如 classmaster 当前在跑的 Copilot session）。
4. **action 错配**：方案把 action 写成"控制面替代既有人工确认"，但远端目前**根本没有任何 action 调用**。强行用 action 替代纯文本人工确认，是新增能力而不是迁移；"Done 必须满足 action 化"会把 net-new 当成 must-have，阻塞迁移本身。

下面分章节逐项指出问题及建议修订。

---

## 1. 设计原则层面（§2）

### 1.1 §2.1 "不新增 stg / KV server 抽象" — 方向对，但漏掉一个关键事实

设计提出三类承载（event / workspace / artifact）+ action 控制面，明确不新增 KV server。这个判断是对的。

**问题**：现实是远端的 stg 不仅仅是 KV，它**已经是一整个外挂系统**——`context-share call kv-put/kv-get`，需要 `dev-helper` 进程在线、`DEV_HELPER_BASE_URL=http://127.0.0.1:8080`、`NO_PROXY` 配置才能用。所有 agent / service 通过 `joi_rpc.py stg-read/write` 直连这个后端。要"不新增 KV"且"彻底下掉 dev-helper"，意味着同时下掉 context-share，**所有现有 stg key 必须有迁移目的地**：

| 现存 stg 键模式 | 数据 | 现状 | 方案落点 |
| --- | --- | --- | --- |
| `work_item/<id>/task_goal` | discovery 写、delivery 读 | KV | workspace `.joi/work-items/<id>/task_goal.json` ✅ 设计已覆盖 |
| `work_item/<id>/definition_of_done` | 同上 | KV | workspace 同上 ✅ |
| `work_item/<id>/clone_manifest` | discovery 写、delivery 读 | KV | workspace 同上 ✅（但 schema 没固化） |
| `work_item/<id>/execution_plan` / `progress_snapshot` / `lesson_plan` | classroom | KV | workspace 同上 ✅ |
| `work_item/<id>/runtime_state` | sensai / bug-triage | KV | workspace 同上 ✅ |
| `delivery_session` | delivery 自报 | KV | workspace ✅ |
| `repo_notes/<repo>/{repo_identity,development_process,development_notes,meta,all}` | router/discovery/delivery，**a1 kbase 的镜像** | KV+kbase | **未明确**（见 §3.3） |
| `capability_atlas` / `capability_atlas/*` | discovery | KV | workspace ✅ 但路径未指定 |
| `e2e-trigger/last-run` / `e2e-scanner/last-scan` / `feedback-scanner/<scope>/last-scan` / `triage-writer/last-write` | 各 service | KV | workspace `.joi/services/<id>/cursor.json` ✅ |

**建议**：在 §4.2 的目录约定下追加一份"stg key → workspace 文件路径"的完整映射表（包括 `capability_atlas`、`repo_notes/*`），并明确"迁移期间允许 stg / workspace 双写，切换完成后 stg 路径下线"。

### 1.2 §2.2 "Server 保持消息枢纽" — 缺一项 server 必须承担的能力

列举的 server 职责（actor connection / event journal / scope fanout / actor inbox / turn / trace / artifact store / channel·thread workspace projection）合理。

**问题**：trigger 幂等 / handoff 派发去重的归属没说。当前由 `run-agent.sh` 自己读 `dispatch-state/<actor>/<kind>-<id>.json` + `last_processed_event_id` 做去重；如果 agent runtime 不接管这件事，会有两个失败模式：(1) `joi agent serve` 重启后重复唤醒；(2) 多实例 race 重复处理。

**建议**：明确 "actor inbox delivery 必须保证 at-most-once 已派发指针，且暴露给 agent runtime"。这件事不是 KV，是 delivery 状态，应当是 server / runtime 共同承担的协议级能力。

### 1.3 §2.5 action 定位 — 与现状不符

方案将 "需求确认 / 发布批准 / service 操作 gate" 全部写进 action，并在 §11 把 "控制面标准化" 列为 Done 条件。

**问题**：远端目前**没有任何 action 流量**——既没有 `action.request` 也没有 `action.response`。classroom 当前的需求确认是 classmaster / teacher 直接以普通文本 reply 完成的，"approve / revise" 是人在群里说的。

这意味着把 action 放进 Done 条件，等于**把"新增能力"绑进"迁移"任务**。结果：
- 阻塞迁移本身（要先实现并接通 action UI 才能下掉 dev-helper）。
- 或者迁移团队为了过 Done，在 agent skill 里强行注入 action.request 调用，但 GUI / 通知端没有对应 UI，反而退化体验。

**建议**：
- §2.5 + §11 把 action 从 Done 条件中**降级为可选增强**（"迁移完成后再启用"）。
- 迁移期保留"普通 event 文本 + 用户回复"作为审批 fallback。
- action 本身值得做，但应作为独立 change 推进，不绑定 dev-helper 下线。

---

## 2. Agent runtime 设计层面（§4.1 / §5 / §8）

### 2.1 prompt 模板设计 — 字段近似齐全，但漏了行为约束

§5 列出 `firstTurnPrefix` / `everyTurnPrefix` / `everyTurnSuffix` + 模板变量（`{actor.id}` / `{workspace.dir}` / `{work_item.id}` 等），覆盖了 `run-agent.sh build_prompt` 里 `[joi handoff v1]` + `[joi bootstrap]` 的主体。

**问题**：现有 bootstrap 还有这些**行为级约束**没被模板字段表达：

1. **router 第一轮 fast-path**：`run-agent.sh` 在 router 首轮注入 "立即创建一个 task thread + 写 work_item runtime_state"，本质是 skill 行为契约。
2. **delivery 仓库守卫**：`require_delivery_handoff_inputs` —— 6 项 stg key 缺任何一个就硬退出。这是 first-turn 之前的 *gate*，不是 prompt。
3. **discovery 只读策略**：当 scope 的 `read_only=1`（resident discovery thread）时，`run-agent.sh` 拒绝写入分支并提示 skill。
4. **completion sentinel**：`everyTurnSuffix` 只写"emit configured Joi completion sentinel"，没规定"哪个字符串、是哪个 provider 的 stop pattern"。Copilot vs Claude 是不一样的。

**建议**：在 §5 之后新增 §5a "Agent dispatch gates"，把上面 1–4 形式化成 AgentSpec 字段：
```jsonc
{
  "preFlight": {
    "requireWorkspaceFiles": [".joi/work-items/{work_item.id}/clone_manifest.json", ...],
    "abortIfMissing": true
  },
  "scopeRoles": { "resident_discovery": { "readOnly": true } },
  "completionSentinel": { "copilot": "...", "claude": "..." }
}
```

### 2.2 dispatch 状态格式（§6.3） — 字段不全

方案给的字段：
```json
{ "last_processed_event_id", "last_processed_occurred_at", "last_turn_kind", "provider_session_id" }
```

**问题**：现有实现还有一组**会话失效字段**，决定 "下一轮是 resume 还是 first_turn"：
- `bundle_version`
- `active_skill`
- `agent_provider`
- `skill_dir`
- `project`（待替换）
- `workspace_dir`

任何一项变化都会触发废弃旧 session、强制 first_turn。如果迁移后只保留 `provider_session_id` 而没有 invalidation 字段，**bundle 版本切换不会触发新会话**——agent 会拿着旧 session 跑新 skill，行为漂移。

**建议**：§6.3 字段表加一节 `validity`：
```json
{
  "validity": {
    "bundle_version": "...",
    "active_skill": "...",
    "agent_provider": "...",
    "skill_dir": "..."
  }
}
```
并明确 dispatch 算法：`if validity_diff(stored, current) -> drop session_id, mark first_turn`。

### 2.3 单实例并发 — 设计静默

§7 给 service supervisor 写了 "single-instance guard"。§8 讲 agent 迁移时**完全没提**。

**问题**：现有 `run-agent.sh` 用 `flock -n` 防止同 actor×scope 并发派发。这个保护必须迁到 `joi agent serve`，否则在 reconnect/race 下会出现两次 LLM 调用并发（双倍消耗、双倍 reply、session 状态错乱）。

**建议**：§8 显式声明 "agent runtime 必须保证 actor×scope 并发度 = 1，重叠 trigger 走 dedup（保留 last 或 fold）"。

### 2.4 work_item_id 提取 — 形式化的责任不清

§5 的模板变量表写 `{work_item.id}` 来源是 "workspace metadata / prompt parser / event metadata"。

**问题**："prompt parser" 是个含糊的责任。当前实现是 `run-agent.sh` 用 grep/regex 从 trigger event text 里抓 `work_item_id: <id>` / `stg prefix: work_item/<id>`。这种抓取是脆弱的业务约定，不应进入 runtime 的稳定接口。

**建议**：把 work_item_id 抬升为一等概念。可选两条路（任选其一）：
- (A) event relation：`work_item -> work_item:<id>`，由发送方（router / discovery / delivery）显式标注；agent runtime 直接读 relation。
- (B) thread workspace canonical_work_item_id：迁移时把每个 thread 的 canonical work_item_id 写进 `.joi/state/scope.json`，agent runtime 读 workspace 而不是解析文本。

不要保留 regex parser。

### 2.5 actor alias（researcher → discovery） — 没提

`run-agent.sh` 有 `case "$ACTOR" in researcher) ACTOR="discovery" ;;` 的 back-compat 别名。历史 event 里仍可能 handoff 到 `researcher`。

**建议**：AgentSpec 增加 `aliases: ["researcher"]`，由 agent runtime 在 inbox dispatch 阶段统一别名解析。

---

## 3. Workspace / Artifact 设计层面（§4.2 / §4.3）

### 3.1 readonly-repos 完全没有归位

`readonly-repos.sh` 在远端做的事：
- 维护 `channel_repo_ids()` 硬编码映射（`a1-auto-dev` → 19 个仓库）。
- 用 `git clone --shared <AUTO_DEV_BASE_DIR>/<repo>` 在 `channel_ws/shared/repos/` 建轻量克隆。
- 在 discovery / delivery workspace 通过 symlink (`workspace/shared -> channel_ws/shared`、`workspace/repos -> channel_ws/shared/repos`) 共享缓存。
- 写 `.readonly-repos.json` 元数据；router 通过 `workspace.readonly_repo.refresh/clone/read` 触发同步。

**方案完全没提这套。**§8 那行 `dev_helper_project -> scope workspace path` 是错的简化——readonly-repos 不是 dev-helper project，它是**跨 scope 共享的源码缓存层**。

**建议**：在 §4 新增 §4.5 "Repo cache（只读源码缓存）"，明确：
- 是 `joi workspace` 的子能力还是单独的 `joi repo-cache` CLI / ServiceSpec。
- 谁维护 `channel → repo_id[]` 映射（建议放 channel workspace `.joi/state/repos.json`，由 channel 创建时显式声明，去掉硬编码）。
- 缓存的物理位置（建议固定为 `channel_ws/.joi/repos/`，不再 `shared/repos/` 双层）。
- 重新同步触发方式（schedule service or on-demand `joi workspace repo-sync`）。
- thread workspace 如何获取仓库视图（symlink 还是 bind mount 还是直接路径变量 `JOI_REPOS_DIR`）。
- `git clone --shared` 优化（依赖 `AUTO_DEV_BASE_DIR` 这个外部约定）是否保留。

### 3.2 delivery workspace provision 没有归位

`provision_delivery_workspace()` 在 first turn 做：(a) 读 `clone_manifest` → git clone target + ref repos；(b) `openspec init --tools claude` 写出 `openspec/changes`、`.claude/commands/opsx/`、`.claude/skills/`；(c) 写 `.dev-helper/delivery-provision.json`；(d) `require_delivery_handoff_inputs` 检查 6 个 stg key 不为空。

**方案在 §8 只写一句 "delivery / openspec mandate"。**

**建议**：
- 把 clone_manifest schema 写进 §4.2 / §6（target_repo + ref_repos[]，每个 entry 含 repo_id / clone_url / base_branch / work_branch）。
- 在 AgentSpec 新增 `provision`（声明性的 git clone + tool init 步骤），由 agent runtime 执行；不要让 skill prompt 自己 shell out。
- "handoff inputs gate"（6 项 workspace 文件存在性）放进 §2.1 提到的 `preFlight` 字段。

### 3.3 a1 kbase repo-notes 没有归位

`joi_rpc.py repo-stg-write/read/verify` 维护 a1 kbase 上的 page（`stg__repo__<repo_id>__dev-notes`，`[repo-dev-notes v1]` 结构块），并在 stg 镜像 5 个 KV key。

**问题**：方案 §4.2 把 `repo_notes` 当作普通 workspace 文件，但**真相源在 a1 kbase**，workspace / stg 都是镜像。彻底下 dev-helper 不等于下 a1。这一点必须明确：
- a1 kbase 是独立外部系统，必须保留。
- workspace 文件是 cache / mirror，不是 source of truth。
- `repo-stg-verify`（校验当前 workspace 仓库 `clone_url`、`canonical_repo_id` 与 kbase 记录一致）是 delivery 安全网，不能丢。

**建议**：在 §4.3 / §4.5（新增 repo cache 那节）说明：
- repo-notes 是"外部 SoT + 本地 mirror"模式，不是普通 workspace state。
- 提供 `joi repo-notes pull / push / verify` 这种语义封装（替换 `joi_rpc.py repo-stg-*`），底层仍调 `a1 kbase`。
- 或者：把 repo-notes 显式声明为 artifact 类型 `repo-notes://repo/<repo_id>@<version>`。

### 3.4 artifact 与 attaches_artifact relation 实施依赖

§4.3 / §11 要求 validation_report / release_receipt / runtime_receipt 等用 artifact 沉淀，并通过 `attaches_artifact` relation 引用。

**问题**：§4.3 自己在脚注承认 "如果现有 event append 尚不支持 attaches_artifact 参数，需要补齐"。这件事是迁移路径上的硬依赖，应进入 Phase 1 的 Workspace/Event CLI 任务，而不是写在边注里。

**建议**：把 "event append 支持 `--attach-artifact`"、"artifact publish 与 event relation 端到端打通" 显式列为 Phase 1 必做项。

---

## 4. Service runtime 设计层面（§7）

### 4.1 ServiceSpec 例子覆盖不全

§7 的 ServiceSpec 例子是 `command_service` + `mode: schedule`，channelId 写死。

**问题**：
- 现有 4 个 service 形态各异：
  - **a1-e2e-trigger**: 每天 10:00 schedule，scope 是 thread `thread_e6378f8cc105`。
  - **a1-e2e-scanner**: 30 min poll，scope 是 thread `thread_476a0d7de723`（与 trigger 不同 thread）。
  - **mr-watcher**: poll，无固定 scope，poll.py 自己枚举 `joi thread list` 找有 `[mr-opened v1]` event 的 thread。
  - **feedback-scanner / triage-writer**: subscribe + once，由 handoff event 触发，retry=2/delay=1。
  - **feedback-fix-orchestrator**: 是 LLM service，走 `run-agent.sh` 不是 Python service（介于 agent 和 service 之间）。
- ServiceSpec 字段必须覆盖：`mode in {schedule, poll, subscribe, once, hybrid}`、`scope`（channel / thread / scope-discovery via event listing）、`onTriggerEvent`（filter pattern）、`retry`、`backoff`、`logRotation`。

**建议**：把 §7 的 ServiceSpec 表格扩展成完整字段表，并把上面 5 个真实 service 各画一份 spec 当 worked example，验证 schema 覆盖度。

### 4.2 once + retry 没规定

feedback-triage 的 `once_one` 有 retry 循环（默认 2 次，1 秒间隔），还输出 `retrying` 状态 JSON。

**问题**：方案 §7 只写"start/stop/restart/status/once"，没说 once 的 retry 语义。

**建议**：补 `retry: { max: 2, delayMs: 1000, emitStatus: true }` 字段。

### 4.3 LLM service（feedback-fix-orchestrator / mr-watcher 现状）归位

mr-watcher 现状是"`run-agent.sh` 启动 + 内部跑 `poll.py`"，混合了 agent 和 service 两种性质。feedback-fix-orchestrator 是纯 LLM，但在 `agents/services/` 目录下注册成 service。

**问题**：方案 §7 说 "interactive_command 不适合替代长驻 service"，但没说**"服务发起的 LLM 调用"**怎么走（mr-watcher 在 merge 时需要让 LLM 总结一段交付总结；feedback-fix-orchestrator 整个就是个 LLM）。

**建议**：明确 service runtime 与 agent runtime 的协作模式：
- service 想触发 LLM 时，应 `event/append --handoff <agent>`，让 agent runtime 唤醒 agent，不要 service 自己 spawn `interactive_command`。
- feedback-fix-orchestrator 应该直接归类为 agent，不是 service（它具备完整 agent 能力，只是入口靠 service 触发）。

### 4.4 hardcoded scope id

`a1-e2e/servicectl.sh` 硬编码 `JOI_SCOPE_ID=chan_bcf8e1e730bd` 和两个 thread id。`feedback-triage/servicectl.sh` 同。

**问题**：方案 §7 ServiceSpec 例子里 `channelId: "chan_bcf8e1e730bd"` 同样是硬编码——只是从 shell 搬到了 JSON。如果接受这种"per-deployment 写死 scope id"，远端只有一份就行；如果要 multi-deployment 还需要参数化。

**建议**：明确选边——如果接受 spec-level 写死 scope id，那就在 §7 写明 "ServiceSpec 是 deployment artifact，不是可移植定义"；如果要多部署，加 `scope: { kind: thread, idFrom: "config:a1_e2e.trigger_thread_id" }` 这种间接寻址。

### 4.5 log rotation 等运维项

现状所有 service 用 `>> log_file` 无限累积，没有 rotation。MR watcher 自 4/28 一直在跑。

**建议**：ServiceSpec 增加 `log: { rotate: { sizeMb: 50, keep: 5 } }` 字段，`joi service serve` 接管轮转。

---

## 5. 状态迁移 / 数据保留（§9 / §12）

§9 Phase 5 写 "归档或删除旧 runtime-tools"；§12 风险表写"迁移脚本先 dry-run"。

**问题**：远端**正在运行**：
- classmaster Copilot session `da6c155f-f3eb-4c5d-9480-eaedc93e2c4b` 在 channel `chan_68b967d1627f`（PID 3383224 active）。
- a1-e2e-trigger PID 3795408、a1-e2e-scanner PID 2294659。
- 所有 dispatch-state 文件、scope-projects、channel-state 都包含活动业务 cursor。
- stg 里有正在进行中的 work item（execution_plan / progress_snapshot 不为空）。

如果按 Phase 5 直接归档，会发生：
- classmaster 失忆（Copilot session 不再 resume，下次 trigger 当成 first_turn 重启）。
- mr-watcher 丢 `seen_note_keys` / `merged_emitted` flag，对所有历史 MR 重发"merged"通知。
- a1-e2e-trigger 丢 `gap_fingerprint`，可能重复触发 pipeline。
- discovery / delivery thread 丢 `clone_manifest` / `task_goal`，下一轮 delivery 拒绝继续。

**建议**：在 §9 之前新增 §8.5 "数据迁移"，至少包括：

| 旧路径 | 新路径 | 迁移工具 | 切换策略 |
| --- | --- | --- | --- |
| `~/.local/state/joi-agent/<actor>/<scope>.session` + `.session-meta.json` | `<workspace>/.joi/dispatch/<actor>.json`（含 validity 字段） | `joi migrate sessions` | 双写一周→切换 |
| `~/.local/state/joi-agent/dispatch-state/<actor>/<scope>.json` | 同上 | 同上 | 同上 |
| `~/.local/state/joi-agent/channel-<cid>.json` | `channel_ws/.joi/state/scope.json` | `joi migrate channels` | 一次性 |
| `~/.local/state/joi-agent/scope-projects/<kind>-<id>.json` | `<scope_ws>/.joi/state/scope.json` | `joi migrate scope-projects` | 一次性 |
| `~/.local/state/joi-agent/services/*/(scanner|trigger|writer)-*.json` | `<scope_ws>/.joi/services/<id>/cursor.json` | `joi migrate service-state` | 双写过渡 |
| `~/.local/state/joi-agent/mr-watcher/state.json` | `<thread_ws>/.joi/services/mr-watcher/state.json` 或 channel 级聚合 | `joi migrate mr-watcher` | 双写过渡 |
| `context-share kv-*` (work_item/* + repo_notes/* + capability_atlas + e2e-* + ...) | workspace `.joi/work-items/...` / `.joi/services/...` / `.joi/repos/...` | `joi migrate stg` | 双写过渡 |
| `~/joi-workspaces/{channel,thread}/<slug>/` 现有目录 | 同位置（保留），加 `.joi/` 子树 | 在原地补 `.joi/` | 不动旧目录 |

每条迁移建议 dry-run + 校验报告（"迁移前后字段计数一致"）。

并且在 cutover 那一刻必须保留：
- 当前 Copilot session_id（classmaster `da6c155f-...`），让 `joi agent serve` 第一次唤醒时继续 resume，不要 first_turn。

---

## 6. Done 条件层面（§11）

### 6.1 第 6 条 "控制面标准化"

如 §1.3 所述，action 是 net-new 能力，不应进入"必须满足"列表。

**建议**：把第 6 条改成 "提供 action.request / action.response 协议路径，并由至少一个 agent skill 演示用法"，不要求把所有人工确认换成 action。

### 6.2 第 7 条 "Agent 可恢复"

"按 actor/scope resume，model/settings 变更能创建新 session" — 方向对。

**建议**：补充字段："session validity 字段集合包含 `bundle_version`、`active_skill`、`agent_provider`、`skill_dir`，其中任一字段变化触发 fresh session。"（与 §2.2 的修订对齐）

### 6.3 缺一条：repo cache + provision 验收

**建议**：新增第 12 条："仓库缓存可用：channel workspace 的 `.joi/repos/` 或等价路径下，readonly 源码可被 router/discovery 使用；delivery workspace 的 clone+openspec init 能在首轮 trigger 时由 agent runtime 自动完成。"

---

## 7. 阶段顺序的可执行性（§9）

§9 给的 5 个 Phase 顺序基本合理，但有两个隐性依赖没排：

1. **Phase 1 `joi workspace` CLI 必须先于 Phase 3**（agent migration），否则 classmaster/teacher 迁过来读不到 work-item 状态。这一点设计已经隐含但没显式声明依赖。
2. **Phase 4 service supervisor 应早于 a1-e2e service 迁移**，但 a1-e2e service 是 schedule + poll 模式，跟 feedback-triage 的 once + retry 模式不同。建议 Phase 4 拆成 4a（supervisor + once/retry，迁 feedback-triage）和 4b（schedule/poll，迁 a1-e2e + mr-watcher），错峰减小 blast radius。
3. **数据迁移 (上面 §5) 是 Phase 3 / Phase 4 的前置条件**，应作为独立 Phase 0.5。

**建议**的修正阶段：
- **Phase 1**: Workspace + Event CLI（含 `--attach-artifact`）。
- **Phase 2**: Prompt template + AgentSpec preFlight + dispatch validity 字段。
- **Phase 2.5**（新）: 数据迁移工具 + dry-run 报告。
- **Phase 3a**: 迁 classmaster + teacher（双写 stg / workspace 一周观察）。
- **Phase 3b**: 迁 router + discovery + delivery（含 readonly-repos 替代）。
- **Phase 4a**: Service supervisor 基础能力（once/retry/schedule/poll/subscribe）+ 迁 feedback-triage。
- **Phase 4b**: 迁 a1-e2e + mr-watcher（含 LLM 协作模式）。
- **Phase 5**: 真正下线 dev-helper / context-share / runtime-tools。

---

## 8. 风险表（§12）补充

需要补的风险：

| 风险 | 应对 |
| --- | --- |
| Cutover 时活动 Copilot session 失忆 | 迁移 session_id + validity 字段；首次唤醒走 resume，不走 first_turn |
| readonly-repos 缓存丢失导致 discovery / delivery 无源码 | 切换前先在新 workspace 路径下完整 sync 一次；保留旧 `shared/repos/` 不删 |
| a1 kbase 写权限丢失 | 新 `joi repo-notes` 包装必须先验证 token / kbase 权限（dev-helper 外取得 a1 CLI 凭证） |
| mr-watcher 状态迁移导致历史 MR 重发通知 | 必须迁 `seen_note_keys` / `merged_emitted` / `closed_emitted`；切换前 dry-run 校验 |
| a1-e2e-trigger 重复触发 pipeline | 迁 `gap_fingerprint` 后比对一次；切换日志保留 |
| feedback-triage retry 行为变化 | retry 配置必须等价（max=2 / delay=1s / emit retrying status） |
| `context-share` 进程下线时间点 | 必须在所有 stg-key 迁移完成且双写关闭之后；不能与 Phase 3/4 并发 |
| `researcher` 别名失效 | AgentSpec aliases 字段保留至少一个 release 周期 |

---

## 9. 总评

方案的"四个一等概念替换 dev-helper"思路是正确的，但**作为一份能直接落地实施的方案，目前主要缺三类东西**：

1. **完备性**：readonly-repos、delivery provision、a1 kbase、stg key 全集映射、actor alias、completion sentinel、preFlight gate、log rotation 等这些"在跑的能力"必须每一项都进设计。
2. **可执行性**：dispatch validity 字段、单实例并发、work_item_id 提取、once+retry、Service mode 矩阵需要在协议字段层面定死，不能停在"需要补齐"。
3. **数据迁移**：必须补一份具体的"旧路径 → 新路径 + 迁移工具 + 切换策略"清单，并把它升级为独立 Phase。

action 化建议从 Done 条件中拆出，作为独立路线推进，避免阻塞 dev-helper 下线。

按本文档的修订意见更新 design 后，再执行 §7 的修正阶段顺序，迁移可以在不丢失现役上下文的前提下完成。
