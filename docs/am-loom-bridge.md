# am Bot to Loom Message Bridge

把 `am listen` 收到的钉钉 bot 消息接入 Loom。每条外部消息会写成一条
Loom directed message，并按配置选择写入 channel、固定 thread，或按
`conversationId` 自动复用 thread。

> Python bridge 已删除。当前唯一推荐入口是 Rust handler：
> `loom service am-handler --service-id <id>`。

## 1. 准备 am

```bash
curl -fsSL http://11.158.213.187:9090/install.sh | bash
am bind --type=bot --access-key-id=*** --access-key-secret=*** --account-id=368136
```

## 2. 准备 ServiceSpec

Bridge 配置放在 `~/.config/loom/services/<id>.json`：

```json
{
  "id": "am_dingtalk_qa",
  "kind": "am",
  "actor": {
    "id": "svc_am_bridge",
    "kind": "service",
    "displayName": "DingTalk QA Bridge"
  },
  "channelId": "<loom_channel_id>",
  "targetAgent": "<target_agent_actor_id>",
  "config": {
    "scope": "auto_thread",
    "reply": true,
    "replyMode": "async_send",
    "replyTimeoutSecs": 120,
    "autoInvite": true
  }
}
```

`config` 字段由 `crates/cli/src/service/am/mod.rs::AmConfig` 解析：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `amBin` | `"am"` | `am` 二进制路径 |
| `amConfigPath` | 由 `am` 自决 | `am --config-path` 透传 |
| `topic` | `/v1.0/im/bot/messages/get` | listener 侧订阅 topic |
| `scope` | `"auto_thread"` | `channel` / `thread` / `auto_thread` |
| `threadId` | - | `scope=thread` 必填；`auto_thread` 下作为固定 thread 覆盖 |
| `reply` | `false` | 是否等待 agent 回复 |
| `replyMode` | `"callback"` | `callback` / `send` / `async_send` |
| `replyTimeoutSecs` | `120` | 等 agent 回复超时 |
| `autoInvite` | `false` | 尝试把 service + target agent 拉入 channel |
| `pendingText` | `"收到，正在处理。"` | `async_send` 的即时 callback 文案 |
| `sendPlainText` | `true` | 回发前把 markdown 压平为单行文本 |
| `sendMaxChars` | `1800` | 回发文本最大长度 |
| `sendAttempts` / `sendRetryDelaySecs` / `sendRetryBackoff` | `4` / `1.0` / `1.8` | `am send` 重试 |
| `sendFallbackChat` | `true` | 群发失败时回退到 `am chat <sender>` |
| `replyStrict` | `false` | true 时回发失败会让 handler 返回失败 |
| `dryRun` | `false` | 只打印 outbound，不执行 `am` |

校验配置：

```bash
loom service validate ~/.config/loom/services/am_dingtalk_qa.json
```

private channel 需要现有成员邀请 service 与目标 agent，或开启 `autoInvite`：

```bash
loom channel invite <loom_channel_id> svc_am_bridge
loom channel invite <loom_channel_id> <target_agent_actor_id>
```

目标 agent 由 `loom-daemon` 或 `loom agent serve` 承载，必须能收到 actor inbox
里的 directed delivery。

## 3. 启动 listener

```bash
am listen --topic /v1.0/im/bot/messages/get \
  --script "loom service am-handler --service-id am_dingtalk_qa"
```

handler 每次从 stdin 读取一条或多条消息 JSON，然后执行：

1. 解析 DingTalk 文本、`conversationId`、sender 和 message id。
2. 在 `auto_thread` 模式下按 `conversationId` 创建或复用 Loom thread。
3. 用 `message.send` 写 directed message，`audience=[targetAgent]`，
   `intent=request_action`，`deliveryPolicy=wake_agent`。
4. 如果开启 `reply`，通过 service actor inbox 等待 parent message 的回复。

handler 写入的 message metadata 包含 `_meta.external = { conversationId,
senderStaffId, messageId }`，用于外部系统排查和回发。

## 4. 回发模式

- `callback`：同步等待 agent 回复，把 DingTalk Stream callback JSON 写到 stdout。
- `send`：等待 agent 回复后调用 `am group/chat` 直接发送，callback 返回 `{}`。
- `async_send`：立刻 callback 一条 `pendingText`，再启动 detached
  `loom service am-handler --async-reply <payload>` 子进程等待和回发。

异步日志默认在：

```text
~/.local/share/loom/service-host/services/<id>/logs/async-reply.log
```

## 5. 状态目录

```text
~/.local/share/loom/service-host/services/<id>/
  thread-map.json
  cursors/
  dedupe.jsonl
  logs/
```

状态目录只保存 connector 私有运行状态。进入 Loom 的协作事实是 message、
delivery ack、artifact 和必要的内部 service event。

## 6. 字段提取

handler 与 `crates/cli/src/service/am/extract.rs` 共用递归 key 表。若 `am listen`
输出格式变化，先运行：

```bash
am listen example
```

然后按实际字段补 `TEXT_KEYS` / `CONVERSATION_KEYS` / `SENDER_KEYS` /
`MESSAGE_ID_KEYS`，重编 `loom`。
