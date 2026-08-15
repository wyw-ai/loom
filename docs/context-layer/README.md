# ContextLayer 文档

Loom 的可插拔上下文组装系统，用于 Agent 提示词构建。

## 文档列表

| 文档 | 目标读者 | 用途 |
|---|---|---|
| [快速入门](./getting-started.md) | 所有用户 | 快速开始：ContextLayer 是什么、何时启用、最小配置 |
| [架构 Wiki](./architecture.md) | 维护者、架构师 | 核心概念、数据流、设计约束、Token 预算瀑布、Skill 安装投影 |
| [配置参考](./configuration.md) | 运维人员 | 完整 `agentcontext.json` schema、各 scheme 配置、合并语义 |
| [API 参考](./api-reference.md) | 开发者 | Trait 方法签名、结构体字段、自定义 Provider 示例 |
| [源码导航](./source-navigation.md) | 贡献者 | 关键文件路径与行号 |
| [MessageList Warm/Cold](./message-list-warm-cold.md) | 所有用户 | Hot/Warm/Cold 分层、Session Reset、摘要生成 |
| [Skill 挂载指南](./skill-mounting-guide.md) | 所有用户 | Skill 安装投影、渐进式披露、MCP 映射 |
| [数据源最佳实践](./data-source-best-practices.md) | 运维人员、架构师 | 各数据源配置示例、优先级分配、安全约束 |
| [自定义 Provider 指南](./custom-provider-guide.md) | 开发者 | ContextResource/ResourceProvider 实现、注册、安全约束 |

## 快速链接

- **启用 ContextLayer**：参见 [快速入门 → 最小配置](./getting-started.md#最小配置)
- **理解预算瀑布**：参见 [架构 Wiki → Token 预算瀑布](./architecture.md#token-预算瀑布)
- **编写自定义 Provider**：参见 [自定义 Provider 指南](./custom-provider-guide.md)
- **挂载 Skill**：参见 [Skill 挂载指南](./skill-mounting-guide.md)
- **配置数据源**：参见 [数据源最佳实践](./data-source-best-practices.md)
- **查找源码位置**：参见 [源码导航](./source-navigation.md)

## 相关信息

- 分支：`feat/add-context-tier-skill-source`
- 设计文档：`loom-context-lifecycle-research-2026-08-13/`（Obsidian 知识库）
- Phase 3 文档：`phase3-plugin-independence/`（Plugin Independence 阶段归档）
- Proto 类型：`crates/proto/src/methods.rs`（`AgentContextSpec`、`ContextResourceSpec`）
- 共享接口 crate：`crates/context-layer-core/`（ContextResource trait, inventory 注册）
- Plugin 仓库：`loom-plugin-context-tier`（warm-summary + message-list 自注册）
