# Skill 挂载指南

> 本文档说明如何在 ContextLayer 中挂载 Skill（技能），包括安装投影、
> 提示词注入、渐进式披露和与 MCP 的映射关系。
> 前置知识：[架构 Wiki](./architecture.md)、[配置参考](./configuration.md)。

---

## 背景

Loom 中的 Skill 通过 `bundle.skills` 安装投影到达 Provider。这是 Skill 到达
Provider 的**单一路径**：

```
spec.json bundle.skills → ensure_scope → ensure_workspace_skill_targets
  → 投影到 workspace 的 .claude/skills/, .agents/skills/ 等目录
  → Provider (Claude/GPT) 原生读取这些目录作为 skill 指令
```

此路径 **不经过** ContextResource chain，不消耗 Loom token budget。

> ⚠️ **未实现说明**：`scheme="skill"` 的 `SkillContextResource`（将 skill 指令
> 注入 Loom 提示词信封）在设计文档中描述过，但**当前代码中未实现**。本文档
> 保留其设计说明供未来参考，但实际使用中 Skill 仅通过 `bundle.skills` 安装投影
> 到达 Provider。

---

## Core Skill vs Plugin Skill 边界规范

> **防护规则**：防止 Plugin Skill 污染 Core Skill 仓库（`loom-skills`）。

### 判定标准

| 类别 | 判定条件 | 仓库位置 |
|---|---|---|
| **Core Skill** | 所有 Loom agent 都需要的通用协作知识；对应内置 CLI 命令 | `loom-skills` 仓库 |
| **Plugin Skill** | 只对特定 plugin 用户有意义；对应 plugin 提供的 ContextResource 或可选功能 | Plugin 仓库 `skills/` 目录 |

### Core Skill 包含

- Loom 协作操作（messaging、tasks、state、attachments）
- 内置 CLI 命令使用指南（`loom message`、`loom task`、`loom attachment` 等）
- 所有 agent 必须遵循的协作模式（路由、交接、状态管理）

### Core Skill 不包含

- Plugin 特定的 ContextResource 行为（warm_summary、session reset、Hot/Warm/Cold 模型）
- 可选功能的操作指南（自定义 Provider、数据源配置）
- 只在安装了特定 plugin 后才有意义的 skill

### Plugin Skill 存放规则

Plugin skill 存放在各自 plugin 仓库的 `skills/` 目录中，通过 `bundle.skills`
或 `plugin.json` 的 `skills` 字段投影到 agent workspace。

```
loom-skills/                    ← Core Skill 仓库
  skills/
    loom/                       ← 协作路由（core）
    attachment/                 ← 附件操作（core）

loom-plugin-context-tier/       ← Plugin 仓库
  skills/
    context-tier/               ← Hot/Warm/Cold 模型（plugin）
      SKILL.md
      references/
        hot-layer.md
        warm-layer.md
        cold-layer.md
```

---

## Skill 安装投影（bundle.skills）

### 配置位置

在 Agent 的 `spec.json` 中配置 `bundle.skills`：

```json
{
  "id": "my-agent",
  "bundle": {
    "skills": [
      {
        "id": "loom",
        "source": "{agent.bundle}/skills/loom"
      },
      {
        "id": "obsidian",
        "source": "git@github.com:user/skill-obsidian.git"
      }
    ]
  }
}
```

### 安装流程

```
spec.json bundle.skills
  ↓
ensure_scope()
  ↓
ensure_workspace_skill_targets()
  ↓
投影到 workspace 目录:
  .claude/skills/<skill-id>/SKILL.md
  .agents/skills/<skill-id>/SKILL.md
  workspace/skills/<skill-id>/SKILL.md
  ↓
Provider (Claude/GPT) 原生读取这些目录
```

### 安装后的目录结构

```
<profile_dir>/workspace/
  └── skills/
      ├── loom/
      │   └── SKILL.md
      ├── obsidian/
      │   └── SKILL.md
      └── custom-skill/
          └── SKILL.md
```

此维度 **不经过** ContextResource chain，不消耗 Loom token budget。

---

## 未来扩展：scheme="skill" 注入（未实现）

> ⚠️ **本节描述的功能在设计文档中规划过，但当前代码中未实现。**
> `SkillContextResource` 结构体不存在于代码库中。以下内容仅作为设计参考保留。

### 配置位置

在 `agentcontext.json` 中声明 `scheme="skill"` 资源：

```json
{
  "version": 1,
  "resources": [
    {
      "scheme": "skill",
      "mount": "workspace-skills",
      "priority": 8,
      "config": {
        "read_instruction": true,
        "max_skills": 5,
        "instruction_file": "SKILL.md"
      }
    }
  ]
}
```

### 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `read_instruction` | `bool` | `true` | 是否读取并注入 skill 指令文件 |
| `max_skills` | `u64` | `5` | 最多注入的 skill 数量 |
| `instruction_file` | `String` | `"SKILL.md"` | 指令文件名 |

### 注入流程

```
agentcontext.json scheme="skill"
  ↓
build_context_resource_chain() → SkillContextResource 注册
  ↓
assemble_chain() 调用 SkillContextResource.assemble()
  ↓
扫描 workspace/skills/*/SKILL.md
  ↓
读取每个 skill 指令文件（受 max_skills 限制）
  ↓
格式化为 PromptSection（参与预算瀑布）
  ↓
注入 Loom 提示词信封
```

此维度 **消耗** token budget，参与预算瀑布控制。

---

## 完整示例：从安装到 Provider 读取

### 步骤 1：配置 Skill 安装

`spec.json`：
```json
{
  "id": "my-agent",
  "provider": { "type": "claude" },
  "bundle": {
    "skills": [
      { "id": "loom", "source": "{agent.bundle}/skills/loom" },
      { "id": "writing", "source": "{agent.bundle}/skills/writing" }
    ]
  },
  "context_layer": {
    "version": 1,
    "resources": [
      { "scheme": "memory", "mount": "memory", "priority": 5 },
      { "scheme": "message-list", "mount": "messages", "priority": 10 }
    ]
  }
}
```

### 步骤 2：Skill 自动投影

Agent 启动时，`ensure_scope()` 自动将 skill 投影到 workspace：

```
workspace/skills/
  ├── loom/SKILL.md      ← 从 bundle 源投影
  └── writing/SKILL.md   ← 从 bundle 源投影
```

### 步骤 3：Provider 读取 Skill

每轮 `compose_with_context_chain()` 执行时：

1. MemoryProvider 注入记忆段落（priority=5）
2. MessageListProvider 注入投递上下文（priority=10）
3. Provider（Claude/GPT）原生读取 `workspace/skills/` 目录中的
   `loom/SKILL.md` 和 `writing/SKILL.md`，作为 skill 指令加载

最终提示词信封结构：

```
[profile_prompt_files]
[runtime_context]
[bootstrap_memory]          ← MemoryProvider
[delivery_context]          ← MessageListProvider
[warm_summary]（如果存在）
[user_message]
```

> 注：Skill 指令由 Provider 原生 skill 系统加载，不经过 ContextResource chain，
> 不出现在上述 Loom 提示词信封结构中。

---

## SKILL.md 格式说明

### 基本格式

```markdown
---
name: loom-operations
description: Loom 多 Actor 协作操作指南
version: 1.0.0
---

# Loom Operations Skill

## 何时使用

当需要协调多个 Actor 的工作流时使用此 Skill。

## 核心操作

### 消息路由
- `message ask @actor` — 唤醒指定 Actor
- `message send` — 发送通知（不唤醒）

### 任务管理
- `task claim` — 认领任务
- `task complete` — 完成任务
```

### Frontmatter 字段

| 字段 | 说明 |
|---|---|
| `name` | Skill 标识符 |
| `description` | 一句话描述（用于 Tier 0 metadata catalog） |
| `version` | Skill 版本 |

### 正文结构建议

```markdown
# <Skill Name>

## 何时使用
<触发条件>

## 核心操作 / 核心概念
<主要功能说明>

## 示例
<使用示例>
```

---

## 渐进式披露（Tier 0/1/2）

遵循 Agent Skills 开放标准（agentskills.io）的渐进式披露模式：

| Tier | 内容 | Token 开销 | 加载时机 |
|---|---|---|---|
| **Tier 0** | Metadata（name + description） | ~100 tokens/skill | 始终在上下文中 |
| **Tier 1** | Body（完整 SKILL.md 正文） | <5K tokens/skill | 按需加载 |
| **Tier 2** | 引用文件（SKILL.md 中链接的外部文件） | 不定 | 按需读取 |

### D3 最小实现

当前 D3 阶段实现 Tier 0 + Tier 1（通过 Provider 原生 skill 系统）：
- **Tier 0**：Provider 读取 frontmatter 的 `name` 和 `description`
- **Tier 1**：Provider 读取完整 `SKILL.md` 正文

### 未来扩展（D4+）

- **Tier 2**：解析 SKILL.md 中的文件引用，按需读取外部文件
- **智能加载**：根据当前任务语义相关性，动态选择加载哪些 Tier 1 body

---

## 与 MCP Resources 的映射关系

MCP（Model Context Protocol）定义了三种原语，Skill 与它们的映射关系：

| MCP 原语 | 类型 | Loom 对应 | 说明 |
|---|---|---|---|
| **Resources** | 被动数据 | ContextResource | Skill 主要映射为此 — 被动注入指令/知识到 prompt |
| **Tools** | 主动执行 | Loom CLI / Agent 工具 | Skill 不映射为此 — Skill 是 Context Capability，不是 Action Capability |
| **Prompts** | 用户模板 | — | Skill 不直接映射 |

**核心区分**（来自业界共识）：
> "Skills tell agents **how to think**, Tools give agents **callable functions**."
> — CrewAI 文档

Skill 是 **Context Capability**（上下文能力），不是 Action Capability（行动能力）。
这意味着 Skill 内容被注入提示词作为指令/知识，而不是作为可调用的函数。

---

## plugin.json 格式（Phase 3）

Phase 3 引入了 `plugin.json` 清单文件，用于声明 plugin 的元数据、skill 范围
和 context resource 注册信息。

### 完整格式

```json
{
  "$schema": "loom-plugin/v1",
  "id": "loom-plugin-context-tier",
  "name": "Context Tier — Hot/Warm/Cold Temperature Model",
  "version": "1.0.0",
  "loom_version": ">=0.1.8",
  "layer": "context",
  "skills": [
    {
      "id": "context-tier",
      "path": "skills/context-tier",
      "scope": "global"
    }
  ],
  "context_resources": [
    {
      "scheme": "warm-summary",
      "priority": 7,
      "scope": "global",
      "registration": "inventory",
      "crate": "loom-plugin-context-tier"
    }
  ],
  "config_template": "context-resources/default-agentcontext.json"
}
```

### 字段说明

| 字段 | 说明 |
|---|---|
| `id` | Plugin 唯一标识，遵循 `loom-plugin-<layer>-<feature>` 命名规范 |
| `layer` | Plugin 所在层（如 `context`），用于分类和路由 |
| `skills` | Skill 声明列表（id, path, scope） |
| `context_resources` | ContextResource 声明列表（scheme, priority, scope, registration, crate） |
| `registration` | 注册方式：`inventory`（编译时自注册） |
| `config_template` | 默认 agentcontext.json 模板路径 |

### build.rs 双路径处理

| 条件 | 行为 |
|---|---|
| 有 plugin.json | Full plugin 路径：解析 scope/priority/resources，多维调度 |
| 无 plugin.json | Pure skill 路径：扫描 skills/ 目录，所有 skill 为 global scope（向后兼容） |

### 命名规范

格式：`loom-plugin-<layer>-<feature>`

| 维度 | 示例 |
|---|---|
| 仓库名 | `loom-plugin-context-tier` |
| Cargo crate | `loom-plugin-context-tier` |
| Rust 标识符 | `loom_plugin_context_tier` |
| plugin.json id | `loom-plugin-context-tier` |

---

## 常见问题 FAQ

### Q: bundle.skills 和 scheme="skill" 必须同时配置吗？

A: 当前只需配置 `bundle.skills`。
- 配置 `bundle.skills`：Skill 投影到 workspace，Provider 原生读取
- `scheme="skill"`（SkillContextResource）：**当前未实现**，配置后不会产生效果

### Q: Skill 指令太长会怎样？

A: Skill 指令由 Provider 原生 skill 系统加载，不参与 Loom ContextResource chain
的预算瀑布控制。Token 开销由 Provider 自身的 skill 加载机制管理。

### Q: 可以只使用 bundle.skills 吗？

A: 可以，这也是当前唯一可用的路径。配置 `bundle.skills` 后，Skill 自动投影到
workspace 目录，Provider（如 Claude）原生读取这些目录作为 skill 指令。

### Q: Skill 来源支持哪些格式？

A: `bundle.skills` 的 `source` 字段支持：
- 本地路径：`{agent.bundle}/skills/loom`
- Git 仓库：`git@github.com:user/skill.git`