# 数据源挂载最佳实践

> 本文档说明如何将各类数据源挂载到 ContextLayer，包括已有 Provider 的
> 配置示例和未来数据源的设计方向。
> 前置知识：[架构 Wiki](./architecture.md)、[配置参考](./configuration.md)。

---

## 背景

ContextLayer 通过 `ContextResource` Trait 统一管理各类数据源的提示词注入。
每个数据源通过 `scheme` 标识，在 `agentcontext.json` 中声明，参与预算瀑布控制。

本文档覆盖：
- 已有 Provider 的配置最佳实践（Memory、MessageList、FileSystem）
- 未来数据源的设计方向（SQLite、Obsidian）
- 多数据源组合配置
- 安全约束

---

## 已有 Provider

### Memory 挂载

包装现有记忆机制，将预渲染的记忆字符串注入提示词。

```json
{
  "scheme": "memory",
  "mount": "agent-memory",
  "priority": 5
}
```

**无需 `config`** — 运行时从 `MemorySpec` 预渲染记忆并传递给 `MemoryProvider`。

**最佳实践**：
- 优先级保持 5（高优先级，记忆是 Agent 身份的核心）
- 如果 Agent 未配置 `MemorySpec`，此 Provider 不贡献任何内容
- 记忆内容由现有 `build_envelope` 逻辑生成，不受 ContextLayer 控制

### MessageList 挂载

将投递上下文（Thread 消息、收件箱条目）包装为提示词段落。

```json
{
  "scheme": "message-list",
  "mount": "delivery",
  "priority": 10
}
```

**无需 `config`** — 运行时自动组装 `delivery_context`。

**最佳实践**：
- 优先级保持 10（标准优先级，对话历史是任务理解的基础）
- 此 Provider 是 Token 消耗的主要来源之一
- 长对话中 Session Reset 机制会自动触发摘要压缩

### WarmSummary 挂载

将持久化的 Warm 摘要挂载为上下文资源。

```json
{
  "scheme": "warm-summary",
  "mount": "warm",
  "priority": 7
}
```

**无需 `config`** — `WarmSummaryContextResource` 自动读取
`{profile_dir}/summaries/{scope_id}.md`。

**最佳实践**：
- 优先级 7（介于 memory=5 和 message-list=10 之间）
- 预算限制为剩余 Token 的 20%（`WARM_SUMMARY_BUDGET_FRACTION = 0.2`）
- 超预算时自动跳过（不截断，C-3 约束）
- 摘要由 Provider 生成并通过 `persist()` 原子写入，此资源只负责读取
- 仅在 D2 模式（`context_layer = Some`）下生效；D1 路径不注入 Warm 摘要

### FileSystem 挂载

将工作区文件挂载为上下文资源。

```json
{
  "scheme": "file",
  "mount": "workspace-docs",
  "priority": 20,
  "config": {
    "path": "${workspace.dir}/docs",
    "max_files": 10
  }
}
```

**配置参数**：

| 参数 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `path` | `String` | `"${workspace.dir}"` | 挂载目录，支持 `${workspace.dir}` 模板 |
| `max_files` | `u64` | `10` | 枚举和读取的最大文件数 |

**最佳实践**：
- **指定子目录**：避免挂载整个 workspace（可能包含大量无关文件），指定 `docs/` 或 `context/` 子目录
- **控制文件数**：`max_files` 根据实际需求设置，默认 10 是合理上限
- **优先级靠后**：文件内容通常是参考性的，优先级 20+ 让它在预算紧张时优先被跳过

**Glob 过滤建议**（未来增强方向）：
当前 FileSystemProvider 只列顶层文件。未来计划增加 `glob` 配置：

```json
{
  "scheme": "file",
  "mount": "workspace-docs",
  "priority": 20,
  "config": {
    "path": "${workspace.dir}/docs",
    "glob": "**/*.md",
    "max_files": 10
  }
}
```

---

## 未来数据源（设计方向）

> 以下数据源尚未实现，为 ARCH 设计方向（P2 优先级），供规划和扩展参考。

### SQLite 挂载

将 SQLite 数据库查询结果注入提示词。

**设计配置**：

```json
{
  "scheme": "sqlite",
  "mount": "task-db",
  "priority": 15,
  "config": {
    "path": "${workspace.dir}/data.db",
    "query": "SELECT id, title, status FROM tasks WHERE channel_id = ? ORDER BY updated_at DESC LIMIT 10",
    "param": "${scope.id}"
  }
}
```

**设计要点**：
- 使用 `rusqlite` crate（同步，不需要 async trait）
- **参数化查询**（防 SQL 注入）：`param` 字段支持 `${scope.id}`、`${actor.id}` 等模板
- 查询结果格式化为 Markdown table 注入提示词
- 路径遍历防护同 FileSystemProvider

**安全约束**：
- 必须使用参数化查询（`params![$param]`），禁止字符串拼接 SQL
- 禁止任意 SQL（查询语句在配置中预定义，不接受运行时输入）
- `MAX_READ_BYTES` 限制适用于查询结果输出

### Obsidian 挂载

将 Obsidian vault 中的笔记注入提示词。

**设计配置**：

```json
{
  "scheme": "obsidian",
  "mount": "vault-notes",
  "priority": 18,
  "config": {
    "vault_path": "C:/Users/hansi/OneDrive/note/ai_note/ai-note",
    "glob": "**/*.md",
    "max_files": 5
  }
}
```

**设计要点**：
- 遍历 `vault_path` + `glob` 模式匹配笔记文件
- 读取匹配文件（受 `MAX_READ_BYTES` 限制）
- 格式化为 PromptSection（含 frontmatter + body）
- 复用 FileSystemProvider 的路径遍历防护
- 可选：支持 wikilink 解析（`[[note]]`）→ 展开为完整内容

**使用场景**：
- 将项目相关的知识库笔记注入 Agent 上下文
- 按目录或 tag 过滤，只注入当前任务相关的笔记

---

## 多数据源组合配置

### 标准组合（推荐起点）

```json
{
  "version": 1,
  "effective_scope": ["thread", "channel"],
  "resources": [
    { "scheme": "memory", "mount": "agent-memory", "priority": 5 },
    { "scheme": "skill", "mount": "workspace-skills", "priority": 8 },
    { "scheme": "message-list", "mount": "delivery", "priority": 10 },
    { "scheme": "warm-summary", "mount": "warm", "priority": 7 },
    {
      "scheme": "file",
      "mount": "workspace-docs",
      "priority": 20,
      "config": { "path": "${workspace.dir}/docs", "max_files": 5 }
    }
  ]
}
```

### 研究型 Agent 组合

```json
{
  "version": 1,
  "effective_scope": ["thread", "channel"],
  "resources": [
    { "scheme": "memory", "mount": "agent-memory", "priority": 5 },
    { "scheme": "skill", "mount": "workspace-skills", "priority": 8 },
    { "scheme": "message-list", "mount": "delivery", "priority": 10 },
    {
      "scheme": "file",
      "mount": "research-notes",
      "priority": 15,
      "config": { "path": "${workspace.dir}/research", "max_files": 10 }
    },
    {
      "scheme": "file",
      "mount": "project-docs",
      "priority": 20,
      "config": { "path": "${workspace.dir}/docs", "max_files": 5 }
    }
  ]
}
```

> **注意**：当前合并语义按 `scheme` 去重，两个 `scheme="file"` 声明中后者
> 会覆盖前者。如需多目录挂载，需要使用不同 scheme 或等待多 mount 支持。
> 上例仅展示设计意图。

### 最小组合（仅基础）

```json
{
  "version": 1,
  "resources": [
    { "scheme": "memory", "mount": "memory", "priority": 5 },
    { "scheme": "message-list", "mount": "messages", "priority": 10 }
  ]
}
```

---

## 优先级分配建议

| 优先级范围 | 用途 | 示例 |
|---|---|---|
| `0` | 永不跳过的关键资源 | （保留） |
| `1-5` | Agent 身份核心 | Memory |
| `6-10` | 任务理解核心 | WarmSummary(7)、Skill(8)、MessageList(10) |
| `11-15` | 任务辅助数据 | 研究笔记、SQLite 查询结果 |
| `16-25` | 参考性数据 | 文件系统、Obsidian 笔记 |
| `26+` | 低优先级补充 | 日志、历史归档 |

**原则**：优先级越高（值越小），在预算紧张时越不容易被跳过。将最关键的
上下文资源放在高优先级，参考性数据放在低优先级。

---

## 安全约束

### C-3/C-6：禁止语义操作

所有 ContextResource 实现必须遵守：

| 禁止 | 允许 |
|---|---|
| 语义压缩 | 读取已持久化数据 |
| 关键词提取 | 格式化为 PromptSection |
| 内容截断 | 预算不足时跳过整个段落 |

**强制点**：`assemble_chain()`（context_layer.rs:209）— 超预算的段落被
skip 而非 truncate。

### 路径遍历防护

所有文件类 Provider 必须复用 FileSystemProvider 的防护模式：

```rust
let canonical = path.canonicalize()?;
if !canonical.starts_with(&base_canonical) {
    return Err(anyhow!("path traversal detected"));
}
```

### MAX_READ_BYTES 限制

| 资源类型 | 限制 |
|---|---|
| 单文件读取 | 128KB（`MAX_READ_BYTES`） |
| 自定义 Provider | 应遵循此限制或显式声明不同上限 |

### SQL 注入防护（SQLite Provider）

- 必须使用参数化查询：`params![$param]`
- 禁止字符串拼接 SQL
- 查询语句在配置中预定义，不接受运行时输入

---

## 常见问题 FAQ

### Q: 如何选择优先级？

A: 参考上文的优先级分配建议表。核心原则：Agent 身份和任务理解相关的数据
放高优先级，参考性数据放低优先级。

### Q: 文件太多导致 Token 超预算怎么办？

A: 调整 `max_files` 参数减少文件数量，或降低优先级让文件资源在预算紧张时
被跳过。也可以指定更精确的子目录路径。

### Q: 可以同时挂载多个文件目录吗？

A: 当前合并语义按 `scheme` 去重，同 `scheme` 的后者覆盖前者。如需多目录，
目前需要自定义 Provider 使用不同 scheme。未来可能支持多 mount 声明。

### Q: 数据源更新后如何刷新上下文？

A: ContextLayer 在每轮 `compose_with_context_chain()` 时重新读取数据源。
文件系统变更、SQLite 数据更新会在下一轮自动反映到提示词中。