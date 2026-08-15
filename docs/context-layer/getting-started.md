# ContextLayer — 快速入门

> **一句话定义**：ContextLayer 是一个可插拔的上下文组装系统，
> 允许每个 Agent 声明其每轮提示词如何从多个来源（记忆、消息历史、文件、
> 摘要）构建，通过优先级排序的资源链和 Token 预算瀑布机制进行组装。

---

## ContextLayer 解决什么问题

没有 ContextLayer 时，Loom 硬编码提示词信封：系统提示词 →
运行时上下文 → 记忆段落 → 用户消息。每个 Agent 获得相同的结构。
长时间运行的会话会累积 Token 膨胀，因为没有机制可以跳过、排序或
预算上下文段落。

ContextLayer 引入了 **ContextResource 链**：每个 Agent 声明它需要
哪些资源、以什么优先级顺序，运行时只组装那些在剩余 Token 预算内
的资源。

---

## D2 默认化 — 无需配置即可使用

从 commit 3b391ba 起，**所有 Agent 默认使用 D2 ContextResource chain**。
无需在 `spec.json` 中配置 `context_layer` — 当该字段为 `null` 或不存在时，
运行时自动使用 `default_agent_context_spec()` 提供的默认 chain：

| 资源 | 优先级 | 说明 |
|---|---|---|
| memory | 5 | Agent 记忆（bootstrap + turn） |
| warm-summary | 7 | 持久化的会话摘要 |
| message-list | 10 | 对话历史 / delivery context |

### 何时自定义 ContextLayer

默认 chain 已满足大多数场景。以下情况建议自定义：

- 需要将工作区文件注入上下文（添加 `file` scheme）
- 需要挂载 Skill 指令（添加 `skill` scheme）
- 需要精细控制上下文预算分配
- 多作用域 Agent（Thread + Channel），不同作用域有不同上下文需求

---

## 自定义配置

### 步骤 1：在 spec.json 中自定义 chain

在 Agent 的 `spec.json` 中添加 `context_layer`（覆盖默认 chain）：

```json
{
  "context_layer": {
    "version": 1,
    "effective_scope": ["thread", "channel"],
    "resources": [
      {
        "scheme": "memory",
        "mount": "memory",
        "priority": 5
      },
      {
        "scheme": "warm-summary",
        "mount": "warm",
        "priority": 7
      },
      {
        "scheme": "message-list",
        "mount": "messages",
        "priority": 10
      },
      {
        "scheme": "file",
        "mount": "workspace-docs",
        "priority": 20,
        "config": { "path": "${workspace.dir}/docs" }
      }
    ]
  }
}
```

### 步骤 2（可选）：添加 agentcontext.json

在 Agent 的 profile 目录中创建 `agentcontext.json`，覆盖或扩展
`spec.json` 中的声明：

```json
{
  "version": 1,
  "effective_scope": ["thread", "channel"],
  "resources": [
    {
      "scheme": "memory",
      "mount": "memory",
      "priority": 5
    },
    {
      "scheme": "message-list",
      "mount": "messages",
      "priority": 10
    },
    {
      "scheme": "file",
      "mount": "workspace-docs",
      "priority": 20,
      "config": {
        "path": "${workspace.dir}/docs",
        "max_files": 10
      }
    }
  ]
}
```

### 步骤 3：重载 Agent

```sh
loom agent reload <actor_id>
```

该 Agent 的下一轮将使用 ContextResource 链。

---

## 作用域继承

ContextLayer 配置在三个层级间合并，上层对相同 `scheme` 的声明覆盖下层：

1. **Agent 级**（`spec.json` 的 `context_layer` 字段）— 基础配置
2. **Profile 级**（`<profile_dir>/agentcontext.json`）— 覆盖 Agent 级
3. **工作区级**（`<profile_dir>/workspace/agentcontext.json`）— 覆盖 Profile 级

上层中与下层相同 `scheme` 的资源声明会 **完全替换** 该资源。新 `scheme`
的资源会被 **追加**。

---

## 接下来会发生什么

当 Agent 触发一轮时（无论是否配置了 `context_layer`）：

1. 运行时加载 `context_layer`（如为 None 则使用 `default_agent_context_spec()`）
2. 加载并合并三层 `agentcontext.json`
3. 从合并后的配置构建 `ContextResourceRegistry`（memory 经 inventory 注册的工厂从 config envelope 构造，交由 `plugin-memory` 的 `MemoryResource`）
4. 组装固定段落（profile 提示词文件、运行时上下文）
5. 运行链：按优先级顺序调用每个资源的 `assemble()`
6. 应用 Token 预算瀑布：超出剩余预算的段落被 **跳过**（不截断）
7. 最后追加用户消息

参见 [架构 Wiki](./architecture.md) 了解完整设计，或
[配置参考](./configuration.md) 了解完整 schema 详情。