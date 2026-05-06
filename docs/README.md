# 文档索引

## 当前实现

- [architecture.md](./architecture.md) —— 进程边界 / 数据归属 / 协议 / 调度循环 / 取消 / actor 管理 / 代码边界
- [service-plugin-system-design.md](./service-plugin-system-design.md) —— service / plugin host 设计（runtime 引用此文档为权威规范）
- [scheduler-plugin.md](./scheduler-plugin.md) —— `joi service serve` 内的 cron 任务插件
- [am-joi-bridge.md](./am-joi-bridge.md) —— `am listen` 钉钉 bot 消息接入 Joi thread 的最小链路
- [command-transport-v0.md](./command-transport-v0.md) —— Command Transport 协议规范（已落地）
- [gui-desktop-design.md](./gui-desktop-design.md) —— Joi Desktop GUI 设计与设计 token 来源

## 频道 / actor 拓扑

- [channel-topology-design.md](./channel-topology-design.md) —— a1-dev-canfeng / classroom 两个生产 channel 的职责切分、thread 拓扑、actor 配置、工作流
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
