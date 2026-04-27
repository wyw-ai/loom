# 当前 App 实现技术总览

本文档记录当前实现现状。当前代码已经收口为 Rust 主线：

- server：`crates/server`
- CLI / chat / agent client：`crates/cli`
- runtime adapter：`crates/agent-runtime`
- GUI：`crates/gui`
- 协议类型：`crates/proto`

旧的 `server/*.js` hosted runtime 说明不再代表当前架构。当前 `joi-server` 不内嵌
agent runtime。

## 1. 当前定位

`joi-apps` 是一个本地多 actor 协作工作台：

- `Channel` 是长期协作空间。
- `Thread` 是 channel 内的任务分支。
- 人、agent、service 都是 `Actor`。
- 协作事实通过 `Event` 进入 timeline。
- agent 的一次执行被组织为 `Turn`。
- 文件和产物通过 `Artifact` 独立发布。
- 审批和权限请求通过 `action.request` + `Receipt` 闭环。

## 2. 进程组成

### 2.1 joi-server

`joi-server` 是 WebSocket JSON-RPC message hub。它负责：

- actor 连接登记
- scope 订阅
- channel / thread / turn / event / relation 存储
- delivery / receipt 记录
- artifact 存储
- event fanout
- turn trace 存储与读取权限

它不做：

- agent spec 安装或读取
- adapter 启停
- ACP / command 子进程管理
- workspace 计算
- runtime 日志管理

### 2.2 joi agent serve

`joi agent serve` 是 agent runtime supervisor。它负责：

- 扫描 `~/.config/joi/agents/*.json`
- 为每个 agent actor 建立 WebSocket 连接
- 订阅相关 scope
- 接收 handoff / directed event
- 打开 turn
- 调用 ACP 或 command adapter
- 把 adapter 输出写回 server
- 处理 action response
- 取消本地 adapter 运行

### 2.3 GUI / chat CLI

GUI 和 chat CLI 是人类交互层。它们负责：

- 创建和浏览 channel / thread
- 追加 message event
- 显式 handoff 给 agent
- 渲染 timeline / artifact / receipt / trace
- 回应 permission request

## 3. 当前数据位置

| 数据 | 默认位置 |
| --- | --- |
| server journal / SQLite / artifacts | server `--data-dir` |
| CLI config | `~/.config/joi/config.toml` |
| agent spec | `~/.config/joi/agents/*.json` |
| actor-private runtime 状态 | `~/.agentx/agents/<actor_id>` |
| channel-scoped workspace | `~/.agentx/channels/<channel_id>/agents/<actor_id>/workspace` |
| channel-scoped runtime logs | `~/.agentx/channels/<channel_id>/agents/<actor_id>/logs` |
| shared channel artifacts | `~/.agentx/channels/<channel_id>/shared/artifacts` |

workspace 已经按 channel 细分。ACP `session/new.cwd` 和 command subprocess cwd 都来自
同一套 `AdapterPrompt.cwd`。

## 4. Agent 生命周期

1. `joi agent serve` 启动后读取本地 spec。
2. 每个 agent worker 用自己的 actor id 连接 server。
3. worker 通过 `actor/upsert` 和 `connection/open` 出现在 actor registry 中。
4. 人类消息通过 `HandsOffTo` relation 指向 agent。
5. server 记录 event / delivery 并 fanout。
6. worker 收到 event 后打开 turn。
7. worker 构造 channel-aware prompt 并调用 adapter。
8. worker 把内容、trace、action request、turn close 写回 server。

server 只承载协议事实，不持有 runtime handle。

## 5. 取消闭环

人类客户端取消 turn 时调用 `turn/close(status=cancelled)`。

server 校验 channel member 后写入 `turn.close` event，并把该 event handoff 给 turn
owner actor。`joi agent serve` 收到后取消本地 adapter。如果 adapter 已经产生了部分
文本，agent client 会先 flush 为 `content.add`，再关闭 turn。

## 6. Agent Spec

agent spec 仍使用 `crates/proto::methods::AgentSpec` schema。transport 目前支持：

- `acp_stdio`
- `command`

`transport.cwd` 已删除。cwd 是 runtime 根据 scope 计算出来的执行上下文，不属于
agent spec。

## 7. 当前验证重点

修改这条链路时优先验证：

- `cargo check -p joi-server -p joi-cli -p agent-runtime`
- `cargo test -p joi-server`
- `cargo test -p joi-cli agent_serve`
- `cargo test -p agent-runtime`
- `cargo fmt --check`
- `git diff --check`

如果只改 agent runtime 行为，通常只需要重新编译 / 重启 `joi` 或
`joi agent serve`。只有协议、store、fanout、artifact、server RPC 行为变更时才需要
重新编译 / 重启 `joi-server`。
