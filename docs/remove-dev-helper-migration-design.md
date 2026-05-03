# Remove dev-helper Migration Design

> 目标：彻底移除 dev-helper / dev-helper-bridge 依赖，用 Joi 原生 `event`、`artifact`、`workspace`、`agent runtime`、`service runtime` 承载现有 classroom 与 a1-auto-dev 链路。

## 1. 背景

当前远端 classroom / a1-auto-dev 链路中，Joi 已经承担了 server、event、actor、agent serve 等核心能力，但 agent 与 service 的一部分运行 glue 仍由 dev-helper 相关脚本承载：

- `dev-helper-bridge/run-agent.sh`：为 agent 拼接 handoff/bootstrap prompt，维护 scope/session/workspace 状态，并调用 dev-helper CLI。
- `dev-helper-bridge/joi_rpc.py`：提供 append、handoff、stg read/write、repo notes 等 helper。
- `bootstrap-channel.sh` / `readonly-repos.sh`：维护 channel workspace 与只读 repo 缓存。
- `servicectl.sh`：维护 a1-e2e、mr-watcher、feedback-triage 等长驻 service 的 pid/log/status/once。

这些能力本质上不是 dev-helper 专属能力，而是 Joi runtime 应该原生具备的能力。迁移目标不是把旧脚本名字搬进 Joi，而是把旧职责重新映射到 Joi 已有或应补齐的一等概念。

## 1.5 两个 channel 的设计意图

迁移后的 Joi runtime 必须承载两个 channel 现有的核心理念，而不仅仅是把旧脚本搬走。所有后续章节补齐的能力都为这两条服务。

### 1.5.1 classroom（小课堂）：可教学、可回归、可自我演化的工作流

classroom 把"维护本机 agent / service 链路"做成可教学的作业。教学循环：

1. classmaster 与用户沟通需求，整理成 task goal / DoD。
2. teacher 自己先按 task 跑一遍链路。
3. teacher **横向比对其它 channel / thread 在类似场景下的历史 event 与 artifact**。
4. teacher **读取被教 agent / service 的 spec、bundle、SKILL.md**。
5. teacher 产出教案 (lesson plan)；教案不只是报告，它包含针对目标 actor 的 spec / bundle 修改方案。
6. **应用教案** → reload 目标 agent / service → 在临时回归 thread 中拉起被教 actor 跑指定输入。
7. 通过则归档；不通过则回到 (5) 修教案再来一轮。

教学循环的"被教对象"既可以是其它 channel 的 agent / service，也可以是 classroom 自身的 actor，机制完全相同。

### 1.5.2 a1-auto-dev：管家 + 常驻调研 + 派生交付 + per-thread 监听

router 是 a1-auto-dev 的"管家"，承接用户、管理 thread、协调 agent / service。

- channel 公共区维护一份只读 `shared/repos/*`，由 repo-cache service 同步。
- channel 内有一个**常驻 discovery thread**，router 与 discovery 在其中多轮调研，确认 `DoD / 目标仓库 / 关联仓库 / 粗方案`。
- discovery 确认开发任务后，router 派生一个 **delivery thread**，**thread workspace 在创建时按 clone manifest 自动准备好目标 + 关联仓库的 worktree**。
- delivery 在 thread workspace 内对每个目标仓库依序走 `/openspec-propose → /openspec-apply-change → /openspec-archive-change`。
- 任务完成后由一个 **thread-bound MR detector** 持续监听该 task 相关 MR 的冲突 / 评论 / CI 状态；有事件就汇总成报告并 handoff 回 delivery 修复，直至任务完成 detector 自停。

### 1.5.3 共同支撑能力

承载这两个理念，在不新增 Joi protocol 概念的前提下，runtime / CLI / spec 需要补这几件事（详见后续章节）：

- 跨 scope 只读访问 event journal / artifact / spec / bundle（teacher 横向比对、读实现的前提）。
- channel scope metadata 上声明 **resident threads**（discovery 常驻 thread 的稳定地址）。
- workspace bootstrap **mount 机制**（thread workspace 投影 channel 公共目录或仓库 worktree）。
- thread 创建时按 artifact 触发 **bootstrap hook**（delivery thread 自动准备仓库）。
- ServiceSpec 支持 **thread-bound 多实例**（per-thread MR detector）。
- AgentSpec 上声明 **handoff trigger prompt prefix**（让 `/discovery` `/delivery` 这种 skill 触发被 runtime 强制注入）。
- AgentSpec bundle skills 在 dispatch 时按 **per-actor 路径**投影（`.joi/skills/<actor_id>/`）。
- spec / bundle **可读 + 可热重启**（教案应用回路）。

这些都是在已有概念上加 CLI 和声明字段，不新增 protocol。

## 2. 设计原则

### 2.1 不新增 stg / KV server 抽象

旧 bridge 中的 `stg` 是历史命名，不应该固化为 Joi 协议层的 `stg/read`、`stg/write` 或 server KV。

新的结构化状态分三类承载：

| 数据类型 | Joi 概念 | 语义 |
| --- | --- | --- |
| 时间线、handoff、状态播报 | `event` | 追加式、可订阅、驱动 agent/service |
| 可变工作状态、草稿、作业 | `workspace` | scope 下的普通文件，由 Joi 管路径与约定 |
| 不可变证据、报告、交付快照 | `artifact` | 可引用、可审计、可在 event 中挂接 |
| 人工确认、权限批准、方案选择 | `action` | 可选控制面事件，不承载长期状态 |

### 2.2 Server 保持消息枢纽

`joi-server` 不负责理解 classroom、a1-auto-dev、repo notes、work item 等业务概念。server 继续负责：

- actor connection
- event journal
- scope fanout
- actor inbox / delivery
- turn / trace
- artifact store
- channel/thread workspace projection

业务状态放在 workspace；业务证据沉淀成 artifact；业务流程通过 event 驱动。

### 2.3 Workspace 是“工作现场”

workspace 承载 mutable state，类似 Copilot session stats 或 agent worktree 内部状态：

```text
data/workspaces/
  channel/{channel_id}/
    .joi/
      state/
      repos/
      skills/  # runtime projection, not persistent state
    <business files...>

  thread/{thread_id}/
    .joi/
      state/
      skills/  # runtime projection, not persistent state
    <business files...>
```

Joi 提供 workspace 管理能力，但文件内容是业务约定，不进入 server protocol 的专用 KV。`assignments/`、`course/`、`work-items/` 等目录只属于 classroom/a1-auto-dev 的 skill 约定，不是 Joi runtime 标准目录。这个 scope-shared workspace 是对 `channel-workspace-model.md` 中 per-agent private workspace 的扩展：runtime 只负责解析当前 scope workspace、skills 投影和安全路径访问；业务文件布局由 skill / service plugin 定义。`skills/` 是 runtime 从 agent bundle / scope skills 投影出的只读目录，不是 workspace 的长期状态源。

### 2.4 Artifact 是“证据快照”

当 workspace 中的 mutable 文件达到某个验收点，需要对外引用或作为 DoD 证据时，发布成 artifact：

```text
workspace file -> artifact/publish -> artifact://... -> event relation attaches_artifact
```

这样可以避免 artifact 被滥用成 KV，又能让最终报告、验证记录、release receipt、runtime receipt 具备不可变引用。

### 2.5 Action 是“控制面”，不是状态面

`action.request` / `action.response` 用于承载需要人或上游 actor 做明确选择的短生命周期决策，例如确认、批准、拒绝、选择方案。它不存储长期状态，不替代 workspace，也不替代 artifact。

迁移期不强制把所有人工确认都改造成 action。远端现有 classroom 目前没有 action 流量，需求确认仍是普通 `content.add` event + 用户文本回复。因此 action 是推荐增强和后续收敛方向，不是下掉 dev-helper 的硬前置条件。迁移必须保留普通 event 文本确认作为 fallback。

适合放在 action 中：

- 权限批准：是否允许 agent 执行高风险命令。
- 需求确认：是否确认 classmaster 整理的目标、边界、DoD。
- 方案选择：选择方案 A / B / C。
- 发布批准：teacher 验证完成后，是否允许发布 actor bundle。
- service 操作批准：是否 restart service、触发 pipeline、清理旧配置。
- dev-helper 清理 gate：是否归档或删除旧 dev-helper 配置。

不适合放在 action 中：

- `actor_request`、`lesson_plan`、`progress_snapshot` 等长期结构化状态。
- validation / release / runtime receipt 的完整内容。
- runtime cursor、service pid/status、work item 目录。

这些内容应分别进入 workspace 或 artifact，action 只引用它们。

### 2.6 少造概念，优先复用 Joi 现有模型

本迁移不新增 server KV、不新增业务专用 protocol，也不把旧 dev-helper 的命名搬进 Joi。遇到旧能力时按以下优先级归位：

1. 能表示为协作事实的，写成 `event`。
2. 能表示为可变工作现场的，写入 scope `workspace`。
3. 需要稳定引用或验收证据的，发布为 `artifact`。
4. 需要确认/选择的，优先普通 event 文本确认；需要机器可读审批时再用 `action`。
5. 长驻外部触发、轮询、定时任务，归入 `service runtime`。

repo cache、repo notes、delivery provision 都是 workspace / service / external integration 的组合，不作为新的 Joi 顶层概念。

## 3. 总体架构

```text
Human / GUI / service
      │
      │ event/append(content.add + hands_off_to)
      ▼
┌─────────────────────┐
│ joi-server          │
│ - event journal     │
│ - artifact store    │
│ - delivery/fanout   │
│ - scope skills      │
└──────────┬──────────┘
           │ event / actor inbox
           ▼
┌──────────────────────────────────────────────┐
│ joi agent serve                              │
│ - interactive_command                        │
│ - prompt templates                           │
│ - scope workspace env                        │
│ - provider session per actor/scope           │
└──────────┬───────────────────────────────────┘
           │ read/write files
           ▼
┌──────────────────────────────────────────────┐
│ scope workspace                              │
│ - .joi/state/*.json                          │
│ - .joi/repos/manifest.json                   │
│ - business files managed by skills           │
│ - projected skills                           │
└──────────────────────────────────────────────┘

┌──────────────────────────────────────────────┐
│ joi service serve                            │
│ - service supervisor                         │
│ - pid/log/status/once                        │
│ - poll/schedule/subscribe plugins            │
│ - writes event + artifact, owns host state   │
└──────────────────────────────────────────────┘
```

## 4. 四个核心概念如何替代旧 dev-helper 能力

### 4.1 Event：替代 append / handoff / timeline

`event` 负责所有协作时间线与唤醒语义：

- 用户消息：`content.add`
- agent 回复：`content.add`
- handoff：`content.add` + `hands_off_to -> actor:<id>`
- reply：`replies_to -> event:<id>`
- 证据引用：`attaches_artifact -> artifact:<id>`
- service 扫描结果：service actor append event

需要补齐 / 保留的 Joi 能力：

- `joi event append --in <thread> --text ...`
- `joi event append --channel --in <channel> --text ... --handoff <actor>`
- `joi handoff` 可以保留为高层 sugar，但底层仍是 event。
- `joi agent serve` 继续从 actor inbox / handoff event 唤醒 agent。
- `joi event query --channel <id> [--type ...] [--since ...] [--limit ...]`：跨 scope 只读拉取 event，供 teacher 这类分析者横向比对历史。该 CLI 不引入新 protocol，只是把 event journal（server 全局资源）暴露成只读查询；channel 是命名空间不是 ACL 边界，敏感性由部署用户的 server access 控制。

### 4.1.1 跨 scope 只读访问

teacher 的横向比对、classroom 自我优化、运维分析都需要跨 channel 读已有事实。Joi 协议层 event journal、artifact store、AgentSpec / ServiceSpec、agent bundle 都是 server 全局资源；channel/thread 是命名空间不是访问 ACL。迁移要求把这件事**显式**暴露成只读 CLI，不再让分析者绕路：

- `joi event query --channel <id> | --in <thread> [--type ...] [--since ...]`
- `joi artifact get <artifact_uri>`：与发布 channel 解耦，凭 URI 拉取。
- `joi agent spec list` / `joi agent spec get <actor_id>`：列出 / 获取已注册 AgentSpec（脱敏后可读）。
- `joi service spec list` / `joi service spec get <service_id>`：同上。
- `joi agent bundle get <actor_id> --path <relative>`：从 AgentSpec.bundle 解析路径并按相对路径读文件（默认 `SKILL.md`、`AGENTS.md`、`skills/**`）。

这些 CLI 在 dispatch 时对所有 agent 默认可用；spec / bundle 中包含密钥时由部署在 spec 加载阶段做脱敏（标记 secret 字段不进入 `spec get` 输出）。这不是给 teacher 专属字段，而是 runtime 通用资源访问能力。

### 4.2 Workspace：替代可变业务状态

workspace 负责可变业务状态，不再有 `stg` 命名。Joi runtime 只约定 `.joi/state/` 这类通用元数据位置；业务状态目录由 skill / service plugin 自己定义。

```text
.joi/
  state/
    scope.json
  repos/
    manifest.json
  skills/
    <runtime-projected skill files>
  <skill-defined business files...>
```

需要新增通用 workspace 能力：

```sh
joi workspace path [--channel <channel_id> | --in <thread_id>] [path]
joi workspace read [--channel <channel_id> | --in <thread_id>] <path>
joi workspace write [--channel <channel_id> | --in <thread_id>] <path> [--text ... | --file ...]
joi workspace list [--channel <channel_id> | --in <thread_id>] [path]
```

设计约束：

- 路径必须是相对路径，禁止 `..` 和绝对路径。
- 不要求 server 解析文件内容。
- agent/service 通过环境变量获得当前 workspace：
  - `JOI_SCOPE_WORKSPACE_DIR`
  - `JOI_CHANNEL_WORKSPACE_DIR`
  - `JOI_THREAD_WORKSPACE_DIR`
  - `JOI_SCOPE_SKILLS_DIR`：当前 scope 下所有 agent 的 skill 投影根（`.joi/skills/`）。
  - `JOI_AGENT_SKILLS_DIR`：当前被 dispatch agent 的 skill 投影目录（`.joi/skills/<actor_id>/`）；agent 默认只读这个。
- CLI 在 agent 当前进程中可以不传 scope，默认读写当前 `JOI_SCOPE_WORKSPACE_DIR`。
- 从外部运维执行时可以显式传 `--channel` 或 `--in`。
- skills 投影源是被 dispatch agent 的 `AgentSpec.bundle/skills/**`，runtime 在 dispatch 时把它软链接到 `.joi/skills/<actor_id>/`，多次 dispatch 幂等；不把 skill 内容当作 workspace 长期业务状态。同 scope 内多 agent 各自投到自己的子目录，互不污染。
- thread scope agent 默认可读 parent channel workspace（例如 repo manifest、channel 级能力索引），默认不可写 parent channel workspace；是否允许跨 scope 写由 scope metadata / membership ACL 决定，默认拒绝。
- channel scope agent 默认不能读取任意 child thread workspace；需要 thread 内容时应通过 event/artifact 或显式授权的 workspace read。

### 4.2.1 Workspace mounts（thread 投影 channel 公共目录或仓库 worktree）

很多业务（discovery 常驻 thread 看 channel `shared/repos`、delivery thread 看 clone 好的目标仓库）需要在 thread workspace 内**直接看到**某些来自 channel workspace 或 service host data dir 的内容。runtime 提供通用 mount 声明，避免 skill 自己读 manifest 拼路径：

`<scope_ws>/.joi/state/scope.json` 可包含：

```jsonc
{
  "mounts": [
    {
      "name": "shared-repos",
      "from": "channel://.joi/repos/cache",
      "to": "shared/repos",
      "readonly": true
    },
    {
      "name": "target-repo:joi-apps",
      "from": "service://repo-cache/cache/aone%2Fjoi-apps",
      "to": "repos/joi-apps",
      "readonly": false
    }
  ]
}
```

约束：

- `from` 支持 `channel://...`（parent channel workspace 内相对路径）、`service://<service_id>/...`（service host data dir 内相对路径）、`thread://...`（同 channel 内其它 thread workspace；默认禁止，需 ACL）。
- `to` 必须是当前 scope workspace 内相对路径。
- runtime 在 scope workspace 创建 / 启动 agent 之前完成 mount（实现一般为 symlink 或 git worktree；不强制具体形式）。
- mount 不引入新 protocol；它就是 workspace 创建时的局部声明。delivery / discovery 的 skill 不需要知道仓库实际路径——`repos/joi-apps/` 直接可用。

旧 stg / context-share 的业务 key 迁移目标由语义决定：跨 agent 契约迁到 artifact + event，single-owner 草稿迁到 owner workspace，service cursor 迁到 service host data dir。具体路径是 skill / service plugin 迁移约定，不是 Joi runtime 约定。本文档附录给出 classroom/a1-auto-dev 的建议映射；迁移期间允许旧 stg 与新目标双写，完成校验后再下线旧 stg/context-share/dev-helper 路径。

### 4.3 Artifact：替代 release evidence / report snapshot

artifact 负责不可变证据：

- 跨 agent 输入/输出契约
- `task_goal.json`
- `definition_of_done.json`
- `clone_manifest.json`
- `lesson_plan.md`
- `validation_report.json`
- `release_receipt.json`
- `runtime_receipt.json`
- `teaching_loop_report.json`
- MR / pipeline / service probe 结果快照
- 最终 lesson / assignment 版本

推荐流程：

```sh
# 1. 业务过程持续写 workspace mutable file（路径由 skill 约定）
joi workspace write --in thread_x classroom/validation_report.json --file validation_report.json

# 2. 达到 DoD 后发布 artifact
joi artifact publish --name validation_report.json --file <workspace-file>

# 3. 在 event 里播报并引用 artifact
joi event append --in thread_x --text "validation passed ..." --attach-artifact art_x
```

如果现有 `event append` 尚不支持 `attaches_artifact` 参数，需要补齐；否则 agent 可以先在正文中粘贴 artifact URI，后续再补关系化引用。

### 4.3.1 Agent 间协作契约：event + artifact，不靠路径约定

为了让 classroom、a1-auto-dev 中的旧 agent 可以被替换、增删或重新实现，跨 agent 的输入/输出必须成为 Joi protocol 可观察的事实，而不是某个共享 workspace 路径。

原则：

1. 上游 agent 可以先在自己的 workspace 写草稿；当产物达到可被其它 actor 消费的状态时，必须发布为 artifact，并通过 `content.add` event + `attaches_artifact` relation 通告。
2. 下游 agent 通过 actor inbox 收到 handoff event 后，从 event relations 读取 artifact；不应通过 `stat <scope_ws>/某个约定路径` 判断上游是否完成。
3. workspace 只作为 agent 自己的工作现场和运维复盘视图，不作为跨 agent API。
4. 若需要保留 workspace mirror，也只能作为 artifact 的可读缓存或人类排查入口；artifact URI 才是跨 agent 契约的稳定引用。
5. `preFlight.requireWorkspaceFiles` 只检查 agent 自己启动所需的本地输入，例如自己上一轮留下的草稿、配置或私有缓存；不能用来表达“等待上游 agent 产出”。等待上游产出必须通过 handoff event + artifact。

classroom/a1-auto-dev 的跨 agent artifact 契约清单建议：

| 产出 | Producer | Consumer | Contract |
| --- | --- | --- | --- |
| task goal / DoD / out-of-scope | classmaster / router | teacher / delivery / feedback-fix-orchestrator | artifact + handoff event |
| clone manifest / repo provision plan | discovery / router | delivery | artifact + handoff event |
| lesson plan / assignment plan | teacher / classmaster | human / downstream agent | artifact + status event |
| validation report | teacher / delivery | release approval / classmaster | artifact + optional action |
| release receipt / runtime receipt | delivery / service | classmaster / human | artifact + status event |
| progress snapshot | owner agent | human / supervisor | workspace draft；需要跨 actor 消费时发布 artifact |

这样 discovery、delivery、teacher 等 actor 的实现可以按 AgentSpec drop-in 替换；只要它继续发布同类 artifact 并发出 handoff event，下游不需要知道它内部 workspace 文件名。

### 4.4 Action：替代人工确认 / 审批 gate / 运行控制选择

action 是迁移后的控制面能力，负责把“继续/停止/选择/批准”变成标准 Joi 事件，而不是让 agent 在普通文本里等待人类口头确认。

当前已有协议形态：

```text
action.request
  payload:
    requestType
    title
    description
    choices[]

action.response
  payload:
    optionId
    kind = accepted|declined
  relations:
    responds_to -> event:<action_request_event_id>
```

迁移后建议使用方式：

#### 需求确认 gate

classmaster 先在自己的 workspace 写需求确认草稿，确认草稿达到可消费状态后发布成 artifact：

```text
requirement_confirmation.json -> artifact://...
assignment_plan.json          -> artifact://...
```

然后发 action：

```json
{
  "requestType": "approval.requirements",
  "title": "Confirm requirements and DoD",
  "description": "请确认 action 关联的 requirement_confirmation 与 assignment_plan artifacts，确认后将创建/复用 thread 并 handoff 给 teacher。",
  "choices": [
    { "id": "approve", "label": "Approve and continue" },
    { "id": "revise", "label": "Needs revision" }
  ]
}
```

#### 发布批准 gate

teacher 在 workspace 写入并发布 artifact：

```text
validation_report.json -> artifact://...
release_receipt.json   -> artifact://...
```

然后发 action：

```json
{
  "requestType": "approval.release",
  "title": "Approve actor bundle release",
  "description": "验证和发布证据已作为 artifact 附加。批准后允许更新 live bundle。",
  "choices": [
    { "id": "approve", "label": "Approve release" },
    { "id": "reject", "label": "Reject" }
  ]
}
```

该 `action.request` 应通过 `attaches_artifact` relation 关联 validation / release artifacts。

#### Service 操作 gate

service supervisor 或 service plugin 可以发 action：

```json
{
  "requestType": "service.restart",
  "title": "Restart a1-e2e scanner",
  "description": "scanner health check failed. Approve restart?",
  "choices": [
    { "id": "restart", "label": "Restart service" },
    { "id": "ignore", "label": "Ignore for now" }
  ]
}
```

`action.response` 被 service host 消费后执行对应控制动作，并将执行结果写入 event；必要时发布 artifact 作为运行证据。若结果需要被业务复盘，service plugin 可以按自身约定同步写 workspace。

#### Spec apply gate（教案应用）

teacher 产出 `lesson_plan` artifact，其中包含针对目标 actor 的 `spec_diff` / `bundle_changes` / `regression_plan`。然后发 action：

```json
{
  "requestType": "approval.spec_apply",
  "title": "Apply lesson plan to actor:delivery",
  "description": "教案 artifact 已附加。批准后将更新 AgentSpec / bundle，并 reload 目标 actor。",
  "choices": [
    { "id": "approve", "label": "Approve and apply" },
    { "id": "reject", "label": "Reject" }
  ]
}
```

`action.response = approve` 被 deployment service（或运维）消费后：

1. 按 `spec_diff` 写回目标 AgentSpec / ServiceSpec 文件。
2. 若 `bundle_changes` 指向新 bundle artifact，把 bundle 落到部署位置。
3. 触发 `joi agent reload <actor_id>` / `joi service reload <service_id>`，runtime 重读 spec、按新 session signature 创建新 provider session。
4. 写入 `runtime_receipt` artifact 作为应用证据。

agent / service 自我演化（包括 classroom 演化自身）走同一条路径，不引入额外机制。

设计约束：

- action payload 只放决策摘要和 choices。
- action 的上下文必须可通过 workspace path 或 artifact relation 找回。
- action.response 必须 `responds_to` 原 action.request。
- action 决策结果若影响长期流程，应以 event 播报；需要稳定引用时发布 artifact，需要本地复盘时再同步写 skill 约定的 workspace mirror。
- action 不应该替代 event timeline；批准结果仍应以普通 event 进行状态播报。
- `requestType` 由 skill 自行选择，建议使用 `<domain>.<verb>` 风格；runtime 不解释该字符串。

### 4.5 Repo cache 与 repo notes

旧 `readonly-repos.sh` 不是 dev-helper project 的一部分，而是 channel 级源码缓存能力。迁移后把它归入 service plugin / workspace manifest 的组合，不新增 server 概念，也不新增 runtime 级 repo 概念。

推荐约定：

```text
channel_ws/
  .joi/
    repos/
      manifest.json
```

`manifest.json` 由 channel 初始化或运维显式写入，替代脚本里的硬编码 `channel -> repo_id[]`：

```json
{
  "repos": [
    {
      "repo_id": "aone/joi-apps",
      "clone_url": "git@...",
      "readonly": true,
      "source": "auto_dev_base"
    }
  ]
}
```

同步方式：

- repo cache sync 是 service plugin 能力，不是 workspace 子命令。on-demand 触发走 `joi service once <repo-cache-service>`，定时同步走 scheduler plugin。
- 物理缓存属于 host data dir，例如 `~/.local/share/joi/service-host/services/repo-cache/cache/{repo_id}`；workspace 只保存 manifest 和业务可见索引。
- 若部署环境仍有 `AUTO_DEV_BASE_DIR`，可以继续使用 `git clone --shared` 优化；否则退化为普通 clone/fetch。
- skill 需要源码时，通过 `JOI_CHANNEL_WORKSPACE_DIR/.joi/repos/manifest.json` 找到 repo-cache service 暴露的路径，或由 service 在 workspace 写入业务约定的索引文件。runtime 不新增 `JOI_REPOS_DIR`。

repo notes 的真相源仍是 a1 kbase。workspace 只保存 mirror/cache，不能把 repo notes 当作普通本地状态：

```text
a1 kbase repo notes  <->  channel_ws/.joi/repos/notes/{repo_id}.json
```

需要提供 service plugin 或已有 service wrapper 替代 `joi_rpc.py repo-stg-*`：

```sh
joi service once repo-notes -- pull <repo_id>
joi service once repo-notes -- push <repo_id> --file <path>
joi service once repo-notes -- verify <repo_id> --expected-clone-url ...
```

这些能力是外部系统集成，底层仍可调用 `a1 kbase`，但不依赖 dev-helper/context-share。

### 4.6 Spec 是 agent/service 的可插拔接入单元

AgentSpec / ServiceSpec 是旧 agent、service 接入 Joi 的唯一注册入口，遵循 drop-in / drop-out：

1. 新增一个 AgentSpec 即注册一个 agent actor；新增一个 ServiceSpec 即注册一个 service actor。
2. 删除或禁用 spec 后，重启对应 host 即下线该 actor；runtime 私有状态留在 agent/service host data dir，可按 actor id 清理，不污染 workspace。
3. spec 之间不通过文件路径隐式耦合。跨 actor 只通过 protocol event、artifact URI、action response、channel membership 表达。
4. prompt template 不写死下游 actor id。需要 handoff 给谁时，由 skill 根据 channel membership、event/artifact metadata 或部署配置决定。
5. ServiceSpec 可以声明自己会向哪个 channel/thread publish event，以及可 handoff 的目标 actor id；这些目标必须来自 channel membership。
6. AgentSpec aliases 用于兼容历史 actor 名，例如 `discovery.aliases = ["researcher"]`；alias 只是 inbox dispatch 的解析规则，不是新 protocol 概念。

可插拔接入旧 agent 的最小形态：

```text
AgentSpec
  actor.id / aliases
  transport = interactive_command
  command + args + provider settings
  bundle / skill projection
  prompt template
  completion sentinel
  optional preFlight for local inputs only
```

可插拔接入旧 service 的最小形态：

```text
ServiceSpec
  actor.id
  plugin = command / scheduler / subscribe / poll
  command + args + env
  target channel/thread membership
  retry / dedupe / log policy
  host data dir for state
```

旧 classroom/a1-auto-dev 的 agent/service 可以逐个用 spec 接入：先保留旧 command/script 作为被监督命令，再逐步把 prompt、handoff、artifact、workspace mirror 迁为 Joi-native 配置。这个过程不要求 Joi server 理解 classroom 业务。

### 4.7 Thread lifecycle：常驻 thread、thread bootstrap、thread-bound service

a1-auto-dev 与 classroom 都依赖一组 thread 级机制：常驻调研 thread、按 artifact 自动准备 workspace、per-thread 监听 service。本节把这些声明性能力归到 runtime，但**不引入新 protocol 概念**——它们都建立在已有的 thread / event / artifact / service plugin / scope.json 之上。

#### 4.7.1 Resident threads（常驻 thread）

channel `<channel_ws>/.joi/state/scope.json` 可声明：

```jsonc
{
  "resident_threads": {
    "discovery": "thread_disc_001"
  }
}
```

约束：

- key 是 channel 自定的角色名（`discovery` 在此例中是 a1-auto-dev 的约定，不是 runtime 词典）。
- value 是已创建 thread 的 id；router 这类 channel agent 通过读 scope.json 拿到稳定地址，不需要重读 event 历史去 grep。
- 创建命令：`joi thread create --in <channel> --resident-as <role> [--title ...]`，runtime 创建 thread 后写回 scope.json.resident_threads，并发 `thread.created` event 公告。
- 删除/替换 resident thread：通过 `joi thread close <id>` + 重新 create；scope.json 同步更新。

router skill 启动时只读 `scope.json.resident_threads.discovery` 即可定址 handoff，channel 级"管家长期记忆"由 scope.json + channel event journal 共同承担。

#### 4.7.2 Thread bootstrap（按 artifact 准备 workspace）

部分 thread（典型如 a1-auto-dev 的 delivery thread）创建时需要按声明准备 workspace 内容，例如 clone 目标 + 关联仓库的 worktree。runtime 提供：

```sh
joi thread create --in <channel> \
  --title "deliver work_item_xxx" \
  --bootstrap-artifact <clone_manifest_artifact_uri>
```

执行流程：

1. runtime 创建 thread + thread workspace + 写 `.joi/state/scope.json`。
2. 把 `clone_manifest` artifact（schema 见 `docs/artifact-contracts.md` §3）解析成 §4.2.1 的 mounts 数组，写入 thread `scope.json.mounts`。每条 repo 默认 `from = service://repo-cache/cache/<urlencoded(repo_id)>`、`to = repos/<basename(repo_id)>`，调用方传 `mounts[]` override 时按顺序覆盖。
3. agent serve 在 `ensure_scope` 时按 mount 声明做 symlink / `git worktree add`；底层复用 channel `.joi/repos/cache/<repo_id>` 缓存。runtime 发 `thread.bootstrapped` event，body 复述 mount 摘要供下游 agent 校验仓库就位（无独立 receipt artifact——mounts 本身就是声明，clone_manifest 是证据）。

bootstrap 不需要新的 ServiceSpec：mounts 是 §4.2.1 已有的 workspace 声明，clone_manifest 是 §4.3.1 已有的跨 agent 契约。整条链路只复用 thread / workspace / artifact 三个元语，不新增 protocol 概念。

#### 4.7.3 Thread-bound service 实例（per-thread MR detector）

部分 service 的生命周期等于一个 thread（典型如 a1-auto-dev 完成态 thread 的 MR 监听）。ServiceSpec 增加可选字段：

```jsonc
{
  "lifecycle": "thread-bound",
  "bind": {
    "scope": "thread",
    "auto_stop_on": ["thread.closed", "service.self_complete"]
  }
}
```

启动方式：

```sh
joi service start --spec mr-detector --in <thread> --params '{"mr_url":"..."}'
```

约束：

- 同一 ServiceSpec 允许**多实例并存**，每实例对应一个 thread；instance id = `<service_id>@<thread_id>`。
- 实例 state（cursor / dedupe / pid / log）写在 `~/.local/share/joi/service-host/services/<service_id>/instances/<thread_id>/`；与 channel-level service state 分离。
- thread 关闭、或实例自检完成时（service plugin 通过特定 exit code / 写 `service.self_complete` event 声明），service host 自停实例并清 pid。
- 实例处理事件、handoff、artifact 与普通 service actor 完全相同；它就是一个"短生命周期 service 实例"，不是新概念。

a1-auto-dev 的 mr-detector 是 thread-bound spec；当前的全局 mr-watcher 仍可作为 channel-level 常驻 service 保留（监听 channel 全局 MR notify、做去重/汇总），二者职责不重叠。

## 5. Prompt 模板设计

旧 `run-agent.sh` 最大的职责之一是拼 prompt。迁移后应由 `AgentSpec.prompt` 承载，不再靠 shell 拼字符串。

建议 schema：

```jsonc
{
  "prompt": {
    "activeSkill": "classmaster",
    "everyTurnPrefix": [
      "[joi handoff v1]",
      "scope_kind: {scope.kind}",
      "scope_id: {scope.id}",
      "actor_id: {actor.id}",
      "from_actor: {trigger.actor_id}",
      "trigger_event_id: {trigger.id}",
      "channel_id: {channel.id}",
      "thread_id: {thread.id}",
      "workspace_dir: {workspace.dir}",
      "skills_dir: {scope.skills}",
      "[/joi handoff v1]"
    ],
    "firstTurnPrefix": [
      "[joi bootstrap]",
      "Read your skill from {agent.bundle}/SKILL.md.",
      "Use `joi event append` for user-visible replies and handoff.",
      "Use `joi workspace read/write` for your mutable working state.",
      "Use `joi artifact publish` for cross-agent outputs and final evidence snapshots.",
      "Use `joi action request` for approvals and choices when human confirmation is required.",
      "[/joi bootstrap]"
    ],
    "everyTurnSuffix": [
      "When finished, emit the configured Joi completion sentinel."
    ]
  }
}
```

模板变量来自 runtime，而不是 actor 自己推断：

| 变量 | 来源 |
| --- | --- |
| `{actor.id}` | AgentSpec actor |
| `{scope.kind}` / `{scope.id}` | trigger event scope |
| `{trigger.id}` / `{trigger.actor_id}` | handoff event |
| `{channel.id}` / `{thread.id}` | scope resolver |
| `{workspace.dir}` | scope workspace |
| `{scope.skills}` | scope skills projection |
| `{agent.bundle}` | AgentSpec `bundle` 字段或 agent actor 注册时解析出的 bundle 路径 |

### 5.0 Handoff trigger prompt prefix（callee 自描述）

`/discovery` `/delivery` 这类 slash command 必须出现在"用户消息位"才会被 Claude / Copilot 解析为 skill 触发；写在 callee 自己的 `firstTurnPrefix`（system 注入）里无效。设计上让 callee **在自己的 AgentSpec 里声明**被 handoff 时所需的 trigger prefix，由 runtime 在 inbox dispatch 时**自动**拼接到 trigger event content 前；caller 无需知道这个细节，可插拔性不被破坏。

```jsonc
{
  "handoff": {
    "triggerPromptPrefix": "/delivery\n",
    "applyOn": "every-turn"
  }
}
```

约束：

- `applyOn` 取值 `first-turn` 或 `every-turn`。skill-driven agent 通常需要 `every-turn` 保证 slash command 一直生效。
- runtime 在把 trigger event 转交给 provider 时，把 prefix 拼接到 input 开头；`from_actor`、`trigger_event_id` 等 metadata 仍由 `everyTurnPrefix` 注入。
- prefix 是 callee 自己的实现细节；caller 端 prompt template / handoff 命令**不允许**写死目标 actor 的 trigger 形态。
- 替换 callee 实现（例如换掉 delivery）只改 callee AgentSpec，router 等 caller 不变。

### 5.1 Agent dispatch gates

不要把所有旧 bridge 行为都塞进 prompt。以下行为应由 agent runtime 在 dispatch 前执行，skill 只读取结果：

#### Pre-flight files

preFlight 只检查 agent 自己启动所需的本地输入，例如自己上轮留下的草稿、私有缓存或本地配置：

```jsonc
{
  "preFlight": {
    "requireWorkspaceFiles": [
      "delivery/local_config.json"
    ],
    "onMissing": "fail_turn"
  }
}
```

这替代旧 `require_delivery_handoff_inputs` 中“本地启动前置检查”的通用部分。runtime 只校验 AgentSpec 写出的相对路径是否存在，不理解 `work_item`、`clone_manifest` 等业务含义。来自上游 agent 的输入必须通过 handoff event 关联 artifact，不能通过 preFlight 约定共享 workspace 路径。缺文件时 runtime 追加标准 error/status event，而不是让 provider 进入半初始化状态。

#### Scope roles

read-only 等通用属性可以写在 `.joi/state/scope.json`，或未来映射到 Membership ACL：

```json
{
  "read_only": true
}
```

runtime 可以把该通用属性注入 prompt 和 env；更细的业务 role 名由 skill 自己记录，不进入 runtime 字段词典。第一阶段只做提示和 preflight，后续可以加 workspace write guard。

#### Completion sentinel

completion sentinel 属于 `interactive_command` transport 的 Done contract，不属于 prompt 模板自由文本。prompt 只引用“configured sentinel”，实际字符串由 transport config 决定，并纳入 session signature。

#### Router fast-path

router 首轮 fast-path 是 skill 行为契约，不应成为 runtime 专用逻辑。runtime 只提供：

- 当前 scope / trigger / workspace 变量。
- `joi thread create`、`joi event append --handoff`、`joi workspace write` 等能力。
- 可选 preflight / read-only guard。

router 是否创建 canonical thread、如何命名 thread、如何初始化业务状态，由 router skill 决定；跨 actor 输出必须发布 artifact 并通过 handoff event 通知，不把 workspace 文件路径当作下游 API。

## 6. Workspace metadata 约定

为了替代 `channel-${id}.json`、`scope-projects/*.json` 中与 scope 本身相关的信息，Joi runtime 只需要约定通用 scope metadata 文件位置。agent dispatch/session 等 runtime 私有状态不进入 workspace。

### 6.1 Channel workspace

```text
.joi/state/scope.json
```

```json
{
  "scope_kind": "channel",
  "scope_id": "chan_x",
  "channel_id": "chan_x",
  "channel_title": "a1-auto-dev",
  "read_only": false,
  "updated_at": "..."
}
```

### 6.2 Thread workspace

```text
.joi/state/scope.json
```

```json
{
  "scope_kind": "thread",
  "scope_id": "thread_x",
  "channel_id": "chan_x",
  "thread_id": "thread_x",
  "thread_title": "update/classmaster",
  "read_only": false,
  "updated_at": "..."
}
```

### 6.3 Agent host state

dispatch cursor、provider session id、session signature 等属于 agent runtime 私有状态，应放 agent host data dir，而不是 workspace。例如：

```text
~/.local/share/joi/agent-host/agents/{actor_id}/{scope_kind}-{scope_id}/session.json
~/.local/share/joi/agent-host/agents/{actor_id}/{scope_kind}-{scope_id}/cursor.json
```

dispatch / session 语义：

1. 每个 actor × scope 同时最多一个 active turn；重叠 trigger 进入 runtime 队列。
2. runtime 使用 server delivery / receipt 作为主幂等来源；agent host state 只做重启恢复和 provider session 记录。
3. runtime 维护内部 session signature；signature 变化时丢弃旧 provider session，下一轮按 first turn 创建新 session。
4. signature 字段由 `interactive_command` 实现决定，至少覆盖 command/args/model/provider/settings 文件 hash/bundle 等会影响会话语义的输入。
5. runtime 不通过 regex 从 prompt 文本提取业务 id。业务标识由 skill 自己从 workspace 或 event payload/meta 中读取。

## 7. Service runtime 迁移设计

`interactive_command` 不适合替代长驻 service。a1-auto-dev、mr-watcher、feedback-triage 应迁到 `joi service serve`。

需要复用现有 `service-plugin-system-design.md` 和 `scheduler-plugin.md` 的模型补齐 service supervisor，不新增一套 service mode 枚举：

| 能力 | 说明 |
| --- | --- |
| `start/stop/restart/status/once` | 替代 `servicectl.sh` |
| pid/log/state 目录 | 默认写 service host data dir，例如 `~/.local/share/joi/service-host/services/{service_id}` |
| single-instance guard | 避免重复 watcher / trigger |
| scheduler / command / subscribe plugin | 支持定时触发、轮询外部系统、订阅 Joi event |
| event output | service 只能通过标准 event 对外说话 |
| artifact evidence | service probe / pipeline / scan result 可发布 artifact |

ServiceSpec 继续作为 deployment artifact，字段形态按现有 service plugin 设计扩展。迁移设计只要求它能表达以下真实形态：

| 现有形态 | Joi-native 承载方式 |
| --- | --- |
| a1-e2e-trigger 定时触发 | scheduler plugin 调度 command service，并把结果写 event/artifact |
| a1-e2e-scanner 周期扫描 | scheduler plugin 或 poll plugin 驱动 command service |
| mr-watcher 轮询 MR 状态并发现 scope | poll plugin / scheduler job + scope resolver；去重状态在 service host data dir |
| feedback-scanner 订阅/轮询反馈 | subscribe plugin 或 once command service；失败重试由 service runtime 管 |
| triage-writer 一次性写入/重试 | command service `once` + retry/log rotation |
| repo-cache sync | service plugin，缓存放 host data dir，manifest/索引可写 workspace |
| repo-notes sync/verify | service plugin 或 command service，外部系统仍是 a1 kbase |
| thread bootstrap（按 clone_manifest 准备 thread workspace） | `joi thread create --bootstrap-artifact` 写 thread `scope.json.mounts`，agent serve 在 ensure_scope 时按 mount 投影（§4.2.1 / §4.7.2）；不再单独的 ServiceSpec |
| mr-detector（新增） | thread-bound service 实例，监听单个 task 的 MR 状态，自检完成或 thread 关闭时自停 |

ServiceSpec 是 deployment artifact，不是完全可移植定义。远端这种单部署场景可以在 spec 中写死 channel/thread id；如果未来需要多部署，再在 config 层做间接寻址，不在本次迁移里新增抽象。

ServiceSpec 注册的是 service actor，AgentSpec 注册的是 agent actor；二者都按 protocol §7.7 的 actor/membership 处理。service 要 publish event 或 handoff 给 agent，必须先以 service actor 身份加入对应 channel membership；handoff target 也必须是该 channel membership 内可解析的 actor id / alias。

ServiceSpec 默认是 channel-level 单实例；声明 `lifecycle: thread-bound` 后允许多实例并存，每实例绑一个 thread，state 落在 `host/services/<service_id>/instances/<thread_id>/`，参见 §4.7.3。

service 需要 LLM 时，不应自己 spawn `interactive_command`。正确模式是 service 以自己的 service actor 身份写 `event/append --handoff <agent>`，由 agent runtime 唤醒对应 agent。`feedback-fix-orchestrator` 这类纯 LLM actor 应归类为 agent；scanner/watcher 这类外部触发器归类为 service。

service idempotency 复用 scheduler-plugin 的 RespondsTo 反向投递和 dedupe 设计，不在迁移方案里另造一套状态协议。once/subscribe/poll 触发都必须记录 trigger event / external key 的 dedupe cursor，避免重启后重复写 handoff 或重复触发 pipeline。

a1-e2e-trigger 与 a1-e2e-scanner 可以作为同一个 a1-e2e ServiceSpec 下的两个 scheduler jobs 接入，以共享外部系统配置、cursor 和 dedupe state；如果运维需要分开部署，也可以拆成两个 ServiceSpec，但二者仍复用同一 service plugin 模型。

### 7.1 Spec / bundle hot reload

teacher 应用教案后必须触发被教 actor 重新启动以加载新 spec / bundle：

- `joi agent reload <actor_id>`：agent host 重读 AgentSpec 与 bundle，按 §6.3 的 session signature 重新计算；signature 变化即丢弃旧 provider session、下一轮按 first turn 启动。
- `joi service reload <service_id>`：service host 重读 ServiceSpec，按 spec 变更决定是否 stop+start；thread-bound 实例随 channel-level reload 一起重启，cursor / dedupe state 保留。
- reload 触发由 deployment service 在消费 `approval.spec_apply` 后发起；运维亦可手动触发。reload 完成后写 `runtime_receipt` artifact + status event。
- 这是已有 spec 加载流程的"显式重启入口"，不是新协议。

## 8. Agent 迁移设计

classmaster / teacher / router / discovery / delivery / sensai / bug-triage / feedback-fix-orchestrator 这类 agent 迁到 `interactive_command`。

迁移后的 AgentSpec 负责：

- provider command：Copilot / Claude
- provider args：`--interactive`、`--allow-all`、`--model`、Claude settings
- session mapping：Joi per actor/scope provider session
- completion sentinel：Done contract
- prompt template：handoff + bootstrap + workspace/artifact/event 指令
- bundle / skills：通过 scope skills 和 `{agent.bundle}` 暴露

旧 bridge 的 `dev_helper_project` 不再存在。对应概念是：

```text
dev_helper_project -> scope workspace path
dev_helper_session_id -> provider session id
stg_prefix -> skill-defined workspace business files
bootstrap-channel -> workspace bootstrap
```

旧 `researcher` 这类历史名称不应写死在 runtime 中。若需要兼容旧事件，可以在 AgentSpec 上声明 aliases，例如 `discovery.aliases = ["researcher"]`，由 inbox dispatch 做 actor id/alias 解析。

## 9. 数据迁移设计

数据迁移是 agent/service cutover 的前置条件，不能等到最后 cleanup 时才处理。迁移工具应优先做成 Joi CLI 离线子命令或一次性运维脚本，但输出必须稳定、可 dry-run、可校验。

| 旧位置 | 新位置 | 迁移策略 |
| --- | --- | --- |
| `~/.local/state/joi-agent/<actor>/<scope>.session` | `~/.local/share/joi/agent-host/agents/<actor>/<scope>/session.json` | 保留 provider session id，首次唤醒继续 resume |
| `~/.local/state/joi-agent/<actor>/<scope>.session-meta.json` | `~/.local/share/joi/agent-host/agents/<actor>/<scope>/session.json.signature` | 转换为 runtime session signature |
| `~/.local/state/joi-agent/dispatch-state/<actor>/<scope>.json` | `~/.local/share/joi/agent-host/agents/<actor>/<scope>/cursor.json` | 合并 last processed event / receipt cursor |
| `~/.local/state/joi-agent/channel-<cid>.json` | `<channel_ws>/.joi/state/scope.json` | 一次性迁移 |
| `~/.local/state/joi-agent/scope-projects/<kind>-<id>.json` | `<scope_ws>/.joi/state/scope.json` | 一次性迁移 |
| `context-share kv work_item/*` 中的跨 agent 产物 | artifact + handoff/status event | 发布为不可变 artifact；workspace 只保留 mirror |
| `context-share kv work_item/*` 中的 single-owner 草稿 | owner agent workspace | 双写观察后切换，具体映射见附录 |
| `context-share kv repo_notes/*` | repo-notes service mirror / workspace business index | 从 a1 kbase pull，workspace 只做业务可见 mirror |
| `context-share kv capability_atlas*` | skill-defined workspace business files | 一次性迁移 + 后续 workspace 写 |
| service state / cursor files | `~/.local/share/joi/service-host/services/<service_id>/**` | 双写观察后切换 |
| `mr-watcher/state.json` | service host data dir | 必须迁 `seen_note_keys` / `merged_emitted` / `closed_emitted` |
| 现有 `~/joi-workspaces/{channel,thread}/...` | 原地保留，补 `.joi/` 子树 | 不移动已有 workspace |

迁移工具要求：

1. `dry-run` 输出旧路径、新路径、字段计数、缺失项。
2. `apply` 执行前保存只读备份。
3. `verify` 校验关键字段等价，例如 session id、gap fingerprint、MR emitted flags、clone manifest。
4. cutover 前对活跃 session 做点名校验，例如 classmaster 当前 Copilot session 必须进入新 agent host session 文件。
5. 双写期结束后才允许停止 context-share/dev-helper。

## 10. 迁移步骤

### Phase 1：补齐通用 workspace、event、查询 CLI

1. 新增 `joi workspace path/read/write/list`。
2. 增强 `joi event append`，支持 `--handoff`、`--reply`、`--attach-artifact`。
3. `joi artifact publish` / `joi artifact get` 与 event relation 端到端打通。
4. 新增 `joi event query --channel <id> | --in <thread>` 跨 scope 只读查询。
5. 新增 `joi agent spec list/get`、`joi service spec list/get`、`joi agent bundle get` 把 spec / bundle 暴露成只读 CLI（脱敏后）。
6. `joi agent serve` 注入 `JOI_SCOPE_WORKSPACE_DIR`、`JOI_CHANNEL_WORKSPACE_DIR`、`JOI_THREAD_WORKSPACE_DIR`、`JOI_SCOPE_SKILLS_DIR`、`JOI_AGENT_SKILLS_DIR`。
7. `joi agent serve` 初始化 `.joi/state/scope.json`，按 AgentSpec.bundle 投影 `.joi/skills/<actor_id>/`。
8. workspace mounts：`scope.json.mounts` 声明 + 启动前完成投影（symlink / worktree）。
9. action 创建能力作为可选增强实现，不作为 Phase 1 阻塞项。

### Phase 2：补齐 prompt template 与 thread/spec lifecycle

1. AgentSpec 增加 `prompt` 配置（`firstTurnPrefix` / `everyTurnPrefix` / `everyTurnSuffix`、模板变量、preFlight、session signature、completion sentinel）。
2. AgentSpec 增加 `handoff.triggerPromptPrefix` + `applyOn`，runtime 在 inbox dispatch 时强制注入到 trigger event content 前。
3. 新增 `joi thread create --in <channel> [--resident-as <role>] [--bootstrap-artifact <uri>]`，写回 channel `scope.json.resident_threads`，调用 bootstrap hook（一个 ServiceSpec）准备 thread workspace。
4. 新增 `joi agent reload <actor_id>` / `joi service reload <service_id>`：spec/bundle 热重启入口。
5. classmaster / teacher prompt 从 shell bridge 迁入 AgentSpec。

### Phase 2.5：数据迁移工具与 dry-run

1. 迁移现有 provider session、session meta、dispatch cursor。
2. 迁移 channel/scope metadata 到 `.joi/state/scope.json`。
3. 迁移 context-share stg keys：跨 agent 产物发布 artifact，single-owner 草稿写 workspace。
4. 迁移 service cursor / pid / status / mr-watcher state 到 service host data dir。
5. 迁移 repo notes mirror 和 capability atlas 到 service plugin / workspace business files。
6. 每个迁移工具必须支持 dry-run 和校验报告。

### Phase 3：迁移 interactive agents

1. Phase 3.0：上线 repo-cache、repo-notes service，完成 readonly-repos / repo-stg-* 的 dry-run 与切换；channel `.joi/repos/cache` 可被 thread workspace 通过 §4.2.1 mounts 直接 mount（无需单独的 provision ServiceSpec）。
2. Phase 3.1：定义 cross-agent artifact 契约清单（task goal、DoD、clone manifest、lesson plan、validation report、receipt 等）。
3. Phase 3a：classmaster / teacher / sensai / bug-triage 先迁，保留普通 event 文本确认 fallback；teacher 走通"横向比对其它 channel + 读 spec/bundle"路径。
4. Phase 3b：router / discovery / delivery / feedback-fix-orchestrator 迁移；router 写入 channel `resident_threads.discovery`；delivery thread 通过 `--bootstrap-artifact <clone_manifest>` 自动准备仓库 worktree；delivery / discovery 在 AgentSpec 上声明 `handoff.triggerPromptPrefix`。
5. 保留旧 command specs 作为 rollback 文件，但默认不加载。
6. 验证 handoff、resume、workspace 草稿、artifact evidence、教案应用回路（spec_apply action → reload → 回归 thread）。

### Phase 4：补齐 service supervisor 与 thread-bound 实例

1. Phase 4a：补 command service / process supervisor、once/retry、日志轮转，先迁 feedback-triage。
2. Phase 4b：复用 scheduler/poll plugin 做调度和 scope discovery，迁 a1-e2e trigger/scanner 和 mr-watcher（channel-level）。
3. Phase 4c：thread-bound service 实例支持（`lifecycle: thread-bound`、多实例 state、自停），上线 mr-detector ServiceSpec；router/delivery 通过 `joi service start --in <thread>` 拉起 per-task detector。
4. a1-e2e trigger/scanner cutover 前必须 dry-run 比对 `gap_fingerprint`。
5. 支持 `start/stop/restart/status/once/reload` 与 pid/log/status/state/single-instance。

### Phase 5：清理 dev-helper

1. 全仓配置不再引用 `dev-helper`、`dev-helper-bridge`、`dev-helper cli`。
2. 远端进程列表无 dev-helper 相关进程。
3. 旧 runtime-tools 归档或删除。
4. 文档、AgentSpec、ServiceSpec 更新为 Joi 原生路径。

## 11. Todo list

| Todo | 内容 | Done |
| --- | --- | --- |
| 1. Inventory | 完整盘点 dev-helper bridge / helper / service scripts 职责 | 每个旧职责映射到 event / artifact / workspace / agent runtime / service runtime |
| 2. Workspace CLI | 实现 `joi workspace path/read/write/list` | 能安全读写 channel/thread workspace 下相对路径文件 |
| 3. Event CLI | 增强 `joi event append` 与 `joi event query` | 能 append、reply、handoff、attach artifact，且能跨 scope 只读查询 |
| 4. Resource read CLI | `joi artifact get` / `joi agent spec` / `joi service spec` / `joi agent bundle get` | teacher 等分析者可读全局 spec / bundle / artifact |
| 5a. Repo cache service | 用 service plugin + workspace manifest 替代 readonly-repos | router/discovery 可读源码 |
| 5b. Repo notes service | 用 service plugin / command service 替代 repo-stg-* | delivery 可校验 repo notes |
| 5c. Repo provision service | thread bootstrap hook，按 clone_manifest artifact 准备 thread workspace mounts | delivery thread 创建即有目标/关联仓库 worktree |
| 6. Cross-agent artifact contracts | 定义 agent 间输入/输出 artifact 清单 | 下游只读 event/artifact，不依赖共享 workspace 路径 |
| 7. Action CLI（可选增强） | `joi action` 创建 approval / choice request；含 `approval.spec_apply` 流程 | 教案应用走 action gate；其它确认仍可 fallback 文本 |
| 8. Workspace bootstrap | scope workspace 初始化 + skills 投影 + mounts 投影 | `.joi/state/scope.json`、`.joi/skills/<actor_id>/`、声明的 mounts 全部可用 |
| 9. Prompt template + handoff prefix | AgentSpec 支持 prompt 模板与 `handoff.triggerPromptPrefix` | classmaster/teacher 不靠 shell；handoff 到 delivery/discovery 自动带 `/delivery` `/discovery` |
| 10. PreFlight / session signature | preFlight files、session signature、completion sentinel 归位 | bundle/model/provider/skill/settings hash 变化会 fresh session |
| 11. Trigger idempotency | runtime 使用 delivery/receipt + actor×scope 单并发 | 重连/重复唤醒不重复处理 |
| 12. Provider parity | Copilot/Claude args/settings/model 完整配置化 | 远端现有 Copilot/Claude 行为等价复现 |
| 13. Thread lifecycle | `joi thread create --resident-as / --bootstrap-artifact`、resident_threads scope.json | router 可定址 discovery、delivery thread 自动 provision |
| 14. Spec/bundle hot reload | `joi agent reload` / `joi service reload` + spec_apply action 落到 spec 文件 | 教案 → spec 改动 → reload → 回归一条龙 |
| 15. Agent migration | 迁移 classmaster/teacher/sensai/bug-triage/router/discovery/delivery/feedback-fix-orchestrator 等 agent | 默认配置不再使用 `run-agent.sh` |
| 16. Service supervisor | Joi 原生 service 进程管理（含 thread-bound 多实例） | 能 start/stop/restart/status/once/reload，有 host data dir、pid/log/state/retry/log rotation；mr-detector 多实例可用 |
| 17. Service migration | 迁移 a1-e2e/mr-watcher/feedback services；上线 mr-detector | service 不再依赖 `servicectl.sh`；per-task MR 监听走 thread-bound 实例 |
| 18. Artifact evidence | 发布验证/发布/运行证据 artifact | Done 报告可引用 artifact URI |
| 19. Data migration | 迁移已有 session/workspace/service/repo/业务状态 | 当前 classroom/a1-auto-dev 上下文不丢 |
| 20. Cleanup | 删除旧 dev-helper 引用 | grep 配置和进程无有效 dev-helper 依赖 |
| 21. E2E | 本地 + 远端验收 | classroom 教学循环、a1-auto-dev 自动开发链路全跑通 |

## 12. 最终 Done 目标

最终 Done 必须同时满足：

1. **配置无 dev-helper**：所有 AgentSpec / ServiceSpec 不再引用 `dev-helper`、`dev-helper-bridge`、`run-agent.sh`、`servicectl.sh`。
2. **进程无 dev-helper**：远端无 dev-helper CLI / bridge / helper 常驻进程；agent 由 `joi agent serve` 驱动，service 由 `joi service serve` 驱动。
3. **状态无 stg/KV server 依赖**：业务可变状态在 workspace，agent/service runtime 私有状态在各自 host data dir。
4. **事件驱动完整**：handoff、reply、status report 都是标准 Joi event。
5. **跨 agent 可插拔**：agent 间输入/输出通过 event + artifact 交付；替换 discovery、teacher、delivery 等实现时不要求复用共享 workspace 文件路径。
6. **证据可审计**：task goal、DoD、clone manifest、validation、release、runtime receipt 等跨 agent 契约和最终证据发布为 artifact，并由 event 引用。
7. **Action 路径可用但非阻塞**：提供 action.request / action.response 协议路径，并至少有一个 agent skill 演示用法；普通 event 文本确认仍可作为 fallback。
8. **Agent 可恢复**：Copilot/Claude provider session 按 actor/scope resume，model/settings hash 变更能创建新 session。
9. **Repo cache 可用 + thread workspace `repos/` 可访问**：router/discovery 可通过 repo-cache service 读源码；delivery thread 通过 `joi thread create --bootstrap-artifact <clone_manifest>` 写 mounts、agent serve 投影后，在 thread workspace 内的 `repos/<repo_id>` 直接可见（mount 由 symlink / worktree 实现，无独立 provision service）。
10. **Service 可监督**：a1-e2e、mr-watcher、feedback-triage 等 service 有 start/stop/status/once/reload、pid、log、single-instance；ServiceSpec `lifecycle: thread-bound` 可创建多实例，每实例 state 独立。
11. **Workspace 可复盘**：channel/thread workspace 下能看到 skill 业务状态、进度、计划、报告；runtime 私有 cursor/session 不污染 workspace；声明的 mounts（channel 公共仓库 / 仓库 worktree）可在 thread workspace 内直接访问。
12. **远端稳定运行**：7878 `joi-server`、agent host、service host 均用最新 Joi 原生逻辑运行。
13. **回滚路径明确**：迁移前旧配置有归档备份；新配置失败时能恢复旧 agent/service 行为。
14. **classroom 教学循环可跑通**：classmaster 与用户沟通需求 → teacher 自跑 + 跨 channel 比对历史 + 读 spec/bundle → 产 lesson_plan artifact → `approval.spec_apply` action 通过后写 spec / bundle 并 reload 目标 actor → 在临时回归 thread 拉起被教 actor 验证；自我演化（target_actor_id 指向 classroom 自身）走同一路径。
15. **a1-auto-dev 自动开发链路可跑通**：router 通过 channel `resident_threads.discovery` 定址常驻 thread；router/discovery 在常驻 thread 调研，确认 task goal/DoD/clone manifest 后发布 artifact；router 通过 `joi thread create --bootstrap-artifact <clone_manifest>` 派生 delivery thread，thread workspace mounts 自动准备目标/关联仓库 worktree；handoff 给 delivery 时 runtime 自动注入 `/delivery` trigger prefix；delivery 对每个仓库走 `/openspec-propose → apply-change → archive`；任务完成后 `joi service start --spec mr-detector --in <thread>` 拉起 thread-bound 监听，发现新事件 handoff 回 delivery 修复，detector 自检完成自停。

## 13. 风险与取舍

| 风险 | 应对 |
| --- | --- |
| workspace 文件缺少 server ACL | CLI 路径只允许 scope workspace 内相对路径；远端文件权限由部署用户控制 |
| artifact 被误用成 KV | 明确 artifact 只做证据快照；mutable state 留在 workspace |
| action 被误用成状态存储 | action 只做审批/选择；长期上下文必须落 workspace，证据必须落 artifact |
| agent 间通过共享 workspace 路径强耦合 | 跨 agent 契约统一走 event + artifact；workspace 只做草稿和 mirror |
| prompt 模板过度定制 | 模板字段通用化，classroom 只是一个配置实例 |
| service 与 agent 边界混淆 | 长驻轮询/调度必须走 service，推理回复才走 agent |
| 历史状态迁移遗漏 | 迁移脚本先 dry-run 输出映射，再执行；迁移后保留旧目录只读备份 |
| Cutover 时活动 Copilot session 失忆 | 迁移 provider session id + session signature；首次唤醒走 resume |
| readonly repo 缓存丢失 | 新 repo-cache service sync 完成前不删除旧 shared/repos；cutover 前完整同步一次 |
| a1 kbase 写权限丢失 | repo-notes service verify 先验证 a1 CLI/kbase 权限 |
| mr-watcher 历史通知重发 | 必须迁移 seen/emitted flags，并 dry-run 校验 |
| a1-e2e-trigger 重复触发 pipeline | 迁移 gap_fingerprint 并在切换前比对一次 |
| feedback-triage retry 行为变化 | retry 配置等价：max=2、delay=1s、emit retrying status |
| context-share 下线过早 | 所有 stg key 迁移并完成双写校验后再停 |
| 跨 scope 只读 CLI 暴露敏感内容 | spec / bundle 加载阶段做脱敏标记；`spec get` 输出过滤 secret 字段；`event query` 沿用 server 现有访问控制 |
| thread bootstrap mounts 与磁盘占用 | agent serve 投影 mount 时优先 `git worktree add` 复用 channel cache；多 thread 并行时按 service host data dir quota 监控 |
| 教案误改导致 actor 失能 | spec_apply action 写入前自动做 spec 备份；reload 失败时回滚到上一版本并发 status event |
| thread-bound 实例资源占用 | mr-detector 实例必须声明自停条件；service host 提供 instance ttl 兜底 |

## 14. 附录：classroom / a1-auto-dev 迁移示例

下面是旧 dev-helper/context-share 业务 key 的建议映射。它只用于当前 classroom/a1-auto-dev skill 自己的迁移参考，不进入 Joi runtime 协议，也不是 Joi runtime 强制约定。跨 agent 契约优先映射为 artifact；workspace 只保存 owner 草稿或 mirror。

| 旧 key / 旧路径 | 新位置建议 | 说明 |
| --- | --- | --- |
| `work_item/<id>/task_goal` | artifact + optional `<scope_ws>/classroom/work-items/<id>/task_goal.json` mirror | discovery / delivery 跨 agent 输入 |
| `work_item/<id>/definition_of_done` | artifact + optional workspace mirror | DoD 源 |
| `work_item/<id>/clone_manifest` | artifact + optional workspace mirror | delivery provision 输入 |
| `work_item/<id>/execution_plan` | owner agent workspace；需要交接时发布 artifact | 可变执行计划 |
| `work_item/<id>/progress_snapshot` | owner agent workspace；需要交接时发布 artifact | 可变进度快照 |
| `work_item/<id>/lesson_plan` | artifact + optional workspace mirror | classroom 教案 |
| `validation_report` / `release_receipt` / `runtime_receipt` | artifact + status event | 验收与运行证据 |
| `capability_atlas*` | `<channel_ws>/classroom/capability-atlas/**` owner workspace；需要跨 actor 固定版本时发布 artifact | discovery 能力索引 |
| `repo_notes/*` | repo-notes service mirror + `<channel_ws>/classroom/repo-notes/**` | workspace 只做业务可见 mirror |
| `e2e-trigger/last-run` | service host data dir | a1-e2e-trigger cursor |
| `e2e-scanner/last-scan` | service host data dir | a1-e2e-scanner cursor |
| `feedback-scanner/<scope>/last-scan` | service host data dir | feedback scanner cursor |
| `triage-writer/last-write` | service host data dir | triage writer cursor |
| `scope-projects/<kind>-<id>.json` | `<scope_ws>/.joi/state/scope.json` | 通用 scope metadata |
| `dispatch-state/<actor>/<scope>.json` | agent host data dir | actor cursor/provider session |

> Work item 注册表：a1-auto-dev 不需要单独的 `work_item` 索引文件。`joi thread list --in <channel>` + channel event journal（包含 `task.created` / `task.handoff` / `thread.bootstrapped` 等 event）+ 每个 delivery thread 的 `task_goal` / `definition_of_done` / `clone_manifest` artifact，已经构成完整的工作项索引；router 只在 channel workspace 维护一个可选的 `a1-auto-dev/work-items/<id>/index.md` mirror，便于人工浏览，不参与 runtime 寻址。

## 15. 结论

彻底下掉 dev-helper 是可行的，但正确方向不是新增 Joi server KV，也不是把旧 `stg` 名字搬进协议，而是用 Joi 的核心资产组合：

- `event`：驱动协作与 handoff（含跨 scope 只读查询、resident_threads 寻址、handoff trigger prefix）。
- `workspace`：承载可变工作现场（含 skills 投影、mounts 投影、thread bootstrap）。
- `artifact`：沉淀不可变证据（含 lesson_plan、clone_manifest、receipt）。
- `action`：承载确认、审批、选择（含 `approval.spec_apply` 教案应用闸门）。
- `agent runtime` / `service runtime`：spec/bundle 热重载 + thread-bound 多实例。

在这个基础上补齐 prompt template、workspace/event/spec CLI、service supervisor、AgentSpec/ServiceSpec drop-in 接入，以及 cross-agent artifact 契约，就可以完整承载原 dev-helper 支持的 classroom 与 a1-auto-dev 能力，并允许旧 agent/service 逐个替换、独立上线或回滚；classroom 自我演化与 a1-auto-dev 端到端自动开发链路均能在新体系下落地。
