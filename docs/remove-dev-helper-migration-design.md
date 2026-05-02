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
| 可变工作状态、草稿、作业、dispatch 游标 | `workspace` | scope 下的普通文件，由 Joi 管路径与约定 |
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
      work-items/
      dispatch/
      services/
      repos/
    skills/
    assignments/

  thread/{thread_id}/
    .joi/
      state/
      work-items/
      dispatch/
      services/
    skills/
    course/
```

Joi 提供 workspace 管理能力，但文件内容是业务约定，不进入 server protocol 的专用 KV。

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
- dispatch cursor、service pid/status、work item 目录。

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
│ - .joi/work-items/**                         │
│ - .joi/dispatch/**                           │
│ - assignments / course                       │
│ - skills                                     │
└──────────────────────────────────────────────┘

┌──────────────────────────────────────────────┐
│ joi service serve                            │
│ - service supervisor                         │
│ - pid/log/status/once                        │
│ - poll/schedule/subscribe plugins            │
│ - writes event + workspace + artifact        │
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

### 4.2 Workspace：替代 stg / scope-projects / dispatch-state / session stats

workspace 负责可变状态，不再有 `stg` 命名。推荐目录：

```text
.joi/
  state/
    scope.json
    progress.json
    execution_plan.json
  work-items/
    {work_item_id}/
      actor_request.json
      requirement_confirmation.json
      assignment_plan.json
      lesson_plan.md
      distribution_plan.json
      teaching_loop_report.json
      validation_report.json
      release_receipt.json
      runtime_receipt.json
  dispatch/
    {actor_id}.json
  services/
    {service_id}/
      cursor.json
      dedupe.json
      status.json
      pid.json
  repos/
    manifest.json
    cache/
      {repo_id}/
    notes/
      {repo_id}.json
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

旧 stg / state 迁移到 workspace 的路径约定：

| 旧 key / 旧路径 | 新 workspace 路径 | 说明 |
| --- | --- | --- |
| `work_item/<id>/task_goal` | `.joi/work-items/<id>/task_goal.json` | discovery / delivery 共享 |
| `work_item/<id>/definition_of_done` | `.joi/work-items/<id>/definition_of_done.json` | DoD 源 |
| `work_item/<id>/clone_manifest` | `.joi/work-items/<id>/clone_manifest.json` | delivery provision 输入 |
| `work_item/<id>/execution_plan` | `.joi/work-items/<id>/execution_plan.json` | 可变执行计划 |
| `work_item/<id>/progress_snapshot` | `.joi/work-items/<id>/progress_snapshot.json` | 可变进度快照 |
| `work_item/<id>/lesson_plan` | `.joi/work-items/<id>/lesson_plan.json` 或 `.md` | classroom 教案 |
| `work_item/<id>/delivery_session` | `.joi/work-items/<id>/delivery_session.json` | delivery provider session 观测 |
| `capability_atlas` / `capability_atlas/*` | `.joi/state/capability_atlas.json` / `.joi/state/capability_atlas/*.json` | discovery 能力索引 |
| `e2e-trigger/last-run` | `.joi/services/a1-e2e-trigger/cursor.json` | service cursor |
| `e2e-scanner/last-scan` | `.joi/services/a1-e2e-scanner/cursor.json` | service cursor |
| `feedback-scanner/<scope>/last-scan` | `.joi/services/feedback-scanner/cursor.json` | service cursor |
| `triage-writer/last-write` | `.joi/services/triage-writer/cursor.json` | service cursor |
| `scope-projects/<kind>-<id>.json` | `.joi/state/scope.json` | scope metadata |
| `dispatch-state/<actor>/<scope>.json` | `.joi/dispatch/<actor>.json` | trigger/session dispatch state |

迁移期间允许旧 stg 和 workspace 双写，完成校验后再下线旧 stg/context-share/dev-helper 路径。

### 4.3 Artifact：替代 release evidence / report snapshot

artifact 负责不可变证据：

- `validation_report.json`
- `release_receipt.json`
- `runtime_receipt.json`
- `teaching_loop_report.json`
- MR / pipeline / service probe 结果快照
- 最终 lesson / assignment 版本

推荐流程：

```sh
# 1. 业务过程持续写 workspace mutable file
joi workspace write --in thread_x .joi/work-items/wi_x/validation_report.json --file validation_report.json

# 2. 达到 DoD 后发布 artifact
joi artifact publish --name validation_report.json --file <workspace-file>

# 3. 在 event 里播报并引用 artifact
joi event append --in thread_x --text "validation passed ..." --attach-artifact art_x
```

如果现有 `event append` 尚不支持 `attaches_artifact` 参数，需要补齐；否则 agent 可以先在正文中粘贴 artifact URI，后续再补关系化引用。

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

classmaster 在 workspace 写入：

```text
.joi/work-items/{work_item_id}/requirement_confirmation.json
.joi/work-items/{work_item_id}/assignment_plan.json
```

然后发 action：

```json
{
  "requestType": "approval.requirements",
  "title": "Confirm requirements and DoD",
  "description": "请确认 workspace 中的 requirement_confirmation 与 assignment_plan，确认后将创建/复用 thread 并 handoff 给 teacher。",
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

`action.response` 被 service host 消费后执行对应控制动作，并将执行结果写入 event + workspace；必要时发布 artifact 作为运行证据。

设计约束：

- action payload 只放决策摘要和 choices。
- action 的上下文必须可通过 workspace path 或 artifact relation 找回。
- action.response 必须 `responds_to` 原 action.request。
- action 决策结果若影响长期流程，应同步写入 workspace，例如 `.joi/work-items/{id}/decisions/{action_event_id}.json`。
- action 不应该替代 event timeline；批准结果仍应以普通 event 进行状态播报。

### 4.5 Workspace repo cache 与 repo notes

旧 `readonly-repos.sh` 不是 dev-helper project 的一部分，而是 channel 级源码缓存能力。迁移后把它归入 workspace 管理，不新增 server 概念。

推荐约定：

```text
channel_ws/
  .joi/
    repos/
      manifest.json
      cache/
        {repo_id}/
      notes/
        {repo_id}.json

thread_ws/
  .joi/
    repos -> ../../channel/{channel_id}/.joi/repos
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

- on-demand：`joi workspace repo-sync --channel <id>`（作为 workspace 子命令，而不是新顶层概念）。
- scheduled：由 `joi service serve` 的 command/poll/schedule service 定期执行同一逻辑。
- 物理缓存默认在 `channel_ws/.joi/repos/cache/{repo_id}`。
- 若部署环境仍有 `AUTO_DEV_BASE_DIR`，可以继续使用 `git clone --shared` 优化；否则退化为普通 clone/fetch。
- agent 通过 `JOI_REPOS_DIR={channel_ws}/.joi/repos/cache` 或模板变量读取只读源码。

repo notes 的真相源仍是 a1 kbase。workspace 只保存 mirror/cache，不能把 repo notes 当作普通本地状态：

```text
a1 kbase repo notes  <->  channel_ws/.joi/repos/notes/{repo_id}.json
```

需要提供 Joi 包装命令替代 `joi_rpc.py repo-stg-*`：

```sh
joi repo-notes pull <repo_id>
joi repo-notes push <repo_id> --file <path>
joi repo-notes verify <repo_id> --expected-clone-url ... --expected-shared-path ...
```

这些命令是外部系统集成，底层仍可调用 `a1 kbase`，但不依赖 dev-helper/context-share。

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
      "work_item_dir: {workspace.dir}/.joi/work-items/{work_item.id}",
      "skills_dir: {scope.skills}",
      "[/joi handoff v1]"
    ],
    "firstTurnPrefix": [
      "[joi bootstrap]",
      "Read your skill from {agent.bundle}/SKILL.md.",
      "Use `joi event append` for user-visible replies and handoff.",
      "Use `joi workspace read/write` for mutable working state.",
      "Use `joi artifact publish` for final evidence snapshots.",
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
| `{agent.bundle}` | resolved bundle |
| `{work_item.id}` | workspace metadata（优先）或 event meta（显式） |

### 5.1 Agent dispatch gates

不要把所有旧 bridge 行为都塞进 prompt。以下行为应由 agent runtime 在 dispatch 前执行，skill 只读取结果：

#### Pre-flight files

delivery 等 agent 在首轮执行前必须确认关键输入文件存在：

```jsonc
{
  "preFlight": {
    "requireWorkspaceFiles": [
      ".joi/work-items/{work_item.id}/task_goal.json",
      ".joi/work-items/{work_item.id}/definition_of_done.json",
      ".joi/work-items/{work_item.id}/out_of_scope.json",
      ".joi/work-items/{work_item.id}/chosen_approach.json",
      ".joi/work-items/{work_item.id}/clone_manifest.json",
      ".joi/work-items/{work_item.id}/task_list.json"
    ],
    "onMissing": "fail_turn"
  }
}
```

这替代旧 `require_delivery_handoff_inputs`。缺文件时 runtime 追加标准 error/status event，而不是让 provider 进入半初始化状态。

#### Scope roles

`resident_discovery` 等 scope role 写在 `.joi/state/scope.json`：

```json
{
  "scope_role": "resident_discovery",
  "read_only": true
}
```

runtime 把该信息注入 prompt 和 env；service/agent 的写操作策略由 skill 与 runtime guard 共同遵守。第一阶段只做提示和 preflight，后续可以加 workspace write guard。

#### Completion sentinel

completion sentinel 属于 `interactive_command` transport 的 Done contract，不属于 prompt 模板自由文本。prompt 只引用“configured sentinel”，实际字符串由 transport config 决定，并纳入 session validity / signature。

#### Router fast-path

router 首轮 fast-path 是 skill 行为契约，不应成为 runtime 专用逻辑。runtime 只提供：

- 当前 scope / trigger / workspace 变量。
- `joi thread create`、`joi event append --handoff`、`joi workspace write` 等能力。
- 可选 preflight / read-only guard。

router 是否创建 canonical thread、如何命名 thread、如何初始化 work item，由 router skill 和 prompt template 决定。

## 6. Workspace metadata 约定

为了替代 `channel-${id}.json`、`scope-projects/*.json`、`dispatch-state/*.json`，Joi runtime 只需要约定文件位置：

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
  "default_work_item_id": "wi-chan-x",
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
  "canonical_work_item_id": "wi-thread-x",
  "updated_at": "..."
}
```

### 6.3 Dispatch state

```text
.joi/dispatch/{actor_id}.json
```

```json
{
  "last_processed_event_id": "evt_x",
  "last_processed_occurred_at": "...",
  "last_turn_kind": "first_turn|subsequent_turn",
  "provider_session_id": "...",
  "validity": {
    "bundle_version": "teacher-skill-v5-lesson-loop",
    "active_skill": "teacher",
    "agent_provider": "copilot",
    "skill_dir": "/path/to/current/skill",
    "workspace_dir": "/path/to/scope/workspace",
    "model": "..."
  }
}
```

`joi agent serve` 可以把 trigger 幂等状态写到这里，或者继续用 runtime 自己的 session store；关键是不要再写 dev-helper state dir。

dispatch / session 算法：

1. 每个 actor × scope 同时最多一个 active turn；重叠 trigger 进入 runtime 队列。
2. runtime 使用 server delivery / receipt 作为主幂等来源；workspace dispatch 文件只作为重启恢复和迁移兼容记录。
3. 如果 `validity` 与当前 runtime 解析结果不同，丢弃旧 `provider_session_id`，下一轮按 first turn 创建新 provider session。
4. validity 至少包含 `bundle_version`、`active_skill`、`agent_provider`、`skill_dir`、`workspace_dir`、`model`。
5. 不再通过 regex 从 prompt 文本提取 `work_item_id`。优先读 `.joi/state/scope.json` 的 `canonical_work_item_id`；必要时由发送方在 event `_meta.workItemId` 显式携带。

## 7. Service runtime 迁移设计

`interactive_command` 不适合替代长驻 service。a1-auto-dev、mr-watcher、feedback-triage 应迁到 `joi service serve`。

需要补齐 service supervisor：

| 能力 | 说明 |
| --- | --- |
| `start/stop/restart/status/once` | 替代 `servicectl.sh` |
| pid/log/state 目录 | 默认写 service workspace `.joi/services/{service_id}` 或 service host data dir |
| single-instance guard | 避免重复 watcher / trigger |
| poll/schedule/subscribe | 支持定时触发、轮询外部系统、订阅 Joi event |
| event output | service 只能通过标准 event 对外说话 |
| artifact evidence | service probe / pipeline / scan result 可发布 artifact |

ServiceSpec 需要覆盖现有真实形态：

| 字段 | 说明 |
| --- | --- |
| `mode` | `schedule` / `poll` / `subscribe` / `once` / `hybrid` |
| `scope` | 固定 channel/thread，或由事件/查询结果发现 scope |
| `command` / `args` | 被监督的脚本或内置 plugin |
| `env` | 显式环境变量，不再隐式依赖 dev-helper |
| `retry` | once/poll 失败重试策略 |
| `log.rotate` | 日志轮转策略 |
| `stateDir` | service workspace 状态目录 |
| `singleInstance` | 同 service id 单实例 |

ServiceSpec 是 deployment artifact，不是完全可移植定义。远端这种单部署场景可以在 spec 中写死 channel/thread id；如果未来需要多部署，再在 config 层做间接寻址，不在本次迁移里新增抽象。

Schedule service 示例：

```jsonc
{
  "id": "a1-e2e-trigger",
  "kind": "command_service",
  "actor": {
    "id": "a1-e2e-trigger",
    "kind": "service",
    "displayName": "A1 E2E Trigger"
  },
  "autostart": true,
  "channelId": "chan_bcf8e1e730bd",
  "config": {
    "mode": "schedule",
    "schedule": "0 10 * * *",
    "scope": { "kind": "thread", "id": "thread_e6378f8cc105" },
    "command": "python3",
    "args": ["{runtime.tools}/a1-e2e/trigger.py"],
    "workspace": "{channel.workspace}",
    "stateDir": "{channel.workspace}/.joi/services/a1-e2e-trigger",
    "logFile": "{channel.workspace}/.joi/services/a1-e2e-trigger/service.log",
    "log": { "rotate": { "sizeMb": 50, "keep": 5 } },
    "singleInstance": true
  }
}
```

Poll service 示例：

```jsonc
{
  "id": "a1-e2e-scanner",
  "kind": "command_service",
  "actor": { "id": "a1-e2e-scanner", "kind": "service", "displayName": "A1 E2E Scanner" },
  "autostart": true,
  "config": {
    "mode": "poll",
    "intervalMs": 1800000,
    "scope": { "kind": "thread", "id": "thread_476a0d7de723" },
    "command": "python3",
    "args": ["{runtime.tools}/a1-e2e/scanner.py"],
    "stateDir": "{channel.workspace}/.joi/services/a1-e2e-scanner",
    "singleInstance": true
  }
}
```

Subscribe / once service 示例：

```jsonc
{
  "id": "feedback-triage",
  "kind": "command_service",
  "actor": { "id": "feedback-scanner", "kind": "service", "displayName": "Feedback Scanner" },
  "autostart": true,
  "config": {
    "mode": "subscribe",
    "onTriggerEvent": {
      "relations": [{ "kind": "hands_off_to", "actor": "feedback-scanner" }]
    },
    "command": "python3",
    "args": ["{runtime.tools}/feedback-triage/feedback_scanner.py", "--once"],
    "retry": { "max": 2, "delayMs": 1000, "emitStatus": true },
    "stateDir": "{scope.workspace}/.joi/services/feedback-scanner",
    "singleInstance": true
  }
}
```

service 需要 LLM 时，不应自己 spawn `interactive_command`。正确模式是 service 写 `event/append --handoff <agent>`，由 agent runtime 唤醒对应 agent。`feedback-fix-orchestrator` 这类纯 LLM actor 应归类为 agent；scanner/watcher 这类外部触发器归类为 service。

## 8. Agent 迁移设计

classmaster / teacher / router / discovery / delivery / sensai / bug-triage 这类 agent 迁到 `interactive_command`。

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
stg_prefix -> workspace work item dir
bootstrap-channel -> workspace bootstrap
```

## 9. 数据迁移设计

数据迁移是 agent/service cutover 的前置条件，不能等到最后 cleanup 时才处理。迁移工具应优先做成 Joi CLI 离线子命令或一次性运维脚本，但输出必须稳定、可 dry-run、可校验。

| 旧位置 | 新位置 | 迁移策略 |
| --- | --- | --- |
| `~/.local/state/joi-agent/<actor>/<scope>.session` | `<scope_ws>/.joi/dispatch/<actor>.json.provider_session_id` | 保留 session id，首次唤醒继续 resume |
| `~/.local/state/joi-agent/<actor>/<scope>.session-meta.json` | `<scope_ws>/.joi/dispatch/<actor>.json.validity` | 转换为 validity 字段 |
| `~/.local/state/joi-agent/dispatch-state/<actor>/<scope>.json` | `<scope_ws>/.joi/dispatch/<actor>.json` | 合并 last processed event |
| `~/.local/state/joi-agent/channel-<cid>.json` | `<channel_ws>/.joi/state/scope.json` | 一次性迁移 |
| `~/.local/state/joi-agent/scope-projects/<kind>-<id>.json` | `<scope_ws>/.joi/state/scope.json` | 一次性迁移 |
| `context-share kv work_item/*` | `<scope_ws>/.joi/work-items/**` | 双写观察后切换 |
| `context-share kv repo_notes/*` | `<channel_ws>/.joi/repos/notes/{repo_id}.json` | 从 a1 kbase pull，workspace 只做 mirror |
| `context-share kv capability_atlas*` | `<channel_ws>/.joi/state/capability_atlas*` | 一次性迁移 + 后续 workspace 写 |
| service state / cursor files | `<scope_ws>/.joi/services/<service_id>/**` | 双写观察后切换 |
| `mr-watcher/state.json` | `<channel_ws>/.joi/services/mr-watcher/state.json` | 必须迁 `seen_note_keys` / `merged_emitted` / `closed_emitted` |
| 现有 `~/joi-workspaces/{channel,thread}/...` | 原地保留，补 `.joi/` 子树 | 不移动已有 workspace |

迁移工具要求：

1. `dry-run` 输出旧路径、新路径、字段计数、缺失项。
2. `apply` 执行前保存只读备份。
3. `verify` 校验关键字段等价，例如 session id、gap fingerprint、MR emitted flags、clone manifest。
4. cutover 前对活跃 session 做点名校验，例如 classmaster 当前 Copilot session 必须进入新 dispatch 文件。
5. 双写期结束后才允许停止 context-share/dev-helper。

## 10. 迁移步骤

### Phase 1：补齐通用 workspace 与 event CLI

1. 新增 `joi workspace path/read/write/list`。
2. 增强 `joi event append`，支持 `--handoff`、`--reply`、`--attach-artifact`。
3. `joi artifact publish` 与 event relation 端到端打通。
4. `joi agent serve` 注入 `JOI_SCOPE_WORKSPACE_DIR`、`JOI_CHANNEL_WORKSPACE_DIR`、`JOI_THREAD_WORKSPACE_DIR`、`JOI_REPOS_DIR`。
5. `joi agent serve` 初始化 `.joi/state/scope.json`。
6. action 创建能力作为可选增强实现，不作为 Phase 1 阻塞项。

### Phase 2：补齐 prompt template

1. AgentSpec 增加 `prompt` 配置。
2. 支持 `firstTurnPrefix` / `everyTurnPrefix` / `everyTurnSuffix`。
3. 支持 trigger、scope、workspace、bundle、skills、work item 等模板变量。
4. 增加 preFlight workspace files gate、session validity 字段、completion sentinel 配置归属。
5. classmaster / teacher prompt 从 shell bridge 迁入 AgentSpec。

### Phase 2.5：数据迁移工具与 dry-run

1. 迁移现有 provider session、session meta、dispatch cursor。
2. 迁移 channel/scope metadata 到 `.joi/state/scope.json`。
3. 迁移 context-share stg keys 到 workspace 文件。
4. 迁移 service cursor / pid / status / mr-watcher state。
5. 迁移 repo notes mirror 和 capability atlas。
6. 每个迁移工具必须支持 dry-run 和校验报告。

### Phase 3：迁移 interactive agents

1. Phase 3a：classmaster / teacher 先迁，保留普通 event 文本确认 fallback。
2. Phase 3b：router / discovery / delivery 迁移，同时接入 repo cache 和 delivery provision。
3. 保留旧 command specs 作为 rollback 文件，但默认不加载。
4. 验证 handoff、resume、workspace 状态读写、artifact evidence。

### Phase 4：补齐 service supervisor

1. Phase 4a：补 command service / process supervisor、once/retry、日志轮转，先迁 feedback-triage。
2. Phase 4b：补 schedule/poll/scope discovery，迁 a1-e2e trigger/scanner 和 mr-watcher。
3. 支持 `start/stop/restart/status/once`。
4. 支持 pid/log/status/state/single-instance。

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
| 4. Repo cache / repo notes | 用 workspace repo cache 和 a1 repo-notes wrapper 替代 readonly-repos / repo-stg-* | router/discovery 可读源码，delivery 可校验 repo notes |
| 5. Action CLI（可选增强） | 增强 `joi action` 创建 approval / choice request | 至少一个 agent skill 演示 action gate；迁移不依赖它 |
| 6. Workspace bootstrap | `joi agent serve` 初始化 scope workspace | `.joi/state/scope.json`、skills、AGENTS.md、env 全部可用 |
| 7. Prompt template | AgentSpec 支持 prompt 模板 | classmaster/teacher 不靠 shell 拼 handoff/bootstrap |
| 8. PreFlight / validity | preFlight files、session validity、completion sentinel 归位 | bundle/model/provider/skill 变化会 fresh session |
| 9. Trigger idempotency | runtime 使用 delivery/receipt + actor×scope 单并发 | 重连/重复唤醒不重复处理 |
| 10. Provider parity | Copilot/Claude args/settings/model 完整配置化 | 远端现有 Copilot/Claude 行为等价复现 |
| 11. Agent migration | 迁移 classmaster/teacher/router/discovery/delivery 等 agent | 默认配置不再使用 `run-agent.sh` |
| 12. Service supervisor | Joi 原生 service 进程管理 | 能 start/stop/restart/status/once，有 pid/log/state/retry/log rotation |
| 13. Service migration | 迁移 a1-e2e/mr-watcher/feedback services | service 不再依赖 `servicectl.sh` |
| 14. Artifact evidence | 发布验证/发布/运行证据 artifact | Done 报告可引用 artifact URI |
| 15. Data migration | 迁移已有 session/workspace/work item/service/repo 状态 | 当前 classroom/a1-auto-dev 上下文不丢 |
| 16. Cleanup | 删除旧 dev-helper 引用 | grep 配置和进程无有效 dev-helper 依赖 |
| 17. E2E | 本地 + 远端验收 | classroom 与 a1-auto-dev 全链路跑通 |

## 12. 最终 Done 目标

最终 Done 必须同时满足：

1. **配置无 dev-helper**：所有 AgentSpec / ServiceSpec 不再引用 `dev-helper`、`dev-helper-bridge`、`run-agent.sh`、`servicectl.sh`。
2. **进程无 dev-helper**：远端无 dev-helper CLI / bridge / helper 常驻进程；agent 由 `joi agent serve` 驱动，service 由 `joi service serve` 驱动。
3. **状态无 stg/KV server 依赖**：结构化工作状态全部在 scope workspace 下，遵循 `.joi/**` 约定。
4. **事件驱动完整**：handoff、reply、status report 都是标准 Joi event。
5. **证据可审计**：validation、release、runtime receipt 等最终证据发布为 artifact，并由 event 引用。
6. **Action 路径可用但非阻塞**：提供 action.request / action.response 协议路径，并至少有一个 agent skill 演示用法；普通 event 文本确认仍可作为 fallback。
7. **Agent 可恢复**：Copilot/Claude provider session 按 actor/scope resume，model/settings 变更能创建新 session。
8. **Repo cache / provision 可用**：router/discovery 可从 workspace repo cache 读源码；delivery 首轮可根据 `clone_manifest` 自动 clone、初始化 OpenSpec、校验 repo notes。
9. **Service 可监督**：a1-e2e、mr-watcher、feedback-triage 等 service 有 start/stop/status/once、pid、log、single-instance。
10. **Workspace 可复盘**：channel/thread workspace 下能看到 work item、assignment、dispatch、progress、service 状态文件。
11. **远端稳定运行**：7878 `joi-server`、agent host、service host 均用最新 Joi 原生逻辑运行。
12. **回滚路径明确**：迁移前旧配置有归档备份；新配置失败时能恢复旧 agent/service 行为。

## 13. 风险与取舍

| 风险 | 应对 |
| --- | --- |
| workspace 文件缺少 server ACL | CLI 路径只允许 scope workspace 内相对路径；远端文件权限由部署用户控制 |
| artifact 被误用成 KV | 明确 artifact 只做证据快照；mutable state 留在 workspace |
| action 被误用成状态存储 | action 只做审批/选择；长期上下文必须落 workspace，证据必须落 artifact |
| prompt 模板过度定制 | 模板字段通用化，classroom 只是一个配置实例 |
| service 与 agent 边界混淆 | 长驻轮询/调度必须走 service，推理回复才走 agent |
| 历史状态迁移遗漏 | 迁移脚本先 dry-run 输出映射，再执行；迁移后保留旧目录只读备份 |
| Cutover 时活动 Copilot session 失忆 | 迁移 provider session id + validity 字段；首次唤醒走 resume |
| readonly repo 缓存丢失 | 新 repo cache sync 完成前不删除旧 shared/repos；cutover 前完整同步一次 |
| a1 kbase 写权限丢失 | `joi repo-notes verify` 先验证 a1 CLI/kbase 权限 |
| mr-watcher 历史通知重发 | 必须迁移 seen/emitted flags，并 dry-run 校验 |
| a1-e2e-trigger 重复触发 pipeline | 迁移 gap_fingerprint 并在切换前比对一次 |
| feedback-triage retry 行为变化 | retry 配置等价：max=2、delay=1s、emit retrying status |
| context-share 下线过早 | 所有 stg key 迁移并完成双写校验后再停 |

## 14. 结论

彻底下掉 dev-helper 是可行的，但正确方向不是新增 Joi server KV，也不是把旧 `stg` 名字搬进协议，而是用 Joi 的核心资产组合：

- `event`：驱动协作与 handoff。
- `workspace`：承载可变工作现场。
- `artifact`：沉淀不可变证据。
- `action`：承载确认、审批、选择等控制面决策。

在这个基础上补齐 prompt template、workspace CLI、service supervisor、agent/service 配置迁移，就可以完整承载原 dev-helper 支持的 classroom 与 a1-auto-dev 能力。
