# am Bot to Joi Thread Bridge

把 `am listen` 收到的钉钉 bot 消息接入 Joi 的最小链路。同一个钉钉
`conversationId` 复用同一个 Joi thread；不同对话自动创建不同 thread；每条
消息显式 handoff 给目标 agent 回答。

> 状态：S2 起 bridge 已迁到 Rust handler。`examples/am-joi-channel-bridge.py`
> 仍保留作为参考实现，不再推荐新部署使用。

## 1. 准备 am

```bash
curl -fsSL http://11.158.213.187:9090/install.sh | bash
am bind --type=bot --access-key-id=*** --access-key-secret=*** --account-id=368136
```

## 2. 准备 ServiceSpec

Bridge 的所有配置进 `~/.config/joi/services/<id>.json`（一个 ServiceSpec
等于一个 bridge 实例，可以同时跑多个 bot）。最小示例：

```json
{
  "id": "am_dingtalk_qa",
  "kind": "am",
  "actor": {
    "id": "svc_am_bridge",
    "kind": "service",
    "displayName": "DingTalk QA Bridge"
  },
  "channelId": "<joi_channel_id>",
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

`config` 全部字段（默认值见 `crates/cli/src/service/am/mod.rs::AmConfig`）：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `amBin` | `"am"` | `am` 二进制路径，按 `PATH` 查找 |
| `amConfigPath` | （由 am 自决） | `am --config-path` 透传 |
| `topic` | `/v1.0/im/bot/messages/get` | 文档用，不直接订阅 |
| `scope` | `"auto_thread"` | `channel` / `thread` / `auto_thread` |
| `threadId` | – | `scope=thread` 必填；`auto_thread` 时设置则强制固定 |
| `reply` | `false` | 是否等 agent 回复 |
| `replyMode` | `"callback"` | `callback` / `send` / `async_send` |
| `replyTimeoutSecs` | `120` | 等 agent 回复超时 |
| `autoInvite` | `false` | 启动时尝试把 service + targetAgent 拉进 channel（best-effort） |
| `pendingText` | `"收到，正在处理。"` | `async_send` 模式立刻 callback 的占位文本 |
| `sendPlainText` | `true` | 回发前 markdown → 单行纯文本 |
| `sendMaxChars` | `1800` | 回发文本最大字符数 |
| `sendAttempts` / `sendRetryDelaySecs` / `sendRetryBackoff` | `4` / `1.0` / `1.8` | `am send` 重试 |
| `sendFallbackChat` | `true` | 群发失败时回退到 `am chat <sender>` |
| `replyStrict` | `false` | true 时 send 失败 propagate 给 callback；false 仅记日志 |
| `dryRun` | `false` | 打印替代 `am` 调用，本地调试用 |

确认 spec 合法：

```bash
joi service validate ~/.config/joi/services/am_dingtalk_qa.json
```

private channel 需要由现有 member 邀请 service + target agent（`autoInvite`
是 best-effort，第一次 spec 下发后可能仍需要手动 invite 一次）：

```bash
joi channel invite <joi_channel_id> svc_am_bridge
joi channel invite <joi_channel_id> <target_agent_actor_id>
```

目标 agent 需要由 `joi daemon` 在线接收 actor-inbox handoff：

```bash
joi daemon --allow-actors <target_agent_actor_id>
```

## 3. 启动 am listener

```bash
am listen --topic /v1.0/im/bot/messages/get \
  --script "joi service am-handler --service-id am_dingtalk_qa"
```

每条 bot 消息触发一次 handler 子进程。Handler 读取 stdin 上的消息 JSON，
解析后等价执行：

```bash
joi --as svc_am_bridge thread create --channel <joi_channel_id> \
  --title "钉钉答疑 · <sender>"
joi --as svc_am_bridge handoff <target_agent_actor_id> \
  --in <thread_id> --message "<question>"
```

Handoff event 携带 `_meta.external = { conversationId, senderStaffId,
messageId }`，agent 回复在 server 侧通过 §9.5 自动反向投递回 service 的
inbox（详见 `docs/service-plugin-system-design.md`）。

## 4. 回发模式

- **`callback`**：handler 同步等 agent 回复，把 DingTalk Stream callback
  JSON 写到 stdout 返回给 `am listen`。简单但受限于 DingTalk callback 超时。
- **`send`**：handler 等 agent 回复后调用 `am group/chat` 直接发。callback
  返回 `{}` 占位。
- **`async_send`**（推荐）：handler 立即 callback 一条 `pendingText`，并
  spawn 一个 detached `joi service am-handler --async-reply <payload>` 子
  进程。子进程后台等 agent 回复后调用 `am group/chat`。日志默认在
  `~/.local/share/joi/service-host/services/<id>/logs/async-reply.log`。

## 5. 状态目录

每个 bridge 实例的私有状态：

```text
~/.local/share/joi/service-host/services/<id>/
  thread-map.json   # auto_thread 的 conversationId → joi threadId
  cursors/          # 当前未使用，留给后续 plugin
  dedupe.jsonl      # 当前未使用，留给后续 plugin
  logs/             # 包括 async-reply 子进程日志
```

## 6. 从 Python 脚本迁移

S2 起推荐路径是上面这个 ServiceSpec + handler。已有部署可以平滑切：

1. 写一份 ServiceSpec，覆盖原本通过 env 设置的内容（JOI_CHANNEL_ID →
   `channelId`、JOI_TARGET_AGENT → `targetAgent`、`AM_JOI_*` env →
   `config.*` 同名字段，命名只是从 `AM_JOI_REPLY_TIMEOUT_SECONDS` 这种
   `SCREAMING_SNAKE` 转成 `replyTimeoutSecs` 这种 camelCase）。
2. 把 `am listen --script ./examples/am-joi-channel-bridge.py` 换成
   `am listen --script "joi service am-handler --service-id <id>"`。
3. 第一次启动时 handler 会检测 `~/.config/aone-message-cli/joi-thread-map.json`，
   一次性 copy 到 `~/.local/share/joi/service-host/services/<id>/thread-map.json`，
   原文件保留作为 rollback。

老脚本的全部 ENV → spec.config 映射：

| 老 ENV | spec / config |
| --- | --- |
| `JOI_CHANNEL_ID` | spec.channelId |
| `JOI_TARGET_AGENT` | spec.targetAgent |
| `JOI_THREAD_ID` | config.threadId |
| `JOI_SERVICE_ACTOR` / `JOI_SERVICE_DISPLAY` | spec.actor.id / spec.actor.displayName |
| `JOI_BIN` | 不需要（handler 是 joi 自己） |
| `AM_BIN` | config.amBin |
| `AM_CONFIG_PATH` | config.amConfigPath |
| `AM_JOI_SCOPE` | config.scope |
| `AM_JOI_REPLY` / `AM_JOI_REPLY_MODE` | config.reply / config.replyMode |
| `AM_JOI_REPLY_TIMEOUT_SECONDS` | config.replyTimeoutSecs |
| `AM_JOI_PENDING_TEXT` | config.pendingText |
| `AM_JOI_SEND_*` | config.send* |
| `AM_JOI_AUTO_INVITE` | config.autoInvite |
| `AM_JOI_REPLY_STRICT` | config.replyStrict |
| `AM_JOI_DRY_RUN` | config.dryRun |

`JOI_SERVER` / `JOI_SERVICE_SPECS` / `JOI_SERVICE_HOST_DATA` 仍用 ENV
（连接级 / 路径级配置）。

## 7. 字段提取

Handler 用与 Python 脚本同一套递归 key 表（详见
`crates/cli/src/service/am/extract.rs`）。`am listen` 输出格式如果与现成
key 不一致，先跑：

```bash
am listen example
```

然后按实际字段补 `extract::TEXT_KEYS` / `CONVERSATION_KEYS` /
`SENDER_KEYS` / `MESSAGE_ID_KEYS`，重编 `joi`。
