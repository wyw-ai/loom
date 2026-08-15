# 第三方上下文资源扩展指南

> 迭代 2 建立了**无特权接缝**: 任何第三方 crate 都能以与官方插件完全相同的契约接入 ContextResource 链——没有隐藏后门，没有 agent-runtime 特权依赖。本文以 `crates/cli/tests/context_replacement.rs` 的 `EchoInputResource` 为范例，讲解替身资源（replacement resource）的完整写法。
>
> 相关文档: [自定义 Provider 指南](./custom-provider-guide.md)（新增 scheme 的通用路径）| [Memory 插件指南](./memory-plugin-guide.md)（官方插件参考实现）| [plugin.json 清单指南](./plugin-guide.md)（清单声明格式与官方源数据化）

---

## 契约总览

第三方资源接入只需三件事:

| 要素 | 说明 |
|---|---|
| 实现 `ContextResource` | `context-layer-core` 公共 trait: `scheme` / `priority` / `assemble` |
| `inventory::submit!` 注册 | 提交 `ContextResourcePlugin { scheme, factory }`，工厂接收 config envelope |
| 自含业务配置 | 工厂从 config envelope（`resources[].config`）读取你的配置; per-agent 状态可由宿主或资源自身注入 |

依赖边界: 你的 crate 只需依赖 `context-layer-core`（+ 你需要的 proto 类型）。**编译期无法依赖 `agent-runtime`**——官方 `plugin-memory` 同样如此，这正是接缝无特权的实证。

## 范例逐段讲解

以下代码取自 `crates/cli/tests/context_replacement.rs`（AC-M2-2 集成测试）。

### 1. 定义资源结构体（自含配置）

```rust
struct EchoInputResource {
    /// 替身资源自己的配置。inventory 工厂是零参的，
    /// 所以第三方资源必须自己携带业务配置。
    label: &'static str,
}

impl EchoInputResource {
    fn new() -> Self {
        Self { label: "echo-config" }
    }
}
```

### 2. 实现 ContextResource

```rust
use context_layer_core::{
    AssemblyContext, ContextResource, PromptSection, SectionSource,
};

impl ContextResource for EchoInputResource {
    fn scheme(&self) -> &'static str { "echo-input" }

    fn priority(&self) -> i32 { 15 }   // 越小越先装配、越先扣预算

    fn effective_scope(&self) -> &[ScopeKind] {
        &[ScopeKind::Thread, ScopeKind::Channel]
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> anyhow::Result<Vec<PromptSection>> {
        // 三条数据通道: 自身配置 + 环境回合输入 + 投递上下文
        let content = format!(
            "[{}] turn={} delivery={}",
            self.label, ctx.turn_input, ctx.delivery_context
        );
        Ok(vec![PromptSection {
            name: "echo_input",
            content,
            source: SectionSource::Resource { scheme: "echo-input", uri: None },
        }])
    }
}
```

要点:

- **`ctx.turn_input` / `ctx.delivery_context`** 是迭代 2 新增的环境数据通道——官方 memory 插件检索记忆用的就是同两个字段，第三方与官方看到完全相同的数据面。
- **`PromptSection.source` 必填**: 每个段落都要声明溯源——资源链产出用 `SectionSource::Resource`，链外运行时组装用 `SectionSource::Runtime`，自起源输入（如用户回合输入）用 `SectionSource::Exempted`。这是迭代 1 起的强制契约（AC-R1-2）。
- **错误处理**: `assemble` 返回 `anyhow::Result`。官方插件的惯例是**降级不中断**——捕获错误后 `tracing::warn` 并返回空 `Vec`，让坏插件不能卡死整个回合。

### 3. inventory 注册

```rust
inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "echo-input",
        factory: || Box::new(EchoInputResource::new()),
    }
}
```

注册后 composer 通过 `discover_plugins()` 自动发现你的工厂——与 `loom-plugin-context-tier` 注册 warm-summary / message-list 的路径完全一致。

### 4. 同链装配验证

```rust
let plugins = discover_plugins();
let factory = *plugins.get("echo-input").unwrap();

let mut registry = ContextResourceRegistry::new();
registry.register(Box::new(plugin_memory::MemoryResource::new(Some(spec))));
registry.register(factory());

let (sections, _) = registry.assemble_chain(&ctx, 100_000);
```

验证了三件事:

1. **可发现**: inventory 注册对 composer 公共 API 可见
2. **同链**: 官方 memory（priority 5）与第三方替身（priority 15）在同一 registry 装配，priority 排序跨官方/第三方一致生效
3. **同规则**: 预算瀑布对替身与官方资源一视同仁——预算不足时**整段跳过，绝不截断**（skip-not-truncate）

## 替换官方资源 vs 新增 scheme

| 场景 | 做法 |
|---|---|
| 新增自有 scheme（如 `echo-input`） | 本文路径: 实现 + inventory 注册 |
| 替换官方 memory | 写一个同语义的自定义资源，在 `agentcontext.json` 高层用不含 `memory` 条目的 resources 数组禁用官方插件，再注册你的 scheme（参见 [Memory 插件指南 · 配置三态](./memory-plugin-guide.md#配置三态)） |
| 仅调整官方资源行为 | 优先用 priority 覆盖 / 配置定制，不必写代码 |

注意: 官方 `plugin-memory` 与第三方资源走**同一 inventory 注册路径**（迭代 3 R1 整改统一; per-agent 状态经 config envelope 注入，见 [Memory 插件指南 · 注册路径](./memory-plugin-guide.md#注册路径r1-规范化)）。

## 已知限制

- **inventory 同 scheme 重复注册的提交顺序不保证**——不要依赖「注册同名 scheme 覆盖官方资源」; 替换官方资源走上表的禁用+新增路径。
- inventory 注册发生在进程链接期，官方插件与第三方插件在同一 registry 汇聚，身份以 `plugin.json` 清单为准（无清单的显示为 `external`）。
