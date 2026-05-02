# Remove dev-helper Migration Design

> 目标：彻底移除 dev-helper / dev-helper-bridge 依赖，用 Joi 原生 `event`、`artifact`、`workspace`、`agent runtime`、`service runtime` 承载现有 classroom 与 a1-auto-dev 链路。

## 1. 背景

当前远端 classroom / a1-auto-dev 链路中，Joi 已经承担了 server、event、actor、agent serve 等核心能力，但 agent 与 service 的一部分运行 glue 仍由 dev-helper 相关脚本承载：

- `dev-helper-bridge/run-agent.sh`：为 agent 拼接 handoff/bootstrap prompt，维护 scope/session/workspace 状态，并调用 dev-helper CLI。
- `dev-helper-bridge/joi_rpc.py`：提供 append、handoff、stg read/write、repo notes 等 helper。
- `bootstrap-channel.sh` / `readonly-repos.sh`：维护 channel workspace 与只读 repo 缓存。
- `servicectl.sh`：维护 a1-e2e、mr-watcher、feedback-triage 等长驻 service 的 pid/log/status/once。

这些能力本质上不是 dev-helper 专属能力，而是 Joi runtime 应该原生具备的能力。迁移目标不是把旧脚本名字搬进 Joi，而是把旧职责重新映射到 Joi 已有或应补齐的一等概念。

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
  - `JOI_SCOPE_SKILLS_DIR`
- CLI 在 agent 当前进程中可以不传 scope，默认读写当前 `JOI_SCOPE_WORKSPACE_DIR`。
- 从外部运维执行时可以显式传 `--channel` 或 `--in`。
- `JOI_SCOPE_SKILLS_DIR` 指向 runtime 投影目录；投影源来自 AgentSpec bundle / scope skills，不把 skill 内容当作 workspace 的长期业务状态。
- thread scope agent 默认可读 parent channel workspace（例如 repo manifest、channel 级能力索引），默认不可写 parent channel workspace；是否允许跨 scope 写由 scope metadata / membership ACL 决定，默认拒绝。
- channel scope agent 默认不能读取任意 child thread workspace；需要 thread 内容时应通过 event/artifact 或显式授权的 workspace read。

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

ServiceSpec 是 deployment artifact，不是完全可移植定义。远端这种单部署场景可以在 spec 中写死 channel/thread id；如果未来需要多部署，再在 config 层做间接寻址，不在本次迁移里新增抽象。

ServiceSpec 注册的是 service actor，AgentSpec 注册的是 agent actor；二者都按 protocol §7.7 的 actor/membership 处理。service 要 publish event 或 handoff 给 agent，必须先以 service actor 身份加入对应 channel membership；handoff target 也必须是该 channel membership 内可解析的 actor id / alias。

service 需要 LLM 时，不应自己 spawn `interactive_command`。正确模式是 service 以自己的 service actor 身份写 `event/append --handoff <agent>`，由 agent runtime 唤醒对应 agent。`feedback-fix-orchestrator` 这类纯 LLM actor 应归类为 agent；scanner/watcher 这类外部触发器归类为 service。

service idempotency 复用 scheduler-plugin 的 RespondsTo 反向投递和 dedupe 设计，不在迁移方案里另造一套状态协议。once/subscribe/poll 触发都必须记录 trigger event / external key 的 dedupe cursor，避免重启后重复写 handoff 或重复触发 pipeline。

a1-e2e-trigger 与 a1-e2e-scanner 可以作为同一个 a1-e2e ServiceSpec 下的两个 scheduler jobs 接入，以共享外部系统配置、cursor 和 dedupe state；如果运维需要分开部署，也可以拆成两个 ServiceSpec，但二者仍复用同一 service plugin 模型。

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

### Phase 1：补齐通用 workspace 与 event CLI

1. 新增 `joi workspace path/read/write/list`。
2. 增强 `joi event append`，支持 `--handoff`、`--reply`、`--attach-artifact`。
3. `joi artifact publish` 与 event relation 端到端打通。
4. `joi agent serve` 注入 `JOI_SCOPE_WORKSPACE_DIR`、`JOI_CHANNEL_WORKSPACE_DIR`、`JOI_THREAD_WORKSPACE_DIR`。
5. `joi agent serve` 初始化 `.joi/state/scope.json`。
6. action 创建能力作为可选增强实现，不作为 Phase 1 阻塞项。

### Phase 2：补齐 prompt template

1. AgentSpec 增加 `prompt` 配置。
2. 支持 `firstTurnPrefix` / `everyTurnPrefix` / `everyTurnSuffix`。
3. 支持 trigger、scope、workspace、bundle、skills 等通用模板变量。
4. 增加 preFlight workspace files gate、session signature、completion sentinel 配置归属。
5. classmaster / teacher prompt 从 shell bridge 迁入 AgentSpec。

### Phase 2.5：数据迁移工具与 dry-run

1. 迁移现有 provider session、session meta、dispatch cursor。
2. 迁移 channel/scope metadata 到 `.joi/state/scope.json`。
3. 迁移 context-share stg keys：跨 agent 产物发布 artifact，single-owner 草稿写 workspace。
4. 迁移 service cursor / pid / status / mr-watcher state 到 service host data dir。
5. 迁移 repo notes mirror 和 capability atlas 到 service plugin / workspace business files。
6. 每个迁移工具必须支持 dry-run 和校验报告。

### Phase 3：迁移 interactive agents

1. Phase 3.0：上线 repo-cache service 与 repo-notes service，完成 readonly-repos / repo-stg-* 的 dry-run 与切换。
2. Phase 3.1：定义 cross-agent artifact 契约清单，确认 task goal、DoD、clone manifest、lesson plan、validation report、receipt 等产物都通过 artifact + event 交付。
3. Phase 3a：classmaster / teacher / sensai / bug-triage 先迁，保留普通 event 文本确认 fallback。
4. Phase 3b：router / discovery / delivery / feedback-fix-orchestrator 迁移，同时接入 repo-cache service 和 delivery provision。
5. 保留旧 command specs 作为 rollback 文件，但默认不加载。
6. 验证 handoff、resume、workspace 草稿、artifact evidence。

### Phase 4：补齐 service supervisor

1. Phase 4a：补 command service / process supervisor、once/retry、日志轮转，先迁 feedback-triage。
2. Phase 4b：复用 scheduler/poll plugin 做调度和 scope discovery，迁 a1-e2e trigger/scanner 和 mr-watcher。
3. a1-e2e trigger/scanner cutover 前必须 dry-run 比对 `gap_fingerprint`，确认不会重复触发 pipeline 或漏扫结果。
4. 支持 `start/stop/restart/status/once`。
5. 支持 pid/log/status/state/single-instance。

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
| 3. Event CLI | 增强 `joi event append` | 能 append、reply、handoff，后续能 attach artifact |
| 4a. Repo cache service | 用 service plugin + workspace manifest 替代 readonly-repos | router/discovery 可读源码 |
| 4b. Repo notes service | 用 service plugin / command service 替代 repo-stg-* | delivery 可校验 repo notes |
| 5. Cross-agent artifact contracts | 定义 agent 间输入/输出 artifact 清单 | 下游只读 event/artifact，不依赖共享 workspace 路径 |
| 6. Action CLI（可选增强） | 增强 `joi action` 创建 approval / choice request | 至少一个 agent skill 演示 action gate；迁移不依赖它 |
| 7. Workspace bootstrap | `joi agent serve` 初始化 scope workspace | `.joi/state/scope.json`、skills 投影、AGENTS.md、env 全部可用 |
| 8. Prompt template | AgentSpec 支持 prompt 模板 | classmaster/teacher 不靠 shell 拼 handoff/bootstrap |
| 9. PreFlight / session signature | preFlight files、session signature、completion sentinel 归位 | bundle/model/provider/skill/settings hash 变化会 fresh session |
| 10. Trigger idempotency | runtime 使用 delivery/receipt + actor×scope 单并发 | 重连/重复唤醒不重复处理 |
| 11. Provider parity | Copilot/Claude args/settings/model 完整配置化 | 远端现有 Copilot/Claude 行为等价复现 |
| 12. Agent migration | 迁移 classmaster/teacher/sensai/bug-triage/router/discovery/delivery/feedback-fix-orchestrator 等 agent | 默认配置不再使用 `run-agent.sh` |
| 13. Service supervisor | Joi 原生 service 进程管理 | 能 start/stop/restart/status/once，有 host data dir、pid/log/state/retry/log rotation |
| 14. Service migration | 迁移 a1-e2e/mr-watcher/feedback services | service 不再依赖 `servicectl.sh` |
| 15. Artifact evidence | 发布验证/发布/运行证据 artifact | Done 报告可引用 artifact URI |
| 16. Data migration | 迁移已有 session/workspace/service/repo/业务状态 | 当前 classroom/a1-auto-dev 上下文不丢 |
| 17. Cleanup | 删除旧 dev-helper 引用 | grep 配置和进程无有效 dev-helper 依赖 |
| 18. E2E | 本地 + 远端验收 | classroom 与 a1-auto-dev 全链路跑通 |

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
9. **Repo cache / provision 可用**：router/discovery 可通过 repo-cache service 读源码；delivery 首轮可根据 clone manifest artifact 自动 clone、初始化 OpenSpec、校验 repo notes。
10. **Service 可监督**：a1-e2e、mr-watcher、feedback-triage 等 service 有 start/stop/status/once、pid、log、single-instance。
11. **Workspace 可复盘**：channel/thread workspace 下能看到 skill 业务状态、进度、计划、报告；runtime 私有 cursor/session 不污染 workspace。
12. **远端稳定运行**：7878 `joi-server`、agent host、service host 均用最新 Joi 原生逻辑运行。
13. **回滚路径明确**：迁移前旧配置有归档备份；新配置失败时能恢复旧 agent/service 行为。

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

## 15. 结论

彻底下掉 dev-helper 是可行的，但正确方向不是新增 Joi server KV，也不是把旧 `stg` 名字搬进协议，而是用 Joi 的核心资产组合：

- `event`：驱动协作与 handoff。
- `workspace`：承载可变工作现场。
- `artifact`：沉淀不可变证据。
- `action`：承载确认、审批、选择等控制面决策。

在这个基础上补齐 prompt template、workspace CLI、service supervisor、AgentSpec/ServiceSpec drop-in 接入，以及 cross-agent artifact 契约，就可以完整承载原 dev-helper 支持的 classroom 与 a1-auto-dev 能力，并允许旧 agent/service 逐个替换、独立上线或回滚。
