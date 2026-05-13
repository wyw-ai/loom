# 文档索引

## 当前实现

- [current-app-implementation.md](./current-app-implementation.md) —— 当前 app 实现技术总览，适合作为入口
- [architecture.md](./architecture.md) —— 当前架构（进程边界 / 数据归属 / 协议 / 调度循环 / 取消 / actor 管理 / 代码边界）
- [architecture-v1-agent-client.md](./architecture-v1-agent-client.md) —— agent client 拆分、adapter 模型与 v1 部署方式
- [service-plugin-system-design.md](./service-plugin-system-design.md) —— service / plugin host 设计（runtime 引用此文档为权威规范）
- [scheduler-plugin.md](./scheduler-plugin.md) —— `joi service serve` 内的 cron 任务插件
- [am-joi-bridge.md](./am-joi-bridge.md) —— `am listen` 钉钉 bot 消息接入 Joi thread 的最小链路
- [command-transport-v0.md](./command-transport-v0.md) —— Command Transport 协议规范（已落地）
- [interactive-command-agent-transport-design.md](./interactive-command-agent-transport-design.md) —— interactive command transport 设计
- [gui-desktop-design.md](./gui-desktop-design.md) —— Joi Desktop GUI 设计与设计 token 来源
- [protocol/task-workflow.md](./protocol/task-workflow.md) —— channel 顶层消息上的 task / assignment 闭环模型和 CLI/RPC 用法

## 频道 / actor 拓扑

- [channel-topology-design.md](./channel-topology-design.md) —— a1-dev-canfeng / classroom 两个生产 channel 的职责切分、thread 拓扑、actor 配置、工作流
- [a1-dev-canfeng-final-actors.md](./a1-dev-canfeng-final-actors.md) —— a1-dev-canfeng 当前生效的最终 actor 分工、状态机、审查 gate 和 MR 推进规则
- [examiner-actor-design.md](./examiner-actor-design.md) —— a1-dev-canfeng 审查员 actor 设计：五件套审查、MR 审查、终态策略和 handoff 边界
- [artifact-contracts.md](./artifact-contracts.md) —— 跨 actor 的 artifact JSON schema 与契约

## 端到端验证

- [e2e-runbook-local.md](./e2e-runbook-local.md) —— 本地 e2e runbook（spec_apply / mr-detector / classroom / a1-auto-dev / thread.closed）

## 协议（来自上游 `joi/docs`）

- [protocol/open-multi-actor-collaboration-protocol-v0.md](./protocol/open-multi-actor-collaboration-protocol-v0.md)
- [protocol/open-multi-actor-collaboration-schema-v0.md](./protocol/open-multi-actor-collaboration-schema-v0.md)
- [protocol/open-multi-actor-collaboration-research.md](./protocol/open-multi-actor-collaboration-research.md)
- [protocol/joi-multi-agent-mvp-share.md](./protocol/joi-multi-agent-mvp-share.md)
- [protocol/channel-workspace-model.md](./protocol/channel-workspace-model.md)
- [protocol/scheduler-context-share.md](./protocol/scheduler-context-share.md)
- [protocol/tool-execution-reply-overlap.md](./protocol/tool-execution-reply-overlap.md)
- [protocol/technical-spec.md](./protocol/technical-spec.md)
