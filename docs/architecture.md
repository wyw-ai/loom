# 架构说明

## 1. 当前结论

这套东西已经不是“能不能做”的问题了，而是已经落成了一版可联调的 runtime + server + GUI。

核心判断：

1. 协议文档里的领域模型是稳定的，适合做 server-first 的实现。
2. `joi` 里已经有足够成熟的 ACP/runtime 经验，可以直接借思路。
3. 本机已经安装了真实 ACP adapter，所以这里不需要把 mock 当主路径。

## 2. 实现边界

当前 `joi-apps` 负责两部分：

### 2.1 Server

负责：

- 协议对象持久化
- 事件追加与关系建模
- scope 实时流绑定、directed delivery 与 membership 维护
- pending delivery 补发
- artifact 存储
- action request / receipt
- 本地 agent registry
- 本地 ACP runtime 托管

### 2.2 GUI

负责：

- 人类 actor 打开 GUI session
- 浏览 `Space` / `Conversation`
- 观察 timeline
- 对 agent 发任务与显式 handoff
- 把 `@handle` 作为普通文本引用显示，而不是机器路由
- 回应 `action.request`
- 在设置抽屉里安装 / 编辑 / 启停 agent
- 观察 runtime 状态和日志

## 3. 和 joi 的映射关系

| 协议对象 | 这次实现 | joi 对应概念 |
| --- | --- | --- |
| `Actor` | `actors` 表 | user / agent / system sender |
| `Space` | `spaces` 表 | channel |
| `Conversation` | `conversations` 表 | thread |
| `Turn` | `turns` 表 | 一次 agent 执行回合 |
| `Event` | `events` 表 | message + runtime event stream |
| `Relation` | `relations` 表 | reply / target / handoff / artifact link |
| `Artifact` | `artifacts` 表 + 文件目录 | shared artifacts |
| `Membership` | `memberships` 表 | scope 上的持续上下文归属 |
| `Delivery` | `deliveries` 表 | 事件投递状态 |
| `Receipt` | `receipts` 表 | action request 处理状态 |

## 4. Server 设计

### 4.1 存储

- SQLite：`data/protocol.sqlite`
- Artifact 文件：`data/artifacts/<artifact-id>/...`
- Agent 文档：`data/agents/*.json`
- Runtime 日志：`data/runtime-logs/*.log`

### 4.2 接口

HTTP 入口：

- `POST /api/rpc`
- `GET /api/stream?session_id=...`
- `GET /api/bootstrap`
- `GET /api/action-requests`
- `GET /api/artifacts/:id/content`

RPC 方法除了协议本身，还增加了本地 runtime 所需的 registry / runtime 操作：

- `agent.list`
- `agent.template.list`
- `agent.get`
- `agent.save`
- `agent.delete`
- `runtime.list`
- `runtime.start`
- `runtime.stop`
- `runtime.log.read`

当前 app 的本地 RPC binding 使用 dot 形式方法名，例如 `scope.read`、`handoff.create`。
协议文档中的 canonical 名称仍使用 slash 形式。

### 4.3 投递模型

事件入库后会按显式定向接收者写 `deliveries`：

1. relation 里被 `targets` / `hands_off_to` 指向的 actor

`scope.subscribe` 只决定哪个连接接收实时流，不决定谁属于 scope 的持续上下文。
持续上下文由 `memberships` 表示，离线 actor 的 directed event 才会进入 `pending`。

## 5. Runtime 设计

### 5.1 真实 ACP adapter

`server/acp.js` 现在已经是一个真正的 ACP stdio adapter，不再是 HTTP/SSE mock runtime：

- 启动子进程
- `initialize`
- 可选 `authenticate`
- `session/new`
- `session/prompt`
- `session/request_permission`
- `session/update`
- `session/cancel`

它的行为对齐 `joi` Rust `acp.rs` 的基本模式：

- 行分隔 JSON-RPC
- ACP permission request 转成 GUI 可见的 `action.request`
- ACP message chunk / tool call 转成协议 event

### 5.2 Agent registry

Registry 会扫描两套来源：

1. `data/agents/*.json`
2. `~/.agentx/agents/*/spec.yaml`

第二套不是直接运行文档，而是当作“可安装模板”。

默认 seeded persona 会优先绑定本机已安装的真实 ACP：

- `actor_agent_planner` 优先选择 `claude-acp`
- `actor_agent_builder` 优先选择 `codex-acp`

如果本地旧文档还是早期 `joi-http-v1` / `examples/runtime-agent.js` 形式，会自动迁移为 ACP transport。

### 5.3 Runtime manager

`server/runtime.js` 负责：

- 按需启动 agent
- 维护 runtime 状态
- 为每个 agent 分配独立 `/tmp` 工作目录
- 把 targeted event 排队给对应 agent
- 打开 `Turn`
- 把 ACP 输出映射成：
  - `content.add`
  - `tool.report`
  - `action.request`
  - `turn.close`
- 把 runtime status / runtime log 推给 GUI

### 5.4 Workspace

每个 agent 启动时都会生成新的 session root：

```text
/tmp/joi-agent-<actor-id>-xxxxxx/
  workspace/
  cache/
  logs/
```

支持的模板变量：

- `{agent.workspace}`
- `{agent.cache}`
- `{agent.logs}`
- `{agent.root}`
- `{channel.workspace}`
- `{channel.cache}`
- `{channel.logs}`
- `{channel.root}`
- `{session.root}`

## 6. GUI 设计

GUI 已经换成协作平台式布局，而不是早期“控制台按钮墙”。

### 6.1 主界面

主界面只保留人真正高频的动作：

- scope 导航
- timeline
- compose / dispatch
- pending approval
- runtime 摘要

### 6.2 设置抽屉

所有 runtime / agent 运维操作都移到设置抽屉：

- agent roster
- agent 编辑
- start / stop
- adapter catalog
- runtime detail
- runtime log

这符合你要求的“像 joi / Discord 一样，主页面不要堆满按钮”。

## 7. 已知真实问题

这里记录的是实际联调时遇到的约束。

1. 某些 ACP adapter 启动依赖家目录写权限，例如 Cursor 会写 `~/.cursor`。
2. 某些 ACP adapter 启动依赖外部网络或自己的远程服务，例如 OpenCode 会探测 models 服务。
3. 所以“真实 ACP 能否成功启动”不只取决于本框架，也取决于 adapter 本机环境。

这些都不是协议层或本框架的 mock 问题，而是 runtime 自身环境要求。

## 8. 后续如果继续收口

如果还要往 `joi` 正式产品的完成度继续收，下一批最值得补的是：

1. thread context 文件化，比如 `summary.md` / `handoff.md`
2. `tool.report` 的专门可视化视图
3. adapter 安装 / 更新 / 健康检查的完整设置流
4. 更强的 permission trace / delivery trace / audit trace
