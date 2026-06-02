# 文档索引

## 当前实现

- [current-app-implementation.md](./current-app-implementation.md) —— 当前 app 实现技术总览，适合作为入口
- [architecture.md](./architecture.md) —— 当前架构（进程边界 / 数据归属 / 协议 / 调度循环 / 取消 / actor 管理 / 代码边界）
- [loom-refactor-plan.md](./loom-refactor-plan.md) —— Loom 破坏性重构技术设计
- [architecture-v1-agent-client.md](./architecture-v1-agent-client.md) —— agent client 拆分、adapter 模型与 v1 部署方式
- [service-plugin-system-design.md](./service-plugin-system-design.md) —— service / plugin host 设计（runtime 引用此文档为权威规范）
- [scheduler-plugin.md](./scheduler-plugin.md) —— `loom service serve` 内的 cron 任务插件
- [am-loom-bridge.md](./am-loom-bridge.md) —— `am listen` 钉钉 bot 消息接入 Loom thread 的最小链路
- [command-transport-v0.md](./command-transport-v0.md) —— Command Transport 协议规范（已落地）
- [interactive-command-agent-transport-design.md](./interactive-command-agent-transport-design.md) —— interactive command transport 设计
- [gui-desktop-design.md](./gui-desktop-design.md) —— Loom Desktop GUI 设计与设计 token 来源
- [gui-actor-provider-management-design.md](./gui-actor-provider-management-design.md) —— GUI 中 actor / provider / host runtime 维护的目标架构、现状差距与落地路线
- [protocol/task-workflow.md](./protocol/task-workflow.md) —— channel 顶层消息上的 task / assignment 闭环模型和 CLI/RPC 用法
- [protocol/agent-coordination-workflow.md](./protocol/agent-coordination-workflow.md) —— 多 agent “开工前 claim，发送前 rebase”的协作协议

## 频道 / actor 拓扑

- [channel-topology-design.md](./channel-topology-design.md) —— a1-dev-canfeng / classroom 两个生产 channel 的职责切分、thread 拓扑、actor 配置、工作流
- [a1-dev-canfeng-final-actors.md](./a1-dev-canfeng-final-actors.md) —— a1-dev-canfeng 当前生效的最终 actor 分工、状态机、审查 gate 和 MR 推进规则
- [a1-dev-canfeng-badcases.md](./a1-dev-canfeng-badcases.md) —— a1-dev-canfeng 真实任务 bad case 样本，用于后续推演
- [a1-dev-canfeng-normalcases.md](./a1-dev-canfeng-normalcases.md) —— a1-dev-canfeng 真实任务 normal case 样本，用于后续推演
- [a1-dev-actor-memory-operating-architecture.md](./a1-dev-actor-memory-operating-architecture.md) —— a1-dev-canfeng 下一版 actor / 研发规范 / Loom memory 终态运行架构
- [a1-dev-engineering-standards-agent-scope.md](./a1-dev-engineering-standards-agent-scope.md) —— 研发规范员 actor、debug playbook、validation-plan / validation-evidence 的改动范围
- [examiner-actor-design.md](./examiner-actor-design.md) —— a1-dev-canfeng 审查员 actor 设计：五件套审查、MR 审查、终态策略和 directed message 边界
- [artifact-contracts.md](./artifact-contracts.md) —— 跨 actor 的 artifact JSON schema 与契约

## 端到端验证

- [e2e-runbook-local.md](./e2e-runbook-local.md) —— 本地 e2e runbook（spec_apply / mr-detector / classroom / a1-auto-dev / thread.closed）

## 协议（来自上游 `loom/docs`）

- [protocol/open-multi-actor-collaboration-protocol-v0.md](./protocol/open-multi-actor-collaboration-protocol-v0.md)
- [protocol/open-multi-actor-collaboration-schema-v0.md](./protocol/open-multi-actor-collaboration-schema-v0.md)
- [protocol/open-multi-actor-collaboration-research.md](./protocol/open-multi-actor-collaboration-research.md)
- [protocol/channel-workspace-model.md](./protocol/channel-workspace-model.md)
- [protocol/scheduler-context-share.md](./protocol/scheduler-context-share.md)
- [protocol/tool-execution-reply-overlap.md](./protocol/tool-execution-reply-overlap.md)
- [protocol/technical-spec.md](./protocol/technical-spec.md)
