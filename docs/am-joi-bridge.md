# am Bot to Joi Thread Bridge

这是一条把 `am listen` 收到的钉钉 bot 消息接入 Joi 的最小链路。默认行为是：
同一个钉钉 `conversationId` 复用同一个 Joi thread，不同聊天对象自动创建不同
thread，然后在 thread 里显式 handoff 给目标 agent 回答。

## 1. 准备 am

```bash
curl -fsSL http://11.158.213.187:9090/install.sh | bash
am bind --type=bot --access-key-id=*** --access-key-secret=*** --account-id=368136
```

## 2. 准备 Joi actor 和 channel 成员

先把 bridge 注册成 service actor：

```bash
joi actor upsert svc_am_bridge --kind service --display "DingTalk QA Bridge"
```

如果目标 channel 是 private，需要由已有 channel 成员邀请 service 和目标 agent：

```bash
joi channel invite <joi_channel_id> svc_am_bridge
joi channel invite <joi_channel_id> <target_agent_actor_id>
```

目标 agent 需要由 `joi agent serve` 在线接收 actor-inbox handoff：

```bash
joi agent serve --specs ~/.config/joi/agents --allow-actors <target_agent_actor_id>
```

## 3. 启动 am listener

```bash
JOI_SERVER=ws://127.0.0.1:7878/rpc \
JOI_CHANNEL_ID=<joi_channel_id> \
JOI_TARGET_AGENT=<target_agent_actor_id> \
JOI_SERVICE_ACTOR=svc_am_bridge \
JOI_SERVICE_DISPLAY="DingTalk QA Bridge" \
am listen --topic /v1.0/im/bot/messages/get \
  --script ./examples/am-joi-channel-bridge.py
```

收到 bot 消息后，脚本会执行等价于：

```bash
joi --as svc_am_bridge thread create --channel <joi_channel_id> --title "钉钉答疑 · <sender>"
joi --as svc_am_bridge handoff <target_agent_actor_id> \
  --in <thread_id> --message "<question>"
```

thread 映射保存在：

```text
~/.config/aone-message-cli/joi-thread-map.json
```

agent 的回答会写回同一个 Joi thread。现在 `joi agent serve` 发出的回答会带
`responds_to -> 原 handoff event` 关系，外部 bridge 可以精确关联问题和回答。

如果确实想继续写 channel 共区，可以显式设置：

```bash
AM_JOI_SCOPE=channel
```

如果想固定写某一个 thread，可以设置：

```bash
JOI_THREAD_ID=<thread_id>
```

## 4. 可选：回发到钉钉

推荐用异步发送模式，避免 DingTalk Stream callback 等 agent 回复时超时：

```bash
AM_JOI_REPLY=1 \
AM_JOI_REPLY_MODE=async_send \
AM_JOI_REPLY_TIMEOUT_SECONDS=120 \
JOI_CHANNEL_ID=<joi_channel_id> \
JOI_TARGET_AGENT=<target_agent_actor_id> \
am listen --topic /v1.0/im/bot/messages/get \
  --script ./examples/am-joi-channel-bridge.py
```

`async_send` 会先通过 callback 快速返回“收到，正在处理。”，后台继续轮询当前 Joi
thread。找到目标 agent 对当前 handoff event 的 `responds_to` 回复后，脚本会调用
`am group <conversationId> <answer> --at <sender>` 回发；如果群发失败，默认 fallback 到
`am chat <sender> <answer>`。

回发时会做两件兼容处理：

- `senderStaffId` 会从 `088084` 规范化成 `88084` 这类 `am` 可发送的 staffId。
- markdown/列表会规整成单行纯文本，避免 `am` 切到受 IP 白名单限制的 bot 发送路径。

如果确实确认 agent 能在 callback 超时前返回，也可以用同步 callback 模式：

```bash
AM_JOI_REPLY_MODE=callback
```

这时脚本会从 stdout 返回 DingTalk Stream callback response：

```json
{"msgKey":"sampleText","msgParam":"{\"content\":\"...\"}"}
```

也可以强制同步二次发送：

```bash
AM_JOI_REPLY_MODE=send
```

所有 `am` 发送默认使用
`AM_CONFIG_PATH=~/.config/aone-message-cli/config.properties`。`am` 回调字段名如果和脚本预置不一致，先跑：

```bash
am listen example
```

然后按实际字段补充 `examples/am-joi-channel-bridge.py` 里的 key 列表。
