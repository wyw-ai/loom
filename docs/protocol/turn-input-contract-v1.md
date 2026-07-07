# Agent Turn Input Contract v1

状态：草案 / 实施中

日期：2026-07-07

本文定义 Loom agent 每个 turn 的动态输入形态。它补齐
`docs/protocol/agent-runtime-awareness.md` 中“当前 turn 的动态信息另行设计”的
部分，并替代旧的 `=== Latest Loom message ===` 弱格式。

## 目标

1. 把投递事实、消息正文、历史增量和操作规则拆开，避免一段 user prompt 同时承担多种职责。
2. 让 agent 收到机器可读的 turn header，而不是从自然语言头部猜测 sender、intent、route 和 visibility。
3. 消息正文必须作为不可变事实呈现：不改写 `@mention`，不把正文拼进 Loom header，正文里的
   `=== ... ===` 不能伪造 runtime 段落。
4. resume session 的上下文来自 durable Delivery：当前 turn 只注入自上次 ack 后的新投递或 gap，
   不再每回合固定回放最近 20 条。
5. 频道根消息触发时，必须显式告知 agent：频道主干上下文未自动注入，需要用 CLI 查询。

## 非目标

- 不改变 `AGENTS.md`、官方 Loom skill 和 `loom guide` 作为操作规则权威位置的设计。
- 不把 provider 的 role 模型改成多 actor 原生模型；v1 仍通过 provider 的 user input 承载动态事实。
- 不要求服务端立即新增 cursor API。v1 可先复用 `inbox.list` 的 pending delivery 语义落地。

## Envelope 结构

默认 user input 由三段组成：

````text
=== Loom turn input v1 ===
```json
{ ... header ... }
```

```loom-message id=msg_123
原始消息正文
```

Reply contract: ...
````

`=== Loom turn input v1 ===` 只是人类可读标题；机器契约是紧随其后的 JSON header。
消息正文必须放入独立 fenced block。fence 长度由 runtime 动态选择，必须长于正文中任何连续反引号，
因此正文即使包含 ```` ``` ```` 或 `=== Latest Loom message ===` 也只能作为正文出现。

## Header Schema

Header 是 JSON object，字段使用 camelCase。

```json
{
  "version": "loom.turn-input.v1",
  "turn": {
    "runId": "run_123",
    "firstTurnInScope": false,
    "scope": {"kind": "thread", "id": "th_123"},
    "replyTarget": "#general:msg_root",
    "replyContract": "AGENTS.md#loom-operating-rules"
  },
  "wake": [
    {
      "kind": "message",
      "id": "msg_123",
      "intent": "request_action",
      "deliveryPolicy": "wake_agent",
      "deliveredBecause": "mention",
      "from": {"id": "actor_human_a", "name": "Canfuu", "kind": "human"},
      "at": "2026-07-07T00:20:00Z",
      "scope": {"kind": "thread", "id": "th_123"},
      "target": "#general:msg_root",
      "routeTargets": [
        {"id": "actor_agent_b", "name": "Worker", "kind": "agent"}
      ],
      "visibility": {"private": false, "privateTo": []},
      "mentions": [
        {"id": "actor_agent_b", "name": "Worker", "kind": "actor", "display": "Worker"}
      ],
      "bodyRef": "loom-message:msg_123"
    }
  ],
  "unreadGap": {
    "count": 0,
    "included": 0,
    "hint": null
  },
  "actorNames": {
    "actor_human_a": "Canfuu",
    "actor_agent_b": "Worker"
  }
}
```

### `turn`

- `runId` 是当前 `run.open` 返回的 run id。
- `firstTurnInScope` 表示 provider scope session 是否刚被 Loom seed。首次 turn 可包含有限 bootstrap
  回放；后续 resume turn 不做固定最近 N 条回放。
- `scope` 是本 turn 的执行 scope。它与调度键、active turn 和 provider session 的粒度一致。
- `replyTarget` 是 agent 应默认发送可见回复的位置。
- `replyContract` 是指向 `AGENTS.md` 稳定规则的短指针；完整操作手册不进入每回合动态输入。

### `wake[]`

`wake[]` 按到达顺序排列，当前批次合并的每个触发源各一项。消息触发必须包含：

- `kind: "message"`；
- `id`、`intent`、`deliveryPolicy`；
- `from`、`at`、`scope`、`target`；
- `deliveredBecause`，可取 `dm`、`mention`、`reply`、`broadcast`、`thread_attention`、
  `task_owner`、`scope_message`、`event_directed` 或 `unknown`；
- `routeTargets`，由 audience/private routing 解析而来；
- `visibility.private` 和 `visibility.privateTo`；
- `mentions`，来自消息解析出的 mention facts。正文中的 `@actor_id` 不改写，显示名映射只放在 header；
- `bodyRef`，指向同一 envelope 内的 fenced body block。

事件触发使用 `kind: "event"`，正文层可以使用 `loom-event` fenced block 存放 JSON payload。

### `unreadGap`

`unreadGap` 描述同一 scope 中自上次 ack 以来仍未注入正文层的 pending delivery。

- `count` 是未注入条数。
- `included` 是 runtime 在本 turn 增量上下文中实际注入的条数。
- `hint` 是 agent 可执行的查询提示。存在 gap、达到预算上限、或频道根消息触发时必须提供。

频道根消息触发时，`hint` 必须包含类似：

```text
Channel timeline is not injected for a root channel wake. Use `loom message read --target '#general'`.
```

## Delivery Cursor 语义

v1 的 cursor 基于 durable Delivery/Receipt，不基于“最近 20 条消息”：

1. 当前 `wake[]` 是本 turn 正在处理并将在 turn 结束后 ack 的 delivery。
2. 同 scope 中仍为 `pending` 且未进入 `wake[]` 的 delivery，按预算进入“new deliveries since last ack”
   上下文；超出预算的部分只计入 `unreadGap`。
3. resume session turn 不再固定调用 `message.list limit=20` 作为历史回放。
4. 首次 scope turn、provider session 丢失或签名变更时，可以做一次有限 bootstrap 回放，并必须标注为
   bootstrap，而不是 unread delivery。

## Prompt Parts

默认 prompt assembly 至少应暴露：

- `runtime_context`：本地时间和 delivery cursor 上下文；
- `latest_message`：完整 turn input v1（header + fenced bodies + reply pointer/reminder）；
- `assignment_context`：assignment JSON，如本 turn 需要；
- `turn_input`：`latest_message + assignment_context` 的合成结果；
- `user_message`：legacy provider fallback，内容为 `=== User message ===\n{turn_input}`。

Provider manifest 可以继续按 `promptAssembly` 选择 `{prompt.full}`、`{prompt.user}` 等输出。

## 兼容性

- `WakeSpec.replyReminder` 继续控制 per-turn reply 指针频率，但完整规则仍只在 `AGENTS.md`。
- 旧 provider 不需要理解 JSON schema；它仍会看到自然语言标题和 fenced message body。
- 新字段必须兼容 serde 默认值；旧 AgentSpec 没有 `wake` 时使用 runtime 默认。

## 验收标准

- turn input 包含 `loom.turn-input.v1` JSON header，且 header 至少有 `turn`、`wake[]`、`unreadGap`。
- 消息正文在 fenced `loom-message` block 内，正文中的 `=== Latest Loom message ===` 不会形成 runtime header。
- 正文中的 `@actor_id` 保持原样；显示名和 mention 映射存在于 header。
- resume turn 不再注入固定最近 20 条回放；pending delivery 超预算时声明 gap。
- 频道根消息触发时出现 CLI 查询提示。
- prompt telemetry 记录重复注入率和回放/增量上下文预算效果。
