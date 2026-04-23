# 当前 App 实现技术总览

> **架构演进说明**
>
> 本文档描述 **v0 实现现状**：agent runtime 由 `joi-server` 内嵌托管。该路径
> 仍是默认部署。
>
> **v1 拓扑**（agent runtime 剥成独立 `joi agent serve` 进程、adapter 抽象
> 支持 ACP 与 command 两类 transport）的代码已落 phase E1–E3：在 server 端
> 设置 `JOI_DISABLE_EMBEDDED_RUNTIME=1` 即切到 v1 部署，由 `joi agent serve`
> 接管 supervisor。设计文档见
> [docs/architecture-v1-agent-client.md](architecture-v1-agent-client.md) 与
> [docs/command-transport-v0.md](command-transport-v0.md)。
>
> v0 内容不再修改——它准确描述当前默认拓扑下的代码。

## 1. 文档目的

这份文档描述 `joi-apps` 当前这版实现到底已经落到了什么程度，以及它和协议设计文档之间的对应关系。

目标不是重复协议定义，而是回答四个更实际的问题：

1. 当前 app 的产品形态是什么。
2. 协议对象在当前代码里分别落到了哪里。
3. server、runtime、GUI 现在各自负责什么。
4. 当前实现还缺什么，哪些能力已经是“真的能跑”的。

协议本身的设计文档已经收进当前仓库的 [docs/protocol/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol)。

## 2. 当前定位

这版 `joi-apps` 不再是协议说明页，也不是纯 demo 控制台。

当前产品形态是一个面向人和多 agent 协作的本地工作台：

- 顶层协作域是 `Room`，对应协议里的 `Channel`
- 分支协作域是 `Thread`，对应协议里的 `Thread`
- 人和 agent 都是平等的 `Actor`
- 协作事实通过不可变 `Event` 进入时间线
- agent 的一次执行被组织成 `Turn`
- 文件和产物通过 `Artifact` 独立发布
- 审批 / 权限请求通过 `action.request` 与 `Receipt` 回写闭环

前端产品命名上，这版已经收口成 `Joi Workroom`，强调“协作工作台”，不是“协议展示器”。

## 3. 文档地图

当前项目里的文档建议按下面方式看：

- [README.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/README.md)
  入口说明，偏运行和能力总览
- [docs/architecture.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/architecture.md)
  较早的一版架构说明，保留设计判断和实现边界
- [docs/current-app-implementation.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/current-app-implementation.md)
  当前这份文档，作为实现现状总览
- [docs/protocol/open-multi-actor-collaboration-protocol-v0.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
  协议核心模型
- [docs/protocol/open-multi-actor-collaboration-schema-v0.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-schema-v0.md)
  协议 schema 视角
- [docs/protocol/channel-workspace-model.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/channel-workspace-model.md)
  channel / workspace 语义
- [docs/protocol/scheduler-context-share.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/scheduler-context-share.md)
  scheduler / context share 设计
- [docs/protocol/tool-execution-reply-overlap.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/tool-execution-reply-overlap.md)
  tool execution 与 reply 关系
- [docs/protocol/technical-spec.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/technical-spec.md)
  更偏整体 technical spec
- [docs/protocol/joi-multi-agent-mvp-share.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/joi-multi-agent-mvp-share.md)
  产品与协作模型背景

## 4. 当前代码结构

### 4.1 目录

核心目录：

- [server/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server)
- [public/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/public)
- [data/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data)
- [docs/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs)
- [examples/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/examples)

### 4.2 职责划分

- [server/index.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/index.js)
  HTTP server、RPC 分发、SSE、静态资源服务
- [server/store.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/store.js)
  协议对象存储与投递模型
- [server/agents.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/agents.js)
  agent registry、模板发现、默认 persona seed
- [server/runtime.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/runtime.js)
  hosted runtime 生命周期、事件入队、turn 映射、日志与状态
- [server/acp.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/acp.js)
  ACP stdio adapter
- [public/app.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/public/app.js)
  GUI 状态、渲染、交互逻辑
- [public/index.html](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/public/index.html)
  主界面骨架
- [public/styles.css](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/public/styles.css)
  工作台布局与样式

## 5. 协议对象到实现的映射

| 协议对象 | 当前实现 | 说明 |
| --- | --- | --- |
| `Actor` | `actors` 表 + agent 文档映射 | 人、agent、system 都统一进 actor 模型 |
| `Endpoint` | `endpoints` 表 | GUI session 和 runtime 接入点 |
| `Channel` | `channels` 表 | 产品中的 room |
| `Thread` | `threads` 表 | 产品中的 thread |
| `Turn` | `turns` 表 | 一次 agent 执行回合 |
| `Event` | `events` 表 | 消息、tool update、审批、状态变化 |
| `Relation` | `relations` 表 | reply、target、handoff、artifact attach |
| `Artifact` | `artifacts` 表 + `data/artifacts/` | 独立共享产物 |
| `Membership` | `memberships` 表 | actor 对 scope 的持续上下文归属 |
| `Delivery` | `deliveries` 表 | 事件投递状态 |
| `Receipt` | `receipts` 表 | action request 的处理状态 |

当前实现遵守的核心判断和协议一致：

- scope 是历史与订阅边界
- event 是不可变事实
- relation 必须显式
- actor 不区分人和 agent 的领域地位
- runtime session 不是协作上下文主键

## 6. Server 侧实现

### 6.1 API 入口

[server/index.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/index.js) 当前提供：

- `GET /api/health`
- `GET /api/bootstrap`
- `GET /api/action-requests`
- `GET /api/artifacts/:id`
- `GET /api/artifacts/:id/content`
- `GET /api/stream?session_id=...`
- `POST /api/rpc`

GUI 和 runtime 当前统一走 `POST /api/rpc`。

### 6.2 RPC 能力

协议主方法：

- `session.open`
- `scope.subscribe`
- `scope.read`
- `channel.create`
- `thread.create`
- `turn.open`
- `event.append`
- `turn.close`
- `artifact.publish`
- `receipt.record`

当前实现里，RPC 方法名使用 dot 形式，例如 `scope.read`、`event.append`。
协议文档里的 canonical 名称仍使用 slash 形式，例如 `scope/read`、`event/append`。

扩展方法：

- `agent.list`
- `agent.template.list`
- `agent.get`
- `agent.save`
- `agent.delete`
- `runtime.list`
- `runtime.start`
- `runtime.stop`
- `runtime.log.read`

### 6.3 数据存储

当前落盘位置：

- SQLite: [data/protocol.sqlite](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data/protocol.sqlite)
- artifact 文件: [data/artifacts/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data/artifacts)
- agent 文档: [data/agents/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data/agents)
- runtime 日志: [data/runtime-logs/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data/runtime-logs)

### 6.4 投递模型

[server/store.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/store.js) 负责在 `event.append` 后写入 `deliveries`。

接收者来源只有一类：

1. relation 中被 `hands_off_to` 显式指向的 actor

另外有两条与投递并行但独立的机制：

1. `scope.subscribe` 只负责把某个连接绑到实时流上，不参与 `deliveries` 计算
2. `memberships` 负责让 actor 在以后被重新唤醒时可以通过 `scope.read` 补看中间历史

在线 session 会通过 SSE 收到实时更新。离线 actor 的 directed event 保留在 `pending deliveries`，后续重连或 runtime 启动时补发。

## 7. Agent Registry 与模板

### 7.1 文档来源

[server/agents.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/agents.js) 维护两类来源：

1. 本地保存的 agent 文档：
   [data/agents/*.json](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/data/agents)
2. 本机已安装的 agentx 模板：
   `~/.agentx/agents/*/spec.yaml`

第二类当前被当作“可安装模板”暴露给 GUI，而不是直接作为运行文档。

### 7.2 默认 persona

当前会 seed 两个默认 agent：

- `Planner`
- `Builder`

映射顺序是基于本机已安装模板自动挑选，例如优先：

- `Planner` -> `claude-acp`
- `Builder` -> `codex-acp`

如果本机没有这些模板，会回退到 mock/fallback 方案，但 fallback 现在只是兜底，不是主路径。

### 7.3 文档结构

本地 agent 文档已经统一成 JSON 规格，核心字段包括：

- `spec.metadata`
- `spec.capabilities`
- `spec.runtime.transport`
- `spec.context`
- `spec.session`
- `identityMarkdown`
- `soulMarkdown`
- `toolPolicy`

## 8. Runtime 托管

### 8.1 Runtime manager

[server/runtime.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/runtime.js) 负责：

- 启停 runtime
- 维护 runtime state
- 为每个 agent 分配独立 session root
- 接收 targeted event 并入队
- 打开和关闭 turn
- 把 ACP 输出转换成协议 event
- 把状态和日志回推给 GUI

### 8.2 自动唤起

当前 runtime 会因为以下 relation 被自动唤起或入队：

- `hands_off_to`

纯文本里的 `@agent` 不会触发 runtime。只有显式定向关系才会触发真实运行闭环。

### 8.3 临时工作目录

每次启动 hosted agent 时都会创建独立的 session root，例如：

```text
/tmp/joi-agent-<actor-id>-xxxxxx/
  workspace/
  profile/
  logs/
```

模板变量会解析到这套目录：

- `{agent.workspace}`
- `{agent.profile}`
- `{agent.logs}`
- `{agent.root}`
- `{channel.workspace}`
- `{channel.cache}`
- `{channel.logs}`
- `{channel.root}`
- `{session.root}`

### 8.4 ACP adapter

[server/acp.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/server/acp.js) 当前是一个真正的 ACP stdio adapter，不是 mock HTTP relay。

它负责：

- `initialize`
- 可选 `authenticate`
- `session/new`
- `session/prompt`
- `session/request_permission`
- `session/update`
- `session/cancel`

当前映射逻辑：

- 文本 chunk -> 同 turn 内累积，turn 关闭时 flush 成单条 `content.add`；过程中 partial chunk 仅作 `turn/trace.update(text.delta)` 推 owner
- tool 调用更新 -> turn 私有 trace（`turn/trace.update(tool.start|tool.update|tool.end)`，仅推 owner，不进事件流）
- 内部状态变化 / 运行时错误 -> turn 私有 trace（`turn/trace.update(status|error)`）
- permission request -> `action.request`
- prompt 完成 -> `turn.close`

## 9. GUI 实现

### 9.1 当前产品信息架构

这版 GUI 已经从“协议对象列表 + 设置后台”收口为协作工作台：

- 左栏：room 导航、room 创建、当前 operator
- 中间：shared focus、open threads、workstream、direct the work
- 右栏：decision inbox、people and agents
- 抽屉：crew、catalog、runtime

### 9.2 主要交互

[public/app.js](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/public/app.js) 当前支持：

- 打开 GUI session
- 订阅 scope
- 浏览 room / thread timeline
- 创建 room
- 以 reply 关系回帖
- 通过显式 handoff 把下一步工作交给 agent
- 在正文里保留 `@handle` 这种普通文本引用与高亮
- 从 room update 创建 thread
- 发布 artifact
- 响应 `action.request`
- 在抽屉里安装 / 编辑 / 启停 agent
- 查看 runtime 状态和 log

### 9.3 当前前端状态

前端全局状态围绕这些对象组织：

- `bootstrap`
- `selectedScope`
- `timeline`
- `pendingActionRequests`
- `agentDocuments`
- `installedTemplates`
- `runtimeStatuses`
- `artifactMetaById`
- `replyTargetId`
- `selectedHandoffActorId`
- `settingsOpen`

### 9.4 已完成的产品化收口

这版 GUI 最近已经做过一次从 demo 到产品化的收口：

- 首页命名改为 `Joi Workroom`
- 视图中心从“协议说明”改成“协作状态”
- 低级 runtime 配置被收进二级展开
- 审批卡和 system event 不再直接把原始 JSON 甩给用户
- 桌面和移动布局都做过响应式修正

## 10. 一条典型工作流

### 10.1 人发起协作

1. 人在 room 里发一条 `content.add`
2. 不论是否显式选择 handoff 目标，前端都调用 `event.append`；handoff 仅是 `content.add` 携带 `hands_off_to` relation 的特例
3. server 只会为 `hands_off_to` 写 directed `deliveries`
4. server 会为发言者或被定向到的 actor 维护 `memberships`
5. runtime manager 发现相关 directed relation
6. 如果 agent 未启动且允许按需启动，则自动启动 runtime
7. 事件入队
8. runtime 打开 turn 并向 ACP prompt

### 10.2 agent 反向请求审批

1. ACP runtime 发 `session/request_permission`
2. ACP adapter 转成 `action.request`
3. GUI 在 decision inbox 中展示
4. 人点击选择并回写 `action.response`
5. `receipt.record` 标记处理状态
6. runtime manager 继续调用 `respondPermission`

### 10.3 artifact 分享

1. 人或 agent 调 `artifact.publish`
2. 文件写入 `data/artifacts/<artifact-id>/`
3. event relation 通过 `attaches_artifact` 显式关联
4. GUI timeline 中把 artifact 渲染成 deliverable 卡片

## 11. 当前已经是真的部分

这里强调的是“已经真跑起来”的能力：

- 协议对象已经真实落 SQLite
- SSE session 已经工作
- agent registry 已经能发现本机模板
- hosted ACP runtime 已经能启动真实 stdio adapter
- runtime log 和 runtime state 已经能回到 GUI
- room / thread / explicit handoff / reply / action request / artifact 已经形成闭环

## 12. 当前缺口

还没有完全做完的部分主要是：

1. thread context 文件化产物还不完整
2. turn 私有 trace（tool 调用、status、error）还没有 owner-only 的可视化视图
3. adapter 安装、更新、健康检查流程还不完整
4. delivery trace / permission trace / audit trace 仍偏基础
5. 现在还是单机 / 本地 server-first 形态，不是跨端联邦形态

## 13. 建议的后续文档维护方式

为了避免后面再次出现“协议设计在一个 repo，实现总结散落在另一个 repo”的问题，建议后续保持：

- 协议设计文档统一沉淀在 [docs/protocol/](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol)
- 当前产品/实现总结统一更新 [docs/current-app-implementation.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/current-app-implementation.md)
- 较偏判断和阶段性思考的说明保留在 [docs/architecture.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/architecture.md)

这样这一个 repo 内就能同时闭环：

- 协议设计
- 当前实现
- 产品形态
- 后续演进
