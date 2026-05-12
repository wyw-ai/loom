# Task Workflow

`Task` 是挂在某条 channel 顶层消息上的工作状态维度。它不替代
`Channel` 或 `Thread`：

- `Channel` 仍然是权限边界。
- `Thread` 仍然是某条 channel root event 下的上下文边界。
- `Task` 记录这条 root event 是否是一项可领取、可追踪、可验收的工作。

## 数据关系

```text
Channel
  root Event
    Task
      canonical Thread
      TaskAssignment[]
      Artifact ids
      result summary
```

创建 task 时，server 要求 `sourceEventId` 指向 channel scope 的顶层
event。server 会复用这条 event 已有的 thread；如果还没有 thread，会自动创建
canonical thread。后续进度、review、交付物和最终摘要都应该回到这个
canonical thread。

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
  "status": "claimed"
}
```

行为：

- 校验 caller 能访问 source event 所在 channel。
- 校验 source event 是 channel 顶层 event。
- 确保同一 source event 只有一个 task。
- 自动复用或创建 canonical thread。

### `task/list`

按 channel、owner、source event、status 过滤。返回 caller 有权限看到的 task。

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

### `task/assignment.update`

记录 assignment 状态、结果 event 和结果摘要。review agent 完成评审后应调用：

```bash
joi --json task assignment update <assignment_id> \
  --status completed \
  --result-event <review_event_id> \
  --result "评审摘要"
```

当 assignment 进入 `completed` / `failed` / `canceled` 这类终态时，
server 会在 canonical thread 里追加一条 `hands_off_to` assignment
`fromActorId` 的回流 event。这个回流只负责把控制权交还给派发该 assignment
的人，不会自动把 task 标成 `done`。派发人、owner actor 或 requester 需要根据
评审结果决定继续修改、再分派、询问用户，或调用 `task/update --status done`
完成验收。

## CLI

```bash
joi --json task create --source-event <event_id> --title "整理文档" --owner <actor_id>
joi --json task list --source-event <event_id>
joi --json task show <task_id>
joi --json message send --target '#<channel_id>:<root_event_id>' --text "任务进度..."
joi --json handoff <actor_id> --target '#<channel_id>:<root_event_id>' --message "请接手..."
joi --json task update <task_id> --status in_progress
joi --json task assign <task_id> --to <actor_id> --type review --instruction "请评审"
joi --json task assignment update <assignment_id> --status completed --result "评审完成"
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
