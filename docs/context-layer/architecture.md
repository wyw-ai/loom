# ContextLayer — 架构 Wiki

> 本文档解释 ContextLayer 系统的设计概念、数据流和约束。
> 配置语法参见 [配置参考](./configuration.md)。Trait 方法签名
> 参见 [API 参考](./api-reference.md)。

---

## 核心概念

### ContextResource Trait

核心抽象。一个 `ContextResource` 是一个可插拔的来源，为零个或多个
`PromptSection` 贡献内容到每轮提示词信封。

```rust
pub trait ContextResource: Send + Sync {
    fn scheme(&self) -> &str;
    fn priority(&self) -> i32;
    fn effective_scope(&self) -> &[ScopeKind];
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>>;
}
```

- **`scheme()`** — URI scheme 标识符（如 `"memory"`、`"message-list"`、
  `"file"`）。注册表使用它将 `agentcontext.json` 声明路由到正确的 Provider。
- **`priority()`** — 组装顺序。值越小 = 越先组装（重要性越高）。
  超出预算的资源按优先级逆序跳过。优先级 `0` = 永不跳过（保留给关键资源）。
- **`effective_scope()`** — 该资源在哪些作用域类型（`Thread`、`Channel`）
  中生效。仅 Channel 的资源在 Thread 作用域中会被跳过。
- **`assemble()`** — 生成实际的 `PromptSection`。接收一个
  `AssemblyContext`，包含作用域元数据、预算信息和投递上下文。

### AssemblyContext

传递给每次 `assemble()` 调用的只读上下文。包含资源所需的一切，
不暴露可变状态：

| 字段 | 类型 | 说明 |
|---|---|---|
| `scope` | `&ScopeRef` | 当前轮运行的作用域（Thread 或 Channel） |
| `channel_id` | `Option<&str>` | Channel ID（如果可用） |
| `actor_id` | `&str` | Agent 的 actor ID |
| `profile_dir` | `&Path` | Agent profile 目录的绝对路径 |
| `budget_remaining` | `u64` | 高优先级资源消耗后剩余的 Token 预算 |
| `budget_total` | `u64` | 本轮的总 Token 预算 |
| `delivery_context` | `&str` | Thread 消息/收件箱条目的字符串形式 |
| `first_turn` | `bool` | 是否为该作用域的第一轮 |

所有字段都是零拷贝引用 — 不克隆大数据。

### ResourceProvider Trait

独立的数据访问层。`ResourceProvider` 读取外部资源
（文件、数据库、API）；`ContextResource` 将它们格式化为提示词段落。
一个 `ResourceProvider` 可以支撑多个 `ContextResource`。

```rust
pub trait ResourceProvider: Send + Sync {
    fn scheme(&self) -> &str;
    fn list(&self, scope: &ScopeRef, profile_dir: &Path) -> Result<Vec<ResourceHandle>>;
    fn read(&self, uri: &str, scope: &ScopeRef, profile_dir: &Path) -> Result<ResourceContent>;
}
```

### ContextResourceRegistry

持有已注册 `ContextResource` 链，按优先级升序排序。
`assemble_chain()` 方法遍历所有资源，检查作用域有效性，调用
`assemble()`，并应用 Token 预算瀑布。

---

## 设计约束

以下约束是由 Trait 契约和运行时强制执行的建筑不变量：

| ID | 约束 | 原因 |
|---|---|---|
| **C-1** | Loom 永不自行生成摘要内容 — 只有 Provider 生成 | 防止 Loom 自身摘要逻辑引入偏差 |
| **C-3/C-6** | 资源不得执行语义压缩、关键词提取或内容截断 | 保持信息保真度；预算溢出 → 跳过，不截断 |
| **C-4** | D2 中 `agentcontext.json` 没有 `skills` 字段 | Skills 由独立机制处理 |
| **C-8** | Warm 摘要持久化是原子操作（写临时文件 + 重命名） | 崩溃安全；写入中途崩溃不会损坏已有摘要 |

---

## 数据流

### D2 路径（始终活跃）

```
compose_envelope_prompt()
  ├── context_layer = spec.context_layer.unwrap_or(default_agent_context_spec())
  └── compose_with_context_chain()
        ├── 1. 加载 + 合并 agentcontext.json（三层）
        ├── 2. 预渲染记忆（如果存在 MemorySpec）
        ├── 3. build_context_resource_chain() → 注册表
        ├── 4. 固定段落：profile_prompt_files、runtime_context
        ├── 5. registry.assemble_chain(ctx, budget_remaining)
        │     └── Token 预算瀑布（见下文）
        └── 6. user_message（始终最后）
```

> **D2 默认化（commit 3b391ba）**：`compose_envelope_prompt` 始终走 D2 chain。
> 当 `context_layer = None` 时，使用 `default_agent_context_spec()` 提供的默认
> chain（memory + warm-summary + message-list）。D1 硬编码路径已删除。

### Token 预算瀑布

当 `assemble_chain()` 运行时，每个资源的段落会根据剩余预算进行评估：

```
for resource in resources（按优先级升序排列）:
    if resource.effective_scope 不包含 ctx.scope.kind:
        跳过

    sections = resource.assemble(ctx)

    for section in sections:
        section_tokens = estimate_tokens(section.content)

        if resource.priority != 0 AND section_tokens > budget_remaining:
            跳过该段落（不截断 — C-3/C-6）
        else:
            budget_remaining -= section_tokens
            包含该段落
```

**关键行为**：资源永远不会被部分包含。如果一个段落会超出剩余预算，
整个段落会被跳过。这保持了信息保真度 — 截断的文件或记忆条目可能
产生误导。

---

## 作用域继承

ContextLayer 配置在三个层级间合并：

```
Agent 级（spec.json context_layer）
    ↓ 合并
Profile 级（<profile_dir>/agentcontext.json）
    ↓ 合并
工作区级（<profile_dir>/workspace/agentcontext.json）
    ↓
本轮使用的有效配置
```

**合并规则**：对于每个 `scheme`，最高层的声明优先。新 `scheme` 的资源
会被追加。`effective_scope` 使用指定了非空值的最高层。

---

## 内置 Provider

Loom 内置四个 Provider，通过工厂注册表（`builtin_resource_factories()` HashMap）
管理：

### 优先级排序

```
memory(5) → warm-summary(7) → message-list(10) → file(20)
```

### 1. MemoryResource（plugin-memory，优先级 = 5）

- **Scheme**：`"memory"`
- **作用域**：Thread + Channel
- **行为**：迭代 2 起以官方插件形态存在于独立 crate `plugin-memory`（仅依赖
  `context-layer-core` + `loom-proto`，无 agent-runtime 特权）。工厂捕获
  per-agent 的 `MemorySpec`，检索与渲染在 `assemble()` 内部执行（读取
  `ctx.turn_input` / `ctx.delivery_context`）。错误时 warn + 空输出降级，
  预算不足时整段跳过（skip-not-truncate）。
- **产出段落**：`bootstrap_memory`、`turn_memory`（仅在非空时）

### 2. WarmSummaryContextResource（优先级 = 7）

- **Scheme**：`"warm-summary"`
- **作用域**：Thread + Channel
- **行为**：读取持久化的 Warm 摘要文件（`{profile_dir}/summaries/{scope_id}.md`），
  格式化为 `PromptSection` 注入提示词。预算限制为剩余 Token 的 20%
  （`WARM_SUMMARY_BUDGET_FRACTION = 0.2`），超预算时跳过（不截断，C-3）。
  摘要内容由 Provider 生成，此资源只负责读取和注入。
- **关联函数**：`persist()`（原子写入：临时文件 + 重命名）、`clear()`（删除摘要文件）、
  `summary_path()`（路径计算）
- **产出段落**：`warm_summary`（仅在摘要存在且未超预算时）

> **D2 化变更**：Warm 摘要不再由 `agent_serve.rs` 硬编码注入，而是通过 D2 chain
> 自动组装。旧的 8 个硬编码函数已删除，`insert_warm_summary_section()` 等已移除。

### 3. MessageListProvider（优先级 = 10）

- **Scheme**：`"message-list"`
- **作用域**：Thread + Channel
- **行为**：将 `delivery_context` 字符串（Thread 消息、收件箱条目）
  包装为单个 `PromptSection`。这是 Agent 看到的对话历史。
- **产出段落**：`delivery_context`

### 4. FileSystemProvider（优先级 = 20）

- **Scheme**：`"file"`
- **作用域**：Thread + Channel
- **行为**：从配置的挂载路径（相对于工作区或绝对路径）列出和读取文件。
  强制执行每文件 128KB 读取限制和路径遍历保护（canonicalize 检查）。
  支持 `${workspace.dir}` 模板展开。
- **配置**：`path`（挂载目录）、`max_files`（默认 10）
- **产出段落**：每个文件一个 `file_resource` 段落，格式为
  `--- <文件名> ---\n<内容>`

### 工厂注册表机制

内置 Provider 通过 `builtin_resource_factories()` HashMap 注册（替代旧的
`match` 分支）。新增 Provider 只需在工厂注册表中添加一行：

```rust
// agent_serve.rs — builtin_resource_factories()
factories.insert("my-scheme".into(), Box::new(|config| {
    Box::new(MyProvider::new(config)) as Box<dyn ContextResource>
}));
```

`build_context_resource_chain()` 遍历 `agentcontext.json` 声明的 resources，
通过 scheme 名称从工厂注册表查找对应的工厂闭包，生成 ContextResource 实例。

---

## Plugin 挂载架构（Phase 3）

Phase 3 引入了 `context-layer-core` crate 和 inventory 自注册机制，
实现了 loom 与 plugin 的完全解耦。

### context-layer-core crate

`crates/context-layer-core/` 是 loom 与 plugin 之间的**唯一耦合点**。
它定义了：

- `ContextResource` trait — 可插拔资源的 AOP 接口
- `AssemblyContext` — 传递给 `assemble()` 的只读上下文
- `PromptSection` — 命名的提示词段落
- `ContextResourcePlugin` — inventory 自注册类型
- `estimate_tokens()` — Token 估算函数

loom core 和 plugin crate 都依赖此 crate，但 loom 不知道 plugin 的具体类型。

### inventory 自注册机制

Plugin crate（如 `loom-plugin-context-tier`）通过 `inventory::submit!`
在编译时自注册：

```rust
// loom-plugin-context-tier/src/lib.rs
inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "warm-summary",
        factory: || Box::new(WarmSummaryContextResource::new()),
    }
}

inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "message-list",
        factory: || Box::new(MessageListProvider::new()),
    }
}
```

Loom 运行时通过 `discover_plugins()` 发现所有已注册的 plugin：

```rust
// crates/agent-runtime/src/context_layer.rs
pub fn discover_plugins() -> HashMap<String, fn() -> Box<dyn ContextResource>> {
    let mut map = HashMap::new();
    for plugin in inventory::iter::<ContextResourcePlugin> {
        map.insert(plugin.scheme.to_string(), plugin.factory);
    }
    map
}
```

为确保 plugin crate 的 inventory 注册不被链接器剥离，
`agent-runtime` 使用 `extern crate loom_plugin_context_tier;` 强制链接。

### Plugin 命名规范

格式：`loom-plugin-<layer>-<feature>`

| 维度 | 示例 |
|---|---|
| 仓库名 | `loom-plugin-context-tier` |
| Cargo crate | `loom-plugin-context-tier` |
| Rust 标识符 | `loom_plugin_context_tier` |
| plugin.json id | `loom-plugin-context-tier` |
| plugin.json layer | `context` |

### build.rs 双路径处理

`crates/cli/build.rs` 根据 plugin.json 存在与否区分两种路径：

| 条件 | 路径 | 行为 |
|---|---|---|
| 有 plugin.json | Full plugin 路径 | 解析 scope/priority/resources，支持多维调度 |
| 无 plugin.json | Pure skill 路径 | 扫描 skills/ 目录，所有 skill 为 global scope（向后兼容） |

---

## Warm 摘要与 Session Reset

ContextLayer 与 Warm/Cold Session Reset 机制集成：

### Session Reset 触发条件

当满足以下 **任一** 条件时触发 Session Reset：

- Token 使用量 > 预算 × 0.8（`SESSION_RESET_TOKEN_THRESHOLD`）
- 消息数量 > 50（`SESSION_RESET_MESSAGE_THRESHOLD`）

**保护条件**：Reset 不会在第一轮触发，也不会在消息数量 <
20（`SESSION_RESET_MIN_MESSAGES`）时触发。

### Reset 流程

1. 运行时拦截触发器，发送摘要生成提示词
2. Provider 生成 5 段结构化摘要：
   - `SESSION INTENT` — 主要目标/目的
   - `KEY DECISIONS` — 设计决策、审批结果、已解决的问题
   - `DELIVERABLES` — 产出物引用、文件路径、文档链接
   - `TASK STATE` — 当前状态和剩余工作
   - `ACTOR CONTEXT` — 每个 Actor 的最新立场/贡献
3. 摘要以原子方式持久化到 `{profile_dir}/summaries/{scope_id}.md`
4. Adapter Session 被重置（对话历史被丢弃）
5. 原始触发器重新排队，摘要作为 Warm 上下文注入

### Warm 摘要预算

Warm 摘要段落最多可占用唤醒上下文 Token 预算的 **20%**
（`WARM_SUMMARY_BUDGET_FRACTION = 0.2`）。如果持久化的摘要超出此分配，
将被 **跳过**（不截断 — C-3）。

---

## D2 默认化（D1 已弃用）

**commit 3b391ba** 起，D2 ContextResource chain 是唯一活跃路径：

- `context_layer = None` → 使用 `default_agent_context_spec()` 默认 chain
  （memory(5) + warm-summary(7) + message-list(10)）
- `context_layer = Some` → 使用声明的 chain 配置
- D1 硬编码路径（`build_envelope` + 手动段落组装）已完全删除（-78 行）
- 所有 actor 默认获得 Warm 摘要注入，无需手动配置

### default_agent_context_spec()

```rust
// crates/proto/src/methods.rs L3403
pub fn default_agent_context_spec() -> AgentContextSpec {
    AgentContextSpec {
        version: 1,
        effective_scope: vec![],
        resources: vec![
            ContextResourceSpec { scheme: "memory", priority: 5, ... },
            ContextResourceSpec { scheme: "warm-summary", priority: 7, ... },
            ContextResourceSpec { scheme: "message-list", priority: 10, ... },
        ],
    }
}
```

这意味着 Agent 无需任何配置即可获得完整的 D2 chain 体验，包括 Warm 摘要注入。

---

## Skill 安装投影

Skill（技能）在 Loom 中通过 `bundle.skills` 安装投影到达 Provider：

```
bundle.skills → ensure_scope → ensure_workspace_skill_targets
  → 投影到 workspace 的 .claude/skills/, .agents/skills/ 等目录
  → Provider (Claude/GPT) 原生读取这些目录作为 skill 指令
  → 不经过 ContextResource chain，不消耗 Loom token budget
```

`bundle.skills` 负责 Skill 的安装和 workspace 文件投影，消费者是 Provider
自身的 skill 系统。

> ⚠️ **未实现说明**：`scheme="skill"` 的 `SkillContextResource`（将 skill 指令
> 注入 Loom 提示词信封）在设计文档中描述过，但**当前代码中未实现**。
> `AgentContextSpec` 不含 `skills` 字段。

详见 [Skill 挂载指南](./skill-mounting-guide.md)。

---

## 参见

- [快速入门](./getting-started.md) — 快速开始指南
- [配置参考](./configuration.md) — 完整 schema
- [API 参考](./api-reference.md) — Trait 方法签名
- [源码导航](./source-navigation.md) — 关键文件位置
- [MessageList Warm/Cold](./message-list-warm-cold.md) — 发布说明
- [Skill 挂载指南](./skill-mounting-guide.md) — Skill 最佳实践
- [数据源挂载最佳实践](./data-source-best-practices.md) — 数据源配置
- [自定义 Provider 指南](./custom-provider-guide.md) — 开发扩展