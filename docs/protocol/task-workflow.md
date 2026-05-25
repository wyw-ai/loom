# Task Workflow

`Task` 是挂在某条 channel 顶层消息上的工作状态维度。它不替代
`Channel` 或 `Thread`：

- `Channel` 仍然是权限边界。
- `Thread` 仍然是某条 channel root event 下的上下文边界。
- `Task` 记录这条 root event 是否是一项可领取、可追踪、可验收的工作。

多 agent 协作的硬规则见
[agent-coordination-workflow.md](./agent-coordination-workflow.md)：开工前 claim，发送前
rebase。也就是说，工作型顶层消息必须先通过 `task.claim` 获得 owner；往 thread /
channel 发送额外可见消息前，应先读取目标最新消息，并用 `message.send.ifLatestMessageId`
做提交前检查。

## 数据关系

```text
Channel
  root Event
    Task
      canonical Thread
      TaskRef[]
      TaskArtifactLink[]
      TaskFact[]
      TaskProjection[]
      TaskAssignment[]
      WorkspaceLease[]
      TaskChangeDelivery[]
```

创建 task 时，server 要求 `sourceEventId` 指向 channel scope 的顶层
event。server 会复用这条 event 已有的 thread；如果还没有 thread，会自动创建
canonical thread。后续进度、review、交付物和最终摘要都应该回到这个
canonical thread。

compound input 仍然以 channel root event 为入口。router 可以为一条原始 root
event 写入多个 child root event，再分别调用 `task/create`；child task 通过
`parentSourceEventId` / `parentTaskId` 追溯原始请求。Joi core 只保存这些
parent 字段，不理解“拆成几个任务”的业务标准。

thread 的交互目标使用 binding 层 canonical target：

```text
#<channel_id>:<root_event_id>
```

发送消息、附件或 handoff 到这个 target 时，binding/CLI 会按需创建或复用这条
root event 下的 thread。actor 不需要先显式 `thread/create`，也不需要为了“进入
thread”给自己发一条 self-handoff。

是否创建 task/thread 不是 server 或 daemon 的静态规则。channel 里的
`hands_off_to` 只表示“把这条消息投递给某个 actor”。收到消息的 actor 先在原
channel turn 里判断：能直接回复就直接回复；如果是复杂、多轮、产出 artifact
或需要其他 actor 协作的工作，再显式创建或复用 task/thread。

## 状态

Task status:

- `todo`
- `claimed`
- `in_progress`
- `waiting_review`
- `done`
- `failed`
- `canceled`

Assignment status:

- `pending`
- `running`
- `completed`
- `failed`
- `canceled`

Assignment type:

- `generate`
- `review`
- `investigate`
- `fix`
- `verify`
- `other`

## RPC

### `task/create`

输入：

```json
{
  "sourceEventId": "evt_...",
  "title": "整理 slock CLI 文档",
  "description": "可选描述",
  "requesterActorId": "actor_human",
  "ownerActorId": "actor_reviewer",
  "status": "claimed",
  "parentSourceEventId": "evt_parent",
  "parentTaskId": "task_parent",
  "practiceContractEpoch": "opaque-practice@2026-05-19"
}
```

行为：

- 校验 caller 能访问 source event 所在 channel。
- 校验 source event 是 channel 顶层 event。
- 确保同一 source event 只有一个 task。
- 自动复用或创建 canonical thread。
- 保存 opaque parent 和 practice epoch 字段；server 不解释实践层语义。

### `task.claim`

`task.claim` 是 owner CAS。调用方可以按 task id claim，也可以直接按 source message
claim：

```json
{
  "sourceMessageId": "msg_...",
  "actorId": "actor_worker"
}
```

语义：

- source message 尚无 task 时，原子创建 task，owner = actor，status = claimed。
- task 已存在但没有 owner 且非终态时，设置 owner = actor，status = claimed。
- task 已由同一 actor 拥有时，幂等成功。
- task 已由其他 actor 拥有或已终态时，返回 conflict / invalid state，不覆盖 owner。

agent 收到 claim 失败后必须停止同一工作，不能在 channel 或 thread 里输出替代性交付。

### Task identity / refs

TaskRef 是 task 的通用身份索引，解决 continuation、外部回调和多任务交错时
“怎么找到同一个工作”的问题。

RPC：

- `task/ref.attach`
- `task/ref.find`
- `task/ref.list`

最小字段：

```json
{
  "taskId": "task_...",
  "kind": "branch",
  "subtype": "git_branch",
  "value": "feature/x",
  "normalized": "repo#feature/x",
  "confidence": "confirmed",
  "status": "active",
  "fields": {}
}
```

Joi core 只按 `channelId + kind + subtype + normalized` 做权限过滤和非终态
active ref 唯一性。`subtype`、`fields` 和 normalized 规则由实践层约定。
`confirmed | inferred` 与 `active | superseded | retired` 只是 lifecycle
标记，不承载 a1-dev 业务判断。

### Typed artifacts

TaskArtifactLink 替代无类型 artifact id 列表。它描述 artifact 对 task 的
schema、role、lineage 和 target/head binding。

RPC：

- `task/artifact.attach`
- `task/artifact.activate`
- `task/artifact.list`

同一 task 下，`schema + role + binding.target_key + binding.purpose` 同时只能有
一个 active link；新 link 激活时必须显式 supersede 旧 link。Joi core 不解析
artifact payload，只保存 opaque schema、lineage 和 binding。

### Facts

TaskFact 是 task 上的事实流，适合外部观察、human decision、service callback
和验证结果。

RPC：

- `task/fact.append`
- `task/fact.list`

支持 lifecycle：`active | superseded | retracted | conflict`。append 对
`taskId + targetKey + kind + signature + status` 幂等；`replaces` 会把旧 fact
标记为 superseded / retracted / conflict。外部事实字段包括：

- `authorityBinding`
- `sourceCursor`
- `sourceSnapshotId`
- `externalUpdatedAt`
- `observedFields`
- `unobservedFields`
- `unavailableReason`
- `snapshotCompleteness`

Joi core 不合并业务字段，也不会把 partial snapshot 当成“未观察字段为 false”。
projection/replay reducer 必须读取这些 opaque metadata 自行判断。

### Projections

TaskProjection 是 GUI、router 和公共 recap 共享的 read model。

RPC：

- `task/projection.put`
- `task/projection.get`
- `task/projection.list`

projection 有 `watermark` 和 `health`：

- `fresh`
- `stale`
- `missing`
- `invalid`
- `repair_required`

projection 缺失或过期时，调用方只能展示 health 或触发 repair，不能临时扫描
thread 文本拼业务状态。

### `task/list`

按 channel、owner、source event、status 过滤。返回 caller 有权限看到的 task。

### `message.send` rebase guard

`message.send` 支持可选 `ifLatestMessageId`：

```json
{
  "target": "#chan_123:msg_root",
  "body": "只发送基于最新上下文的 delta",
  "ifLatestMessageId": "msg_latest"
}
```

server 只在目标 scope 当前最新 message id 等于 `ifLatestMessageId` 时写入。若不匹配，
返回 conflict。调用方应重新 `message.read`，把待发内容 rebase 到最新上下文后再决定
发送、只发 delta，或跳过。

### `task/update`

更新 owner、status、result summary、artifact id。常见用法：

```bash
joi --json task update <task_id> --status waiting_review
joi --json task update <task_id> --status done --result "已交付并完成评审"
```

### `task/assignment.create`

创建 assignment，并在 task canonical thread 写入一条带 `hands_off_to` 的
handoff event。该 event 同时带：

- `relates_to_task -> task`
- `responds_to -> task.sourceEventId`
- payload `_meta.taskId`
- payload `_meta.assignmentId`

这样下游 agent 收到 handoff 时，既能自然看到任务说明，也能通过 CLI 更新结构化
assignment result。

assignment 可以携带 opaque `contract`：

```json
{
  "target": { "target_key": "opaque", "head": "opaque" },
  "effects": { "authorized": ["repo.push"] },
  "workspace": {
    "resource_key": "opaque",
    "write_mode": "write",
    "lease_id": "lease_..."
  },
  "context": {
    "required_artifacts": ["art_..."],
    "required_facts": ["fact_..."],
    "required_validation_facts": ["fact_..."]
  },
  "required_capabilities": ["workspace.write"],
  "versions": {
    "target_actor_spec_revision": "opaque"
  },
  "idempotency_key": "opaque"
}
```

server guard：

- terminal task 拒绝 `fix` / `review` assignment。
- required artifacts 必须已经作为该 task 的 active artifact link 绑定；legacy
  `task.artifact_ids` 只作为兼容兜底。
- required facts / required validation facts 必须属于同一 task 且处于 active。
- required capabilities 用 actor/service 声明的 opaque capability set 做包含性检查。
- `versions.target_actor_spec_revision` 存在时，创建 assignment 即校验目标
  actor/service 的 revision；缺失或不一致都拒绝。
- 同 idempotency key 的 pending/running assignment 返回 existing。
- 同 idempotency key 的 completed assignment 拒绝重复创建。

### Assignment context / preflight

RPC：

- `task/assignment.context`
- `task/assignment.preflight`

`assignment.context` 返回 task、assignment、refs、artifact links、facts、summary
projection 和 guards。agent turn 必须先读 context，再按 contract 声明的输入执行。

`assignment.preflight` 用于外部副作用前校验：

```bash
joi task assignment preflight <assignment_id> \
  --target-key <opaque> \
  --head <opaque> \
  --effect <opaque>
```

preflight 只回答当前 assignment 是否仍允许执行该 effect，检查 freshness、lease、
runtime revision 和 assignment status；它不理解 effect 的业务含义。`pending` 不会
通过 preflight，agent 必须先把 assignment 标为 `running`，再在外部副作用前调用
preflight。

### Workspace lease

RPC：

- `task/workspace.lease.acquire`
- `task/workspace.lease.release`
- `task/workspace.lease.list`

同一 `resourceKey` 允许多个 active read lease；active write lease 与任何其它
active lease 冲突。lease 只保护共享资源写入，不是 task identity；task identity
仍然来自 TaskRef。

### Task changes

RPC：

- `task/change.list`
- `task/change.ack`

TaskFact append、TaskProjection put、TaskAssignment create/update、
TaskArtifactLink activate/supersede、WorkspaceLease acquire/release 和
task-scoped action.response 都可以产生 durable task change delivery。

每个 recipient 用 cursor 补拉未处理变化。ack 必须带 owner disposition，例如：

- `assignment_created`
- `action_requested`
- `fact_written`
- `artifact_written`
- `projection_repaired`
- `blocked`
- `noop_recorded`
- `escalated`

ack 只表示 owner 已经完成后续处理或明确记录无需处理；单纯收到通知不应 ack。
server 会拒绝空 ack：ack 必须带 reason，或带 result artifact/fact/evidence refs
表示已经产生了可追踪处理结果；同一 delivery 不能重复 ack。

### Action task association

`action.request` payload 可带：

- `taskId`
- `targetKey`
- `decisionKind`
- `blocksAssignmentId`
- `conversionOwnerActorId`
- `expiresAt`

`action.response` 到达后，server 会向 conversion owner / task owner 发送
`changeType=action` 的 task change delivery。Joi core 不把 response 直接转成
decision fact；conversion owner 必须显式写 TaskFact 或 typed artifact。

### `task/assignment.update`

记录 assignment 状态、结果 event 和结果摘要。review agent 完成评审后应调用：

```bash
joi --json task assignment update <assignment_id> \
  --status running

joi --json task assignment update <assignment_id> \
  --status completed \
  --result-event <review_event_id> \
  --result "评审摘要" \
  --result-envelope-json '{"assignment_id":"<assignment_id>","status":"completed","evidence_refs":["fact_or_artifact_id"]}'
```

进入 `completed` 前必须已经是 `running`。`completed` 必须提交 result envelope，
且 envelope 至少引用一个 artifact / fact / evidence；server 会同时检查 assignment
guard 仍然 clean，避免 agent 在 stale context 或 lease 冲突下完成交付。

当 assignment 进入 `completed` / `failed` / `canceled` 这类终态时，
server 会在 canonical thread 里追加一条 `hands_off_to` assignment
`fromActorId` 的回流 event。这个回流只负责把控制权交还给派发该 assignment
的人，不会自动把 task 标成 `done`。派发人、owner actor 或 requester 需要根据
评审结果决定继续修改、再分派、询问用户，或调用 `task/update --status done`
完成验收。

## CLI

```bash
joi --json task create --source-event <event_id> --title "整理文档" --owner <actor_id>
joi --json task claim --source-message <channel_message_id>
joi --json task claim <task_id>
joi --json task list --source-event <event_id>
joi --json task show <task_id>
joi --json message send --target '#<channel_id>:<root_event_id>' --if-latest <latest_message_id> --text "任务进度..."
joi --json handoff <actor_id> --target '#<channel_id>:<root_event_id>' --message "请接手..."
joi --json task update <task_id> --status in_progress
joi --json task assign <task_id> --to <actor_id> --type review --instruction "请评审" --contract-file assignment-contract.json
joi --json task assignment context <assignment_id>
joi --json task assignment preflight <assignment_id> --target-key <opaque> --head <opaque> --effect <opaque>
joi --json task workspace lease acquire <assignment_id> --resource-key <opaque> --mode write
joi --json task ref attach <task_id> --kind branch --subtype git_branch --value feature/x --normalized repo#feature/x --confidence confirmed
joi --json task fact append <task_id> --kind ci.status --signature ci:p1 --payload-json '{"state":"passed"}'
joi --json task projection put <task_id> --type summary --health fresh --payload-json '{"summary":"ready"}'
joi --json task change list --task-id <task_id>
joi --json task change ack <change_id> --disposition noop_recorded --reason "no lifecycle change"
joi --json task assignment update <assignment_id> --status running
joi --json task assignment update <assignment_id> --status completed --result-envelope-json '{"assignment_id":"<assignment_id>","status":"completed","evidence_refs":["fact_or_artifact_id"]}'
joi memory append --actor <actor_id> --scope channel --channel <channel_id> --summary "待审核经验"
joi memory update --actor <actor_id> <memory_id> --status accepted --message <message_id>
```

## Emma/Q Review Flow

1. 人在 channel 发顶层消息，表达一项工作。
2. daemon 只把这条 channel handoff 投递给 Emma，并在原 channel 打开判断 turn。
3. Emma 判断这是复杂任务后，调用 `task/create --source-event "$JOI_TRIGGER_EVENT_ID"`。
   server 原子创建或复用挂在这条 root event 下的 canonical thread。
4. root event 气泡显示关联的 task/thread 链接；Emma 用
   `message send --target "#$AGENTX_CHANNEL_ID:$JOI_TRIGGER_EVENT_ID"` 或
   `attachment upload --target ...` 直接把进度和交付物写入 canonical thread。
   channel turn 只回复一条简短指针。
5. Emma 在 task thread target 里继续记录进度、上传 artifact，并用
   `task/update --artifact-id` 绑定到 task。
6. Emma 用 `task assign --type review --to Q仔` 创建 review assignment。
7. Q仔 收到 canonical thread 里的 handoff，评审后调用
   `task assignment update` 写回结构化结果。
8. server 在同一 canonical thread 里 handoff 回创建该 assignment 的 actor
   （这里通常是 Emma）。Emma 根据评审结果决定修改、再派发、询问请求人，或把
   task 更新为 `done`。
9. 如果 owner 是人，请求人在 UI/CLI 里验收后把 task 更新为 `done`。

这个闭环保证 review 不只停留在另一个 thread 或 agent 私有上下文里，而是回到
task timeline、assignment result 和 canonical thread。
