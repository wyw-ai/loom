# e2e-readiness-review

> 进入 p5-e2e/1（本地 e2e）前对上一会话 9 个 commit（568cf29..073ea03）相对
> v4 设计 + artifact-contracts 的偏差盘点。本文不是设计修订；是 review 清单。
> 分类：**必修**（不修则 a1-auto-dev / classroom 剧情跑不下去）、
> **可观察**（e2e/1 跑一次就知道要不要修）、**无影响**（记一下不动）。
>
> 评判尺度：(1) 严格遵循 Joi 设计理念（actor/channel/thread/event/workspace/
> artifact/action/agent/service），非必要不增概念；(2) 验收必须能跑通
> a1-auto-dev（router→discovery→delivery + per-thread mr-detector）和
> classroom（教案 artifact → approval.spec_apply → reload → 回归）两条剧情。

## 必修（M）

### M1. `{spec.dir}` 占位符未实现，mr-detector 拉不起来
- 现状：`data/services/mr-detector/spec.json` 引用 `{spec.dir}/bundle/poll.sh`；
  但 p4c/3（cd3d7cf）`build_substitutions` 只生成 `{instance.data_dir}` /
  `{thread.id}` / `{channel.id}` / `{params.X}` 四类。`{spec.dir}` 字面量被
  原样传给 bash，命令执行必然 ENOENT。
- 设计依据：§4.7.3 "scheduler 跑、emit event、self_complete" 隐含的前提是
  scheduler 能找到 plugin 脚本。spec 自描述布局（spec.json + bundle/）是
  §4.6 ServiceSpec 的标准形态。
- 影响：a1-auto-dev p5-e2e/1 第 4 步 "joi service start --spec mr-detector"
  起不来 → 任务完成态 MR 监听完全断链。
- 建议修法：在 `build_substitutions` 增加 `{spec.dir}`（= ServiceSpec 加载时
  记录的目录，需要 `ServiceSpec` 上加 `loaded_from: PathBuf` 之类的运行时
  字段，或在 ServiceContext 里附加），同时考虑 `{bundle.dir}` 做语义清晰
  化。这是已有元语下的最小补丁，不引入新概念。

### M2. `repo-provision` ServiceSpec 是孤儿：thread.create 不调用它
- 现状：a3313c6（`joi thread create --bootstrap-artifact`）解析 artifact
  得到 `mounts[]` 后**只写 thread `scope.json`**；不读 `repo-provision`
  spec、不执行 `provision.sh`、不发 `thread.bootstrapped` event、不发布
  `repo-provision-receipt.json` artifact。
  与此同时 `data/services/repo-provision/{spec.json,bundle/provision.sh}`
  完整 commit 进仓库（429761e），plugin kind=`command`，trigger=`thread.bootstrap`
  ——`thread.bootstrap` 这个 trigger kind 在 host 里也没对应 dispatcher。
- 设计依据：§4.7.2 明确 "按部署声明的 bootstrap hook（一个 ServiceSpec，
  例如 `repo-provision`）"；§12 Done 第 9 条要求 "delivery 首轮可根据 clone
  manifest artifact 自动 clone、初始化 OpenSpec、校验 repo notes"；
  artifact-contracts §3 clone-manifest.json 的 consumer 同时是 delivery
  *和* `joi thread create --bootstrap-artifact`——但下游的 `repo-provision`
  receipt 路径目前没人产出。
- 影响：现在仓库里**两套实现并存**：
  - 实际跑的：`agent_serve` 拿 thread `scope.json.mounts` 做 symlink/worktree
    投影（Phase 1 chunk 3 的产物，2239aa6 / 8e732a5）。
  - spec 层声明的：`repo-provision` 命令式 bootstrap hook（429761e）+
    receipt artifact——一行代码都没跑。
- 建议修法（二选一，必须挑一个落地）：
  - **(a) 删 repo-provision spec**，明确 mount 投影即 bootstrap：a3313c6 已
    满足 §4.7.2 的"声明 mounts"语义，spec 层不再需要 ServiceSpec。代价：
    放弃 receipt artifact，§12 Done 第 9 条改成"thread workspace 内 repos/
    可访问"。
  - **(b) 保留 repo-provision，把 thread.create 接进去**：thread.create 在
    写 mounts 之前先发 `joi service once repo-provision` 之类的 trigger，
    receipt 写回 `scope.json` 后再放行 agent dispatch。代价：要给 service
    runtime 加一个新的 `command` plugin trigger kind = `thread.bootstrap`（不
    是新概念，是已有 command plugin 的 trigger source 扩展）。
- 我倾向 (a)：mounts 已经表达了"按 artifact 准备 worktree"，再多一个
  service hook 是双重簿记，违反"少造概念"。

### M3. mr-detector 输出走 `content.add`，不是 `mr-event-*` artifact + status.update
- 现状：`bundle/poll.sh` 把 `{schema_version, producer, event_kind, mr_id,
  ...}` JSON 行 print 到 stdout；scheduler 把每行变成普通 `content.add`
  event 投到 thread。没有 `joi artifact publish`，没有 `status.update`。
- 契约依据：`docs/artifact-contracts.md` §6 明确 "Each MR transition is
  published as its own artifact and announced via `status.update` event。
  The artifact name is `mr-event-<event_kind>-<mr_id>.json`"。
- 影响：delivery agent 按 §4.3.1 的契约从 `event.attaches_artifact` 反查
  MR 事件——查不到，因为没人发 artifact、没人发 status.update。剧情
  "detector 发现新事件 → handoff 回 delivery 修复"在协议层就断链。
- 建议修法（保留两个候选，对齐时拍板）：
  - **(a) poll.sh 自己调 joi CLI**：在 `provision.sh` 里 `joi artifact
    publish --file <stdout-line>` + `joi event append --type status.update
    --in {thread.id} --attach-artifact <uri>`。需要 poll.sh 有 cli + scope
    上下文（已经通过 `{thread.id}/{channel.id}/{instance.data_dir}` 占位符
    具备，再加 `{spec.dir}` 后完整）。最 Joi-native，不动 scheduler。
  - **(b) scheduler 学会按 spec 配置发 artifact**：spec.config 加一个
    `publishAs: "artifact"` 选项，scheduler 发现 stdout 行带 `event_kind`
    时自动 publish + status.update。优点：plugin 脚本不依赖 joi CLI；缺
    点：scheduler 概念变大。
- 我倾向 (a)：poll.sh 早就需要 jq + bash 这类外部依赖，多依赖 joi CLI 不
  增本质复杂度，scheduler 保持纯调度。

### M4. `auto_stop_on: thread.closed` 声明了但代码不识别
- 现状：mr-detector spec 写 `auto_stop_on: ["thread.closed",
  "service.self_complete"]`；scheduler 只在 self_complete 时 abort（88cccd2）。
  host watcher（7d07e55）只在 `request.json` 被删除时 stop instance。
  thread 被关闭后没有触发链通知 host。
- 设计依据：§4.7.3 "thread 关闭、或实例自检完成时...service host 自停实例
  并清 pid"。
- 影响：手工关闭 thread 时 mr-detector 进程泄漏；e2e/1 反复试跑时会留下
  僵尸 instance。
- 建议修法：host 在 `supervise_instances` poll 周期里查 thread 状态（已经
  有 client → server 通道），thread.closed 时主动删 request.json + abort。
  或者最低限度：监听 `thread.closed` event 投递。

### M5. classroom 教学循环最后一公里 (`approval.spec_apply` → 写 spec → reload) 未实现
- 现状：
  - AgentSpec 上 `approval.spec_apply` action 协议层已在（dcd74f4 prompt
    template 一并提到）。
  - `joi agent reload <id>` / `joi service reload <id>`（6e997c8）只 bump
    epoch marker，host（58ca3f4）watch 后重启 plugin。
  - **缺失**：消费 `approval.spec_apply` action.response 的逻辑——把教案
    artifact 内的 spec/bundle diff 落到 `data/agents/<id>/spec.json` 或
    bundle 文件，然后触发 reload。这一段 commit 历史里看不到。
- 设计依据：§7.1 "reload 触发由 deployment service 在消费
  `approval.spec_apply` 后发起"；§12 Done 第 14 条 classroom 教学循环
  收口。
- 影响：classroom 剧情"教案 artifact → approval.spec_apply → reload →
  回归"在第 2.5 步断链。e2e/1 第 5 步无法演示。
- 建议修法：实现一个 small `joi spec apply --action <event_id>` 子命令
  （或 `joi action accept` 的 side effect），按教案 artifact 的 schema
  写入 spec/bundle 后触发 reload。这是 "已有 CLI 的胶水"，不引入新概念。

## 可观察（O，e2e/1 真撞上再修）

### O1. `load_specs` 要求扁平 `<dir>/<id>.json`，仓库布局是 `<dir>/<id>/spec.json`
- 上一会话交接文档 §7 第 1 项已自陈。e2e/1 启动 `joi service serve` 第一
  下就会撞。修法二选一：
  - 让 `load_specs` 递归识别 `*/spec.json`（最小改动，符合"少造概念"）。
  - 加一个 `joi service install <bundle-dir>` 把 `<id>/spec.json` 软链到
    `~/.config/joi/services/<id>.json`（更显式但多一步）。
- 倾向递归识别。

### O2. `instance_id == thread_id` 单一约束
- 上一会话已自陈。a1-auto-dev 单 thread 多 MR 时会冲突。当前剧情每个
  delivery thread 一个 task / 多 MR 暂不明确，先观察 e2e/1。

### O3. grep guard ALLOWED_PREFIXES 整文件白名单
- `crates/cli/src/{cmd/spec.rs,cmd/workspace.rs,main.rs}` 整文件放行。
  风险低（这些文件未来也会引用设计文档名），先放着。

### O4. `migrate-dev-helper` flaky 测试
- `tests::dry_run_with_no_sources_is_clean` 偶发失败，rerun 即过。e2e
  阶段如再翻车，根因 + 修。

### O5. `joi service start` 没有要求 `--params` 与 spec.params_schema 校验
- start 写 request.json 时只解析 JSON，不校验 schema。host 端也未见严格
  校验。不挡 e2e，但生产时要补；列入收尾。

## 无影响（N）

### N1. AgentSpec aliases / displayName 中文化纯文案，不动
### N2. service `self_complete_on` 字段在 mr-detector spec 顶层 config 里
出现但 scheduler 不读（实际靠 poll.sh 写 sentinel 行 + scheduler strip）。
冗余声明，e2e 后清掉。

---

## 总评

设计理念遵循度：**总体不偏**——p4a/p4b/p4c/p5 的 9 个 commit 都是在
ServiceSpec 已有字段上做 typed 化 + 复用 scheduler 模型 + 复用 reload-epoch
marker，没有造新一等概念。p4c/3 的 placeholder 替换是"在已有 plugin 内做
小工具"，没把它推成 runtime 概念，**这是对的**。

但**有两类断链需要修**才能进 e2e/1：
1. **路径占位符不全**（M1）+ **scheduler 输出未走 artifact**（M3） → mr-detector
   作为 a1-auto-dev 验收第二腿，没法在协议层正确说话。
2. **bootstrap hook 双轨制**（M2）+ **classroom 收口缺胶水**（M5） → 两个剧情各
   缺最后一公里。

**M1 / M3 / M5 必须修在 p5-e2e/1 之前**，因为它们决定剧情能不能走完。
**M2 必须先 ask_user 拍板 (a) 还是 (b)**，否则 commit 走错方向。
**M4** 在 e2e/1 里只要不反复 close thread 就不致命，可以放到 e2e/1 中段补。

---

## 给下一步的建议（待 ask_user）

按依赖顺序：

1. ask_user：M2 选 (a) 删 repo-provision，还是 (b) 接进 thread.create？
2. p5-e2e/0a：补 M1 `{spec.dir}` 占位符（小 commit）。
3. p5-e2e/0b：按 M2 决议执行——若 (a) 删 spec + bundle + 文档；若 (b) 加
   command plugin 的 thread.bootstrap trigger。
4. p5-e2e/0c：补 M3 mr-detector 调 joi CLI 发 artifact + status.update。
5. p5-e2e/0d：补 M5 `joi spec apply` 胶水。
6. p5-e2e/0e：补 O1 `load_specs` 递归识别（最低成本让 serve 跑起来）。
7. p5-e2e/1：本地双剧情 e2e。

如果 ask_user 反馈"先进 p5-e2e/1，撞墙了再修"，则跳过 0a-0e；但根据上面
的分析，M1 / M3 / M5 是**协议层断链**，e2e/1 第一个剧情不到一半就会停。
建议至少先修 M1 + M2（拍板）+ M3。

---

## Session 2 复评（HEAD = `fcf0638`，2026-05-03 第二轮会话）

> 本节是新一轮会话独立形成的判断，不是简单"对照 checkbox"。基线是
> 上一会话的 M1–M5 / O1–O5，加上自 073ea03 起新增的 13 个 commit
> （5386a3c..fcf0638）。重点回答两个问题：(a) 新 commit 有没有偷偷
> 引入新概念？(b) e2e/1 的"剧情完整跑通"标准已经达到了吗？

### 已闭环（不再阻塞 e2e/1）

| 原 ID | 状态 | 闭环 commit | 校验 |
|---|---|---|---|
| M1 `{spec.dir}` 占位符 | ✅ | `5386a3c` | ServiceContext 加 `spec_path: Option<PathBuf>`，scheduler `build_substitutions` 派生 `{spec.dir}` / `{bundle.dir}`；`load_specs` / `resolve_spec_path` 同时识别 nested + flat。无新顶层概念，是已有 substitution 表的扩展。判定：合规。 |
| M2 repo-provision 双轨 | ✅ (a) | `ff538a9` | 删除孤儿 spec，§4.7.2 + §7 + §12 文档同步改写"mounts 即 bootstrap"。clone-manifest consumer 限定为 discovery/router。判定：合规，符合"少造概念"。 |
| M3 stdout → artifact + status.update | ✅ | `2c87f9d` + smoke `81d91b3` | JobSpec 新增 opt-in `emit.mode = "artifact_per_json_line"` 字段，复用 `artifact` / `status.update` / `attaches_artifact` 既有元语；mr-detector spec 切换到该模式。判定：合规，扩展位是 JobSpec 配置项不是新概念。 |
| M5 `joi spec apply --action` | ✅ | `06db5fe` + smoke `662e16b` + CLI 修复 `a6ceeef` | 新子命令读 action.response → fetch lesson-plan artifact → JSON deep-merge spec_patch → 写 bundle_writes → bump reload epoch → 发 runtime_receipt + status.update。lesson-plan `spec_apply` block 已写入 artifact-contracts.md §4。无新协议/新事件类型。判定：合规。 |
| O1 nested spec 布局 | ✅ | 5386a3c 顺手做掉 | `load_specs` 现支持 `<dir>/<id>/spec.json`。 |

### 新发现并已修复（本会话动手）

| 编号 | 问题 | 闭环 commit | 备注 |
|---|---|---|---|
| S2-F1 | **`service.self_complete` 后 request.json 未被回收。** `crates/cli/src/service/host.rs::supervise_instances` 文档承诺 self_complete 删请求文件，实际只在外部 `joi service stop` 时 abort。M3 smoke 只能 WARN。属于 §4.7.3 "实例自检完成时 service host 自停实例并清 pid" 协议层断链。 | `fcf0638` | watcher 改用 `JoinHandle`，每 tick 检查 `is_finished()` 后 `delete_request`。M3 smoke 现 FAIL-strict。 |
| S2-F2 | M3 smoke 自停断言放宽。 | `fcf0638` | WARN→FAIL，回归保护。 |
| S2-F3 | `joi event append --content-type application/json` 把 body 套 `{contentType,text}` 信封，破坏 `payload.requestType` 解析；`--artifact-link` 误用 `kind: "links"`（proto 只允 `attaches_artifact`）；`joi spec apply` 直接读 env 不接 `--as`。 | `a6ceeef` | 三处都是真 bug，影响协议读出来的字段，不是 cosmetic。 |

### 必修（M，剧情完整性卡点）— Session 2 视角

#### M6 (= 原 M4 升级). `auto_stop_on: thread.closed` 仍未识别

- 现状：`mr-detector/spec.json` 仍写 `bind.auto_stop_on = ["thread.closed", "service.self_complete"]`；S2-F1 闭环了 self_complete 这一支，**thread.closed 这一支仍是死字段**。host watcher 不监听 `thread.closed` event，scheduler 也不查 thread 状态。
- 设计依据：§4.7.3 明确两条触发，缺一条则 e2e 重复 close-thread 留僵尸 instance；§12 Done #10。
- 影响等级：**剧情 A 收尾段会反复 close 同一个 delivery thread 做回归**，僵尸 instance 会让下一轮 `joi service start` 因 instance_id 冲突失败（O2 的实际触发面）。
- 建议：watcher 在每个 tick 多查一次 thread status（reuse 已有 client），thread 已 closed 则等同于 self_complete 处理（reap request.json + 落 join handle）。或者更轻量：订阅 `thread.closed` 事件流。前者一行 RPC 调用就够，倾向之。
- 工时估计：~50 行代码 + 复用 S2-F1 的 reap 路径。

#### M7. **真 LLM 双剧情尚未跑过任何一次**（最重要！）

- 现状：smoke 1.3 (mr-detector) 和 1.4a (spec_apply) 都是**人手驱动的 deterministic** 路径——脚本自己 publish artifact、append action.request、accept 选项、call `joi spec apply`。**不经任何 LLM**。
- 协议层验证 ✅，但 §1.5 / §12 Done #14 #15 验收点要求的 "router→discovery→delivery 自主链路" 与 "classmaster→teacher 自跑" **零覆盖**。
- 真 LLM 暴露的额外风险（已知会被烤出来的）：
  1. teacher SKILL.md 是否把 `spec_apply` block 的 schema 教给了 LLM？目前 SKILL.md 完全没提到这个 frontmatter 约定，LLM 出的 lesson-plan 大概率没有 spec_apply 块 → action gate 跑不动。
  2. router prompt 是否硬约束"严格 handoff，不要自答"？LLM 很容易直接回答用户问题，不走 handoff 链。
  3. delivery thread 起来后能不能用 `joi workspace path` + 已 mount 的 `repos/` 做出真 commit + push？bare-repo fixture 是否足够当 stand-in MR？
  4. settings-glm.json 的 model/quota 在压上 6 个 actor 同时跑时会不会被 throttle？
- 影响等级：**这才是 e2e/1 的真正验收门槛**。M1–M5 + S2-F1 全闭环，但只跑 deterministic smoke 就声明 "本地 e2e 通过" 是自欺——和上一会话警告的"hello-world scheduler 不算通过"同质。
- 建议：1.4b/1.4c 必须在 187 cutover 之前真跑。先做 1.4b classroom（spec_apply 链 LLM 端较单纯），再 1.4c a1-auto-dev（链路最长）。每一轮按必要性最小迭代 prompt。

### 可观察（O）— 状态更新

| 编号 | 状态 | 备注 |
|---|---|---|
| O1 nested 布局 | ✅ 已修 | 5386a3c |
| O2 `instance_id == thread_id` 单一 | ⏳ 仍存 | 实际触发面被 M6 放大；M6 修了之后此项再观察。本次剧情每个 delivery thread 单 task 单 MR，一对一映射可接受。 |
| O3 ALLOWED_PREFIXES 整文件白名单 | ⏳ 仍存 | 收尾再扫。 |
| O4 migrate-dev-helper flaky 测试 | ⏳ | 与 e2e 无关。 |
| O5 `--params` 不校验 spec.params_schema | ⏳ | 列入 p5-e2e/3 收尾。 |
| **O6 (新)** reload-epoch marker 断言 | ⏳ | `scripts/e2e/run-spec-apply.sh` 现在 **WARN** 找不到 marker，没有 FAIL-strict。修法：定位 `agent_marker_path()` = `<JOI_AGENT_DATA_ROOT>/agents/<actor_id>/.reload-epoch`（针对 `lesson-target` 是 `actor_lesson_target`），改为 FAIL。 |
| **O7 (新)** mr-detector spec.config `self_complete_on` 字段是死代码 | ⏳ | 之前 N2 就提过，scheduler 实际靠 stdout sentinel 行；这个 config 字段从不被读取，留在 spec 里会误导。建议 e2e 通过后清掉（与 ALLOWED_PREFIXES 收尾合并）。 |

### 无影响（N）

- N1 中文 displayName / aliases：不动。
- N2 `self_complete_on` 冗余字段升级为 O7，预计 e2e/3 删。
- **N3 (新)** `EmitConfig` 当前只支持一种 mode (`ArtifactPerJsonLine`)；以后如果要支持 line-by-line content event 等其他模式，是 enum 扩展，不是新概念。本期不动。

---

## Session 2 总评

设计理念遵循度：**继续合规**。新增的 `EmitConfig` / `spec_apply` CLI / `reload-epoch` 都建立在已有元语之上，没有引入第二套 service mode 枚举或新的事件类型。`spec_apply` 块以 lesson-plan artifact frontmatter 落地，是最干净的"用 artifact 表达可执行声明"案例。

**但 e2e/1 离"双剧情完整跑通"还差 M7**：真 LLM 链路一次没跑过，1.3 / 1.4a 的 deterministic smoke 只能证明协议互通，不能替代 §12 Done #14 #15。M6 是这次重跑前必须修的次要门槛（避免 thread close 时 instance 泄漏污染重试）。

**新一轮推进顺序建议**（按依赖与代价）：

1. **修 M6**（thread.closed teardown，~50 行，沿用 S2-F1 reap 通道）。同时把 O6 reload-epoch 断言改 FAIL。
2. **1.4b 真 LLM classroom**：扩 teacher SKILL.md 教 spec_apply block schema + 写 `scripts/e2e/run-classroom.sh`。允许多轮 prompt 迭代。
3. **1.4c 真 LLM a1-auto-dev**：写 `scripts/e2e/run-a1-auto-dev.sh`，串 router→discovery→delivery→`joi service start mr-detector`。
4. **1.5 联跑 + 文档**：`scripts/e2e/full-run.sh` + `docs/e2e-runbook-local.md`。
5. ask_user → ssh 187 进 p5-e2e/2 cutover。
6. p5-e2e/3：M6/O6 之外的 ALLOWED_PREFIXES 清理 + O5 / O7 收尾 + cutover 报告。

如果用户希望先把 M6 / O6 修掉再进 1.4b，是更稳妥的选择（避免中途因僵尸 instance 误诊 LLM 链路问题）。

