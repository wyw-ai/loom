# ContextLayer — API 参考

> ContextLayer 系统的 Trait 定义、方法签名和使用示例。
> 配置语法参见 [配置参考](./configuration.md)。

---

## 默认 ContextResource Chain

```rust
// crates/proto/src/methods.rs:3403
pub fn default_agent_context_spec() -> AgentContextSpec
```

返回默认 chain 配置，当 `AgentSpec.context_layer` 为 `None` 时自动使用：

| 资源 | scheme | mount | priority |
|---|---|---|---|
| Agent 记忆 | `memory` | `agent-memory` | 5 |
| 会话摘要 | `warm-summary` | `warm` | 7 |
| 消息历史 | `message-list` | `delivery` | 10 |

所有 Agent 默认获得此 chain，无需在 `spec.json` 中配置 `context_layer`。

---

## ContextResource Trait

所有上下文资源实现的核心 Trait。

```rust
pub trait ContextResource: Send + Sync {
    fn scheme(&self) -> &str;
    fn priority(&self) -> i32;
    fn effective_scope(&self) -> &[ScopeKind];
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>>;
}
```

### 方法

#### `scheme(&self) -> &str`

返回此资源处理的 URI scheme。注册表使用它将 `agentcontext.json` 声明
路由到正确的 Provider。

**发行版可用 scheme**：`"memory"`、`"message-list"`、`"file"`、`"warm-summary"`、`"skill"`（前三者来自官方插件，`file` 为宿主内置资源; `skill` 为设计中形态）

#### `priority(&self) -> i32`

返回组装优先级。值越小 = 越先组装（重要性越高）。超出预算的资源按
优先级逆序跳过。优先级 `0` = 永不跳过（保留给关键资源）。

**默认优先级**（插件由 `plugin.json` 清单声明，宿主内置资源由工厂表定义）：

| Provider | 优先级 |
|---|---|
| MemoryResource（plugin-memory，清单声明） | 5 |
| WarmSummaryContextResource（清单声明） | 7 |
| MessageListProvider（清单声明） | 10 |
| FileSystemProvider（宿主内置） | 20 |

#### `effective_scope(&self) -> &[ScopeKind]`

返回此资源在哪些作用域类型中生效。仅 Channel 的资源在 Thread 作用域中
会被跳过，反之亦然。

```rust
pub enum ScopeKind {
    Thread,
    Channel,
}
```

#### `assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>>`

生成此资源对提示词的贡献。返回零个或多个 `PromptSection`。

**约束**（由 Trait 契约强制执行）：
- 不得执行语义压缩、关键词提取或内容截断（C-3、C-6）
- 可以读取已持久化的数据、格式化为段落、或在预算不足时跳过自身
- 应在组装大量内容前检查 `ctx.budget_remaining`

---

## AssemblyContext

传递给 `assemble()` 的只读上下文。

```rust
pub struct AssemblyContext<'a> {
    pub scope: &'a ScopeRef,
    pub channel_id: Option<&'a str>,
    pub actor_id: &'a str,
    pub profile_dir: &'a Path,
    pub budget_remaining: u64,
    pub budget_total: u64,
    pub delivery_context: &'a str,
    pub turn_input: &'a str,
    pub first_turn: bool,
}
```

### 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `scope` | `&ScopeRef` | 当前轮运行的作用域（Thread 或 Channel） |
| `channel_id` | `Option<&'a str>` | Channel ID（如果可用） |
| `actor_id` | `&str` | Agent 的 actor ID |
| `profile_dir` | `&Path` | Agent profile 目录的绝对路径 |
| `budget_remaining` | `u64` | 高优先级资源消耗后剩余的 Token 预算 |
| `budget_total` | `u64` | 本轮的总 Token 预算 |
| `delivery_context` | `&str` | Thread 消息/收件箱条目的字符串形式 |
| `turn_input` | `&str` | 当前回合的用户输入（迭代 2 新增）。环境数据通道——官方 memory 插件用它做记忆检索，第三方替身资源读取同一字段（参见[第三方资源扩展指南](./third-party-resource-guide.md)） |
| `first_turn` | `bool` | 是否为该作用域的第一轮 |

所有字段都是零拷贝引用（`&'a`）。生命周期绑定到轮的作用域。

---

## ResourceProvider Trait

外部资源的数据访问层。与 `ContextResource`（提示词组装层）分离。

```rust
pub trait ResourceProvider: Send + Sync {
    fn scheme(&self) -> &str;
    fn list(&self, scope: &ScopeRef, profile_dir: &Path) -> Result<Vec<ResourceHandle>>;
    fn read(&self, uri: &str, scope: &ScopeRef, profile_dir: &Path) -> Result<ResourceContent>;
}
```

### ResourceHandle

```rust
pub struct ResourceHandle {
    pub uri: String,
    pub name: String,
    pub size_bytes: u64,
}
```

### ResourceContent

```rust
pub struct ResourceContent {
    pub uri: String,
    pub media_type: String,
    pub text: String,
}
```

---

## ContextResourceRegistry

持有已注册资源链并执行组装。

```rust
pub struct ContextResourceRegistry {
    resources: Vec<Box<dyn ContextResource>>,
}
```

### 方法

#### `new() -> Self`

创建空注册表。

#### `register(&mut self, resource: Box<dyn ContextResource>)`

注册资源。按优先级顺序插入（优先级值越小 = 越先组装）。

#### `is_empty(&self) -> bool`

注册表中没有资源时返回 true。

#### `has_scheme(&self, scheme: &str) -> bool`

任何已注册资源使用了给定 scheme 时返回 true。

#### `assemble_chain(&self, ctx: &AssemblyContext<'_>, budget_remaining: u64) -> (Vec<PromptSection>, u64)`

按优先级顺序组装所有资源，应用 Token 预算瀑布。返回已组装段落和剩余预算。

**算法**：
1. 按优先级升序遍历资源
2. 跳过 `effective_scope` 不包含 `ctx.scope.kind` 的资源
3. 调用 `resource.assemble(ctx)` — 出错时记录日志并跳过
4. 对每个返回的段落：
   - 估算 Token 数量
   - 如果 `priority != 0` 且 `section_tokens > budget_remaining`：**跳过**（不截断）
   - 否则：从预算中减去，包含该段落
5. 返回 `(段落列表, 剩余预算)`

---

## PromptSection

上下文组装的输出单元。

```rust
pub struct PromptSection {
    pub name: &'static str,
    pub content: String,
    pub source: SectionSource,
}
```

| 字段 | 说明 |
|---|---|
| `name` | 段落标识符（如 `"bootstrap_memory"`、`"delivery_context"`、`"warm_summary"`、`"user_message"`） |
| `content` | 注入到提示词中的文本内容 |
| `source` | 溯源标记（迭代 1 起，AC-R1-2）。声明该段落从哪个持久来源产生，用于调试与审计 |

段落最终以 `"\n\n"` 分隔符拼接。

### SectionSource

```rust
pub enum SectionSource {
    Resource { scheme: &'static str, uri: Option<String> },
    Runtime { origin: &'static str },
    Exempted { reason: &'static str },
}
```

| 变体 | 说明 |
|---|---|
| `Resource` | 由注册在该 scheme 下的 ContextResource 产出。`uri` 为单一持久来源定位符（如 `"summaries/{scope_id}.md"`）；`None` 表示聚合/投影内容无单一 uri（豁免记录于 ARCH 设计 §1.3） |
| `Runtime` | 由运行时在资源链外组装（固定段落与旁路路径，如 summary 生成提示词） |
| `Exempted` | 显式豁免溯源；`reason` 记录原因。保留给自起源输入（用户回合输入）与仅为编译保留的遗留路径 |

便捷构造: `PromptSection::from_resource(name, scheme, content)`（uri=None）与 `from_resource_uri(name, scheme, uri, content)`（uri=Some）。

---

## 内置 Provider API

### MemoryResource（plugin-memory）

迭代 2 起，memory 以官方插件形态存在于独立 crate `plugin-memory`（无特权依赖: 仅 `context-layer-core` + `proto`，编译期禁止依赖 `agent-runtime`）; 迭代 3 R1 整改起完全规范化——`inventory::submit!` 注册 + `plugin.json` v2 清单（`official`，v1.0.0）。compose 路径的预渲染特判已删除，`MemoryResource` 在链装配阶段自行检索 + 渲染。详见[Memory 插件指南](./memory-plugin-guide.md)。

```rust
pub struct MemoryResource { /* 捕获的 Option<MemorySpec> */ }

impl MemoryResource {
    pub fn new(spec: Option<MemorySpec>) -> Self;
}

pub fn open_memory_store(profile_dir: &Path, spec: &MemorySpec) -> JsonlMemoryStore;
pub fn open_memory_store_dyn(profile_dir: &Path, spec: &MemorySpec) -> Arc<dyn MemoryStore>;
```

- `new(spec)` — 捕获 per-agent 的 `MemorySpec`（`None` = 未配置，assemble 返回空）。注册经 `inventory::submit!`（`memory_resource_factory` 从 config envelope 的 `memory` key 构造; actor spec 由宿主注入 envelope——见 [Memory 插件指南 · 注册路径](./memory-plugin-guide.md#注册路径r1-规范化)）
- `open_memory_store` / `open_memory_store_dyn` — 打开 JSONL 存储（相对路径基于 `profile_dir`），公开供 MCP bridge 复用
- 重导出 shim: `agent_runtime::memory::*` import 路径不变；新代码建议直接依赖 `plugin-memory`

**Scheme**：`"memory"` | **优先级**：5 | **作用域**：Thread + Channel | **段落**：`bootstrap_memory` / `turn_memory`

行为语义: `MemorySpec` 缺失或 `delivery.prompt=false` → 不贡献段落; 检索/渲染错误 → `tracing::warn` + 空输出降级（回合不中断）; 预算不足 → 整段跳过（skip-not-truncate）。检索输入为 `ctx.turn_input` + `ctx.delivery_context`。

### 第三方替身资源（replacement contract）

第三方 crate 经 `inventory::submit!` 注册 `ContextResourcePlugin { scheme, factory }`（工厂接收 config envelope），读取与官方插件相同的环境数据通道（`ctx.turn_input` / `ctx.delivery_context`），参与同一 priority 排序与预算瀑布。完整范例参见[第三方资源扩展指南](./third-party-resource-guide.md)。

### WarmSummaryContextResource

```rust
pub struct WarmSummaryContextResource {
    effective_scopes: Vec<ScopeKind>,
}

impl WarmSummaryContextResource {
    pub fn new() -> Self;
    pub fn summary_path(profile_dir: &Path, scope_id: &str) -> PathBuf;
    pub fn persist(profile_dir: &Path, scope_id: &str, summary: &str) -> Result<()>;
    pub fn clear(profile_dir: &Path, scope_id: &str);
}
```

- `new()` — 创建实例，默认作用域 `[Thread, Channel]`
- `summary_path()` — 计算摘要文件路径：`{profile_dir}/summaries/{scope_id}.md`
- `persist()` — 原子写入摘要（临时文件 + 重命名，防崩溃损坏）
- `clear()` — 删除摘要文件（如 scope 被删除时调用）

**Scheme**：`"warm-summary"` | **优先级**：7 | **作用域**：Thread + Channel

`assemble()` 行为：读取持久化摘要 → 计算预算（`budget_remaining * 0.2`）→
超预算则跳过（不截断，C-3）→ 格式化为 `warm_summary` 段落。

### MessageListProvider

```rust
pub struct MessageListProvider { ... }

impl MessageListProvider {
    pub fn new() -> Self;
}
```

**Scheme**：`"message-list"` | **优先级**：10 | **作用域**：Thread + Channel

将 `AssemblyContext` 的 `delivery_context` 包装为单个段落。

### FileSystemProvider

```rust
pub struct FileSystemProvider { ... }

impl FileSystemProvider {
    pub fn new(mount_path: impl Into<String>, max_files: usize) -> Self;
}
```

**Scheme**：`"file"` | **优先级**：20（通过 `FileContextResource` 适配器） | **作用域**：Thread + Channel

- `mount_path` — 读取文件的目录（支持 `${workspace.dir}`）
- `max_files` — 枚举的最大文件数（默认 10）
- 每文件限制：128KB

### scheme="skill"（未实现）

> ⚠️ `SkillContextResource` 在设计文档中规划过，但**当前代码中未实现**。
> 以下为设计参考，实际不可用。

设计中的 Scheme：`"skill"` | 设计优先级：8 | 设计作用域：Thread + Channel

设计中的行为：扫描 `workspace/skills/*/` 目录，读取每个 skill 子目录中的
指令文件，格式化为 `PromptSection`。

**当前实际路径**：Skill 通过 `bundle.skills` 安装投影到达 Provider，
不经过 ContextResource chain。详见 [Skill 挂载指南](./skill-mounting-guide.md)。

---

## 自定义 Provider 示例

### 方式一：inventory 自注册（推荐 — Phase 3）

Phase 3 引入了 `context-layer-core` crate 和 inventory 自注册机制。
自定义 Provider 通过 `inventory::submit!` 在编译时自注册，loom 运行时
通过 `discover_plugins()` 自动发现，**无需修改 loom 源码**。

```rust
// 在你的 plugin crate 中 (如 loom-plugin-my-feature/src/lib.rs)
use context_layer_core::{AssemblyContext, ContextResource, ContextResourcePlugin, PromptSection};
use anyhow::Result;
use proto::types::ScopeKind;

pub struct MyCustomResource {
    effective_scopes: Vec<ScopeKind>,
}

impl ContextResource for MyCustomResource {
    fn scheme(&self) -> &str { "my-custom" }
    fn priority(&self) -> i32 { 15 }
    fn effective_scope(&self) -> &[ScopeKind] { &self.effective_scopes }
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        // 仅读取已持久化的数据 — 不做语义压缩 (C-3/C-6)
        Ok(vec![])
    }
}

// 编译时自注册 — loom 通过 discover_plugins() 自动发现
inventory::submit! {
    ContextResourcePlugin {
        scheme: "my-custom",
        factory: || Box::new(MyCustomResource::new()),
    }
}
```

### 方式二：工厂注册表（内置 Provider）

对于 loom 内置 Provider，在 `agent_serve.rs` 的 `builtin_resource_factories()`
工厂注册表中添加一行：

```rust
factories.insert("my-scheme".into(), Box::new(|_config| {
    Box::new(MyCustomResource::new()) as Box<dyn ContextResource>
}));
```

### ContextResourcePlugin（inventory 注册类型）

```rust
// crates/context-layer-core/src/lib.rs
pub struct ContextResourcePlugin {
    pub scheme: &'static str,
    pub factory: fn() -> Box<dyn ContextResource>,
}

// 启用 inventory 收集
inventory::collect!(ContextResourcePlugin);
```

### discover_plugins()

```rust
// crates/agent-runtime/src/context_layer.rs:191
pub fn discover_plugins() -> HashMap<String, fn() -> Box<dyn ContextResource>> {
    let mut map = HashMap::new();
    for plugin in inventory::iter::<ContextResourcePlugin> {
        map.insert(plugin.scheme.to_string(), plugin.factory);
    }
    map
}
```

Loom 通过 `extern crate loom_plugin_context_tier;` 强制链接 plugin crate，
确保 inventory 注册不被链接器剥离。

### ContextResourceBuilder（Builder DSL）

对于不需要自定义 struct 的简单场景，可使用 `ContextResourceBuilder` 通过
闭包创建 ContextResource，无需实现 trait：

```rust
use agent_runtime::ContextResourceBuilder;

let resource = ContextResourceBuilder::new("my-scheme")
    .priority(15)
    .assemble(|ctx| {
        Ok(vec![PromptSection {
            name: "my_section".into(),
            content: "Hello world".into(),
        }])
    })
    .build();
```

**Builder API**：

| 方法 | 默认值 | 说明 |
|---|---|---|
| `new(scheme)` | — | 创建 Builder，默认 priority=15, scopes=[Thread, Channel] |
| `.priority(n)` | 15 | 设置优先级 |
| `.scopes(vec)` | [Thread, Channel] | 设置生效作用域 |
| `.assemble(closure)` | — | 提供 assemble 闭包（必须调用） |
| `.build()` | — | 返回 `Box<dyn ContextResource>` |

内部通过 `ClosureContextResource` 适配器实现 `ContextResource` trait。

---

## FileContextResource 适配器

运行时使用 `FileContextResource` 作为适配器，将
`FileSystemProvider`（`ResourceProvider`）包装为 `ContextResource`：

```rust
struct FileContextResource {
    provider: FileSystemProvider,
    effective_scopes: Vec<ScopeKind>,
}

impl ContextResource for FileContextResource {
    fn scheme(&self) -> &str { "file" }
    fn priority(&self) -> i32 { 20 }
    fn effective_scope(&self) -> &[ScopeKind] { &self.effective_scopes }
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        let handles = self.provider.list(ctx.scope, ctx.profile_dir)?;
        // ... 读取每个文件并生成 PromptSection
    }
}
```

此模式（ResourceProvider → ContextResource 适配器）可复用于其他数据源
（数据库、API 等）。