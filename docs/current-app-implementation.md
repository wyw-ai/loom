# 当前 App 实现技术总览

本文档记录当前实现现状。当前代码已经收口为 Rust 主线：

- server：`crates/server`
- CLI / chat / daemon：`crates/cli`
- runtime adapter：`crates/agent-runtime`
- GUI：`crates/gui`
- 协议类型：`crates/proto`

旧的 `server/*.js` hosted runtime 说明不再代表当前架构。当前 `loom-server` 不内嵌
agent runtime。

## 1. 当前定位

`loom-apps` 是一个本地多 actor 协作工作台：

- `Channel` 是长期协作空间。
- `Thread` 是 channel 公共区某条 root message 下的任务分支，不支持嵌套。
- `Task` 是挂在 channel 顶层消息上的工作状态维度，自动关联该消息的
  canonical thread。
- 人、agent、service 都是 `Actor`。
- 协作事实通过 `Event` 进入 timeline。
- agent 的一次执行被组织为 `Turn`。
- 文件和产物通过 `Artifact` 独立发布。
- 权限请求、agent 主动提问和批准/拒绝请求都通过
  `action.request` / `action.response` 传输；`loom ask-user-question` 和
  `loom request-approval` 会阻塞等待答案并返回给当前 agent 工具调用。

## 2. 进程组成

### 2.1 loom-server

`loom-server` 是 WebSocket JSON-RPC message hub。它负责：

- actor 连接登记
- scope 订阅
- channel / thread / turn / event / relation 存储
- task / assignment 状态存储
- delivery / delivery ack 记录
- artifact 存储
- event fanout
- turn trace 存储与读取权限

它不做：

- machine agent 配置读取
- adapter 启停
- ACP / command 子进程管理
- workspace 计算
- runtime 日志管理

### 2.2 loom-daemon

`loom-daemon` 是 agent runtime supervisor。它负责：

- 读取 `~/.loom-apps/desktop.toml` 的 machine agent 配置
- 自动探测 PATH 上的 provider CLI
- 在内存里合成 per-actor runtime `AgentSpec`
- 为每个 agent actor 建立 WebSocket 连接
- 订阅相关 scope
- 接收 directed message / directed event
- 打开 turn
- 调用 ACP 或 command adapter
- 把 adapter 输出写回 server
- 处理 action response
- 取消本地 adapter 运行

### 2.3 GUI / chat CLI

GUI 和 chat CLI 是人类交互层。它们负责：

- 创建和浏览 channel / thread
- 追加 message event
- 显式 directed message 给 agent
- 创建、领取和追踪 task
- 渲染 timeline / artifact / delivery ack / trace
- 回应 permission request

## 3. 当前数据位置

| 数据 | 默认位置 |
| --- | --- |
| server journal / SQLite / artifacts | server `--data-dir` |
| CLI config | `~/.loom-apps/cli.toml` |
| machine / agent config | `~/.loom-apps/desktop.toml` |
| actor-private runtime 状态 | `~/.agentx/agents/<actor_id>` |
| channel-scoped workspace | `~/.agentx/channels/<channel_id>/agents/<actor_id>/workspace` |
| channel-scoped runtime logs | `~/.agentx/channels/<channel_id>/agents/<actor_id>/logs` |
| shared channel artifacts | `~/.agentx/channels/<channel_id>/shared/artifacts` |

workspace 已经按 channel 细分。ACP `session/new.cwd` 和 command subprocess cwd 都来自
同一套 `AdapterPrompt.cwd`。

## 4. Agent 生命周期

1. `loom-daemon` 启动后读取 machine 配置、探测 provider CLI，并展开成 actor。
2. 每个 agent worker 用自己的 actor id 连接 server。
3. worker 通过 `actor/upsert` 和 `connection/open` 出现在 actor registry 中。
4. 人类消息通过 `message.send` 的 audience / delivery policy 指向 agent。
5. server 记录 message / delivery 并 fanout。
6. worker 收到 channel 公区里的显式 directed message 后，先在原 channel scope 打开判断
   turn。模型决定直接回复，还是把这条 root message claim 成 task 并转到
   canonical thread。
7. worker 构造 channel-aware prompt 并调用 adapter。
8. worker 把内容、trace、action request、run close 写回 server。

server 只承载协议事实，不持有 runtime handle。

## 5. 取消闭环

人类客户端取消 run 时发送 run cancel message。

server 校验 channel member 后关闭 run，并把 cancel message 投递给 run owner
actor。`loom-daemon` 收到后取消本地 adapter。如果 adapter 已经产生了部分
文本，agent client 会先 flush 为 `message`，再关闭 run。

## 6. Runtime 配置

落盘配置是 desktop machine agent 列表。daemon 会按探测到的 provider CLI 在内存里
合成 per-actor `AgentSpec`。transport 目前支持：

- `command`
- `acp_stdio`
- `interactive_command`

`transport.cwd` 已删除。cwd 是 runtime 根据 scope 计算出来的执行上下文，不属于
落盘配置。

## 7. 当前验证重点

修改这条链路时优先验证：

- `cargo check -p loom-server -p loom-cli -p agent-runtime`
- `cargo test -p loom-server`
- `cargo test -p loom-cli agent_serve`
- `cargo test -p agent-runtime`
- `cargo fmt --check`
- `git diff --check`

如果只改 agent runtime 行为，通常只需要重新编译 / 重启 `loom` 或
`loom-daemon`。只有协议、store、fanout、artifact、server RPC 行为变更时才需要
重新编译 / 重启 `loom-server`。
