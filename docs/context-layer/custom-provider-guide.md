# 自定义 ContextResource 开发指南

> 本文档说明如何实现自定义 ContextResource 和 ResourceProvider，
> 从 Trait 实现到注册到 ContextLayer 链。
> 前置知识：[架构 Wiki](./architecture.md)、[API 参考](./api-reference.md)。

---

## 背景

ContextLayer 通过 `ContextResource` Trait 提供可插拔的扩展机制。当内置
Provider（Memory、MessageList、FileSystem、Skill）无法满足需求时，
可以开发自定义 Provider 来接入新的数据源（数据库、API、Obsidian vault 等）。

本文档覆盖：
- ContextResource Trait 实现步骤
- ResourceProvider Trait 实现步骤（可选，用于数据访问层分离）
- 注册到 `builtin_resource_factories()`
- 预算瀑布参与说明
- 安全约束清单

---

## 架构分层

在开始实现前，理解 ContextLayer 的两层架构：

```
ContextResource Trait（提示词组装层）
  ├── assemble() → 返回 Vec<PromptSection>
  ├── 参与 budget waterfall
  └── 可选包装 ResourceProvider
        │
ResourceProvider Trait（数据访问层）
  ├── list() → 枚举资源
  └── read() → 读取单个资源
```

**设计原则**：
- `ResourceProvider` 是数据层（读取外部资源）
- `ContextResource` 是组装层（格式化为提示词段落）
- 一个 `ResourceProvider` 可被多个 `ContextResource` 包装
- 简单场景可只实现 `ContextResource`（跳过 `ResourceProvider`）

---

## 步骤 1：实现 ContextResource Trait

### Trait 定义

```rust
pub trait ContextResource: Send + Sync {
    fn scheme(&self) -> &str;
    fn priority(&self) -> i32;
    fn effective_scope(&self) -> &[ScopeKind];
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>>;
}
```

### 实现示例：自定义 Markdown 文件资源

```rust
use agent_runtime::context_layer::{AssemblyContext, ContextResource};
use agent_runtime::envelope::PromptSection;
use anyhow::Result;
use proto::types::{ScopeKind, ScopeRef};
use std::path::PathBuf;

pub struct MarkdownContextResource {
    effective_scopes: Vec<ScopeKind>,
    mount_path: String,
    max_files: usize,
}

impl MarkdownContextResource {
    pub fn new(mount_path: impl Into<String>, max_files: usize) -> Self {
        Self {
            effective_scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
            mount_path: mount_path.into(),
            max_files,
        }
    }
}

impl ContextResource for MarkdownContextResource {
    fn scheme(&self) -> &str {
        "markdown"
    }

    fn priority(&self) -> i32 {
        15  // 在 message-list (10) 和 file (20) 之间
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.effective_scopes
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        // 1. 解析挂载路径（支持 ${workspace.dir} 模板）
        let resolved_path = self.mount_path.replace(
            "${workspace.dir}",
            &ctx.profile_dir.join("workspace").to_string_lossy()
        );
        let base_path = PathBuf::from(&resolved_path);

        // 2. 检查目录是否存在
        if !base_path.exists() || !base_path.is_dir() {
            return Ok(vec![]);  // 无数据时不贡献内容
        }

        // 3. 枚举 .md 文件（限制数量）
        let mut entries: Vec<_> = std::fs::read_dir(&base_path)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
            .take(self.max_files)
            .collect();

        // 4. 读取每个文件并生成 PromptSection
        let mut sections = Vec::new();
        for entry in &entries {
            let content = std::fs::read_to_string(entry.path())
                .unwrap_or_default();

            // 安全检查：不截断内容（C-3/C-6）
            if !content.trim().is_empty() {
                sections.push(PromptSection {
                    name: format!("markdown:{}", entry.path().file_name().unwrap().to_string_lossy()),
                    content,
                });
            }
        }

        Ok(sections)
    }
}
```

### 实现要点

#### `scheme()` — 唯一标识

返回唯一的 URI scheme（如 `"markdown"`、`"obsidian"`、`"sqlite"`）。
用于 `agentcontext.json` 声明和 registry 路由。

#### `priority()` — 优先级分配

| 值 | 含义 | 适用场景 |
|---|---|---|
| `0` | 永不跳过 | 关键资源（Warm 摘要） |
| `1-9` | 高优先 | Agent 身份核心（Memory=5, Skill=8） |
| `10-19` | 标准 | 任务理解（MessageList=10） |
| `20+` | 低优先 | 参考性数据（File=20） |

#### `effective_scope()` — 作用域过滤

声明此资源在哪些作用域类型中生效：

```rust
// Thread + Channel 都生效
fn effective_scope(&self) -> &[ScopeKind] {
    &self.effective_scopes
}

// 仅 Thread 生效
fn effective_scope(&self) -> &[ScopeKind] {
    &[ScopeKind::Thread]
}
```

#### `assemble()` — 核心方法

**必须遵守的约束**：
- 从 `AssemblyContext` 获取 scope/channel_id/profile_dir/budget_remaining
- **读取已持久化数据**（禁止运行时生成/压缩/截断，C-3/C-6）
- 返回 `Vec<PromptSection>`
- 如果 budget 不足或无数据，返回空 `Vec`（自行跳过）

**AssemblyContext 可用字段**：

| 字段 | 用途 |
|---|---|
| `scope` | 当前作用域（Thread/Channel） |
| `channel_id` | Channel ID |
| `actor_id` | Agent 的 actor ID |
| `profile_dir` | Agent profile 目录路径 |
| `budget_remaining` | 剩余 Token 预算 |
| `budget_total` | 总 Token 预算 |
| `delivery_context` | 投递上下文字符串 |
| `first_turn` | 是否为首轮 |

---

## 步骤 2（可选）：实现 ResourceProvider Trait

当需要分离数据访问层时，实现 `ResourceProvider`：

```rust
use agent_runtime::context_layer::{ResourceProvider, ResourceHandle, ResourceContent};
use anyhow::Result;
use proto::types::{ScopeRef};
use std::path::Path;

pub struct MarkdownProvider {
    mount_path: String,
    max_files: usize,
}

impl MarkdownProvider {
    pub fn new(mount_path: impl Into<String>, max_files: usize) -> Self {
        Self {
            mount_path: mount_path.into(),
            max_files,
        }
    }
}

impl ResourceProvider for MarkdownProvider {
    fn scheme(&self) -> &str {
        "markdown"
    }

    fn list(&self, _scope: &ScopeRef, profile_dir: &Path) -> Result<Vec<ResourceHandle>> {
        let base = profile_dir.join(&self.mount_path);
        let mut handles = Vec::new();

        if base.exists() {
            for entry in std::fs::read_dir(&base)?.take(self.max_files) {
                if let Ok(e) = entry {
                    if e.path().extension().map_or(false, |ext| ext == "md") {
                        let name = e.file_name().to_string_lossy().to_string();
                        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                        handles.push(ResourceHandle {
                            uri: format!("markdown:///{}", name),
                            name,
                            size_bytes: size,
                        });
                    }
                }
            }
        }

        Ok(handles)
    }

    fn read(&self, uri: &str, _scope: &ScopeRef, profile_dir: &Path) -> Result<ResourceContent> {
        let name = uri.strip_prefix("markdown:///").unwrap_or(uri);
        let path = profile_dir.join(&self.mount_path).join(name);

        // 路径遍历防护
        let canonical = path.canonicalize()?;
        let base = profile_dir.join(&self.mount_path).canonicalize()?;
        if !canonical.starts_with(&base) {
            return Err(anyhow!("path traversal detected"));
        }

        let content = std::fs::read_to_string(&canonical)?;

        Ok(ResourceContent {
            uri: uri.to_string(),
            media_type: "text/markdown".to_string(),
            text: content,
        })
    }
}
```

### ResourceProvider vs ContextResource

| 维度 | ResourceProvider | ContextResource |
|---|---|---|
| 职责 | 数据访问（list/read） | 提示词组装（assemble） |
| 输出 | `ResourceHandle` / `ResourceContent` | `PromptSection` |
| 预算参与 | 不直接参与 | 参与 budget waterfall |
| 关系 | 可被多个 ContextResource 包装 | 直接注入提示词 |

参考实现：`FileContextResource`（agent_serve.rs:8756-8807）包装 `FileSystemProvider`。

---

## 步骤 3：注册方式

Phase 3 提供两种注册方式：

### 方式 A：inventory 自注册（推荐 — 外部 Plugin）

外部 Plugin crate 通过 `inventory::submit!` 在编译时自注册。
**无需修改 loom 源码**，loom 运行时通过 `discover_plugins()` 自动发现。

```rust
// 在你的 plugin crate 中 (如 loom-plugin-my-feature/src/lib.rs)
use context_layer_core::ContextResourcePlugin;

inventory::submit! {
    ContextResourcePlugin {
        scheme: "my-custom",
        factory: || Box::new(MyCustomResource::new()),
    }
}
```

loom 通过 `extern crate loom_plugin_my_feature;` 强制链接，
确保 inventory 注册不被链接器剥离。

### 方式 B：工厂注册表（内置 Provider）

对于 loom 内置 Provider，在 `agent_serve.rs` 的 `builtin_resource_factories()`
工厂注册表中添加一行：

```rust
// agent_serve.rs — builtin_resource_factories()
fn builtin_resource_factories(
    bootstrap_memory: &str,
    turn_memory: String,
) -> std::collections::HashMap<String, ResourceFactory> {
    let mut factories = std::collections::HashMap::new();

    // 内置 scheme...
    factories.insert("memory".into(), Box::new(move |_config| {
        Box::new(MemoryProvider::new().with_rendered(/* ... */)) as Box<dyn ContextResource>
    }));

    // ↓↓↓ 新增你的自定义 scheme ↓↓↓
    factories.insert("markdown".into(), Box::new(|config| {
        let path = config.as_ref()
            .and_then(|c| c.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("${workspace.dir}/docs");
        Box::new(MarkdownContextResource::new(path)) as Box<dyn ContextResource>
    }));

    factories
}
```

`build_context_resource_chain()` 会自动遍历 `agentcontext.json` 声明的 resources，
通过 scheme 名称从工厂注册表或 discover_plugins() 查找对应闭包并生成实例。

### 注册要点

1. **外部 Plugin** 使用 `inventory::submit!`（方式 A），零改动 loom
2. **内置 Provider** 使用工厂注册表（方式 B），在 `builtin_resource_factories()` 添加一行
3. 工厂闭包签名为 `|config: Option<&Value>| -> Box<dyn ContextResource>`
4. 对未知 scheme 由 `build_context_resource_chain()` 自动记录警告日志（不 panic）

### 替代方案：ContextResourceBuilder

对于简单场景，可跳过自定义 struct，直接用 Builder DSL：

```rust
use agent_runtime::ContextResourceBuilder;

factories.insert("my-simple".into(), Box::new(|_config| {
    ContextResourceBuilder::new("my-simple")
        .priority(15)
        .assemble(|ctx| {
            let content = std::fs::read_to_string(
                ctx.profile_dir.join("my-context.md")
            ).unwrap_or_default();
            if content.is_empty() {
                Ok(vec![])
            } else {
                Ok(vec![PromptSection {
                    name: "my_context".into(),
                    content,
                }])
            }
        })
        .build()
}));
```

---

## 步骤 4：在 agentcontext.json 中使用

注册完成后，在 `agentcontext.json` 中声明自定义资源：

```json
{
  "version": 1,
  "resources": [
    { "scheme": "memory", "mount": "memory", "priority": 5 },
    { "scheme": "message-list", "mount": "messages", "priority": 10 },
    {
      "scheme": "markdown",
      "mount": "project-docs",
      "priority": 15,
      "config": {
        "path": "${workspace.dir}/docs",
        "max_files": 8
      }
    }
  ]
}
```

---

## 预算瀑布参与说明

自定义 Provider 自动参与 `assemble_chain()` 的预算瀑布：

```
assemble_chain(ctx, budget_remaining=10000)

1. MemoryProvider (priority=5)
   assemble() → 2 sections, 800 tokens
   budget_remaining: 10000 - 800 = 9200

2. MessageListProvider (priority=10)
   assemble() → 1 section, 3000 tokens
   budget_remaining: 9200 - 3000 = 6200

3. MarkdownContextResource (priority=15)  ← 你的自定义 Provider
   assemble() → 3 sections, 2000 tokens total
   budget_remaining: 6200 - 2000 = 4200

4. FileSystemProvider (priority=20)
   assemble() → 2 sections, 5000 tokens total
   5000 > 4200 (budget_remaining) AND priority != 0
   → 跳过（不截断）

最终输出: 所有已包含的段落
```

### 预算检查建议

在 `assemble()` 中，如果生成大量内容，建议先检查剩余预算：

```rust
fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
    // 粗略估算将要读取的内容大小
    let estimated_tokens = self.estimate_content_size(ctx)?;

    // 如果预估超出预算，可以主动返回空（避免无谓的 I/O）
    if estimated_tokens > ctx.budget_remaining && self.priority() != 0 {
        return Ok(vec![]);  // 主动跳过
    }

    // 正常读取和组装
    // ...
}
```

这不是必须的（`assemble_chain` 会处理跳过），但可以避免不必要的 I/O 操作。

---

## 安全约束清单

自定义 Provider 实现必须遵守以下安全约束：

### C-3/C-6：禁止语义操作

| 禁止 | 原因 |
|---|---|
| 语义压缩 | 引入信息偏差 |
| 关键词提取 | 丢失上下文 |
| 内容截断 | 截断的内容可能产生误导 |

**替代方案**：预算不足时返回空 `Vec`（跳过整个段落）。

### 路径遍历防护

所有读取文件的 Provider 必须实现 canonicalize 检查：

```rust
let canonical = path.canonicalize()?;
let base_canonical = base_path.canonicalize()?;
if !canonical.starts_with(&base_canonical) {
    return Err(anyhow!("path traversal detected"));
}
```

### MAX_READ_BYTES 限制

单文件读取不超过 128KB（`MAX_READ_BYTES = 131072`）：

```rust
const MAX_READ_BYTES: usize = 128 * 1024;

fn read_file_limited(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() as usize > MAX_READ_BYTES {
        tracing::warn!("file exceeds MAX_READ_BYTES, skipping: {}", path.display());
        return Ok(String::new());
    }
    Ok(std::fs::read_to_string(path)?)
}
```

### SQL 注入防护（如适用）

使用参数化查询，禁止字符串拼接：

```rust
// 正确 ✅
let result = conn.query(&query, params![scope_id])?;

// 错误 ❌
let query = format!("SELECT * FROM tasks WHERE id = '{}'", scope_id);
let result = conn.query(&query, [])?;
```

---

## 常见问题 FAQ

### Q: 自定义 Provider 需要实现 ResourceProvider 吗？

A: 不必须。`ResourceProvider` 是可选的数据访问层分离。简单场景可直接实现
`ContextResource`，在 `assemble()` 中直接读取数据。当数据源需要被多个
ContextResource 复用时，才需要分离 `ResourceProvider`。

### Q: 如何调试自定义 Provider？

A: 在 `assemble()` 中添加 `tracing::debug!` 日志，查看段落生成情况和预算消耗。
也可以在 `build_context_resource_chain()` 中打印注册信息。

### Q: 自定义 Provider 可以是 async 的吗？

A: 当前 `ContextResource` trait 是同步的。对于同步数据源（文件系统、SQLite），
同步实现足够。如果未来需要 async（如 HTTP API 调用），再考虑 async trait 扩展。
ARCH 判断：当前不需要 async — `rusqlite` 是同步的，文件 I/O 也是同步的。

### Q: 如何处理 Provider 中的错误？

A: `assemble()` 返回 `Result<Vec<PromptSection>>`。出错时可以：
- 返回 `Err` — `assemble_chain()` 会记录日志并跳过此资源
- 返回 `Ok(vec![])` — 静默跳过（无数据时推荐）
- 不要 panic — 会中断整个轮次