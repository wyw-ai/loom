# ContextLayer — 源码导航

> ContextLayer 实现的文件级和行级索引。
> 源码位置：`F:\pj\loom`，分支：`feat/context-layer-message-list-mvp`

---

## 文件清单

| 文件 | 行数 | 职责 |
|---|---|---|
| `crates/context-layer-core/src/lib.rs` | 200 | **共享接口 crate** — ContextResource trait, AssemblyContext, PromptSection, ContextResourcePlugin, estimate_tokens |
| `crates/plugin-memory/src/lib.rs` | — | **官方 memory 插件 crate**（迭代 2）— MemoryResource, open_memory_store |
| `crates/agent-runtime/src/context_layer.rs` | 320 | 核心 Trait + Registry + 预算瀑布 + discover_plugins() |
| `crates/agent-runtime/src/context_layer/warm_summary.rs` | 163 | WarmSummaryContextResource |
| `crates/agent-runtime/src/context_layer/builder.rs` | 173 | ContextResourceBuilder + ClosureContextResource |
| `crates/agent-runtime/src/memory/mod.rs` | 14 | 重导出 shim（迭代 2 起业务在 plugin-memory） |
| `crates/agent-runtime/src/message_list.rs` | 124 | MessageListProvider |
| `crates/agent-runtime/src/filesystem.rs` | 191 | FileSystemProvider |
| `crates/agent-runtime/src/envelope.rs` | — | PromptSection 定义 |
| `crates/agent-runtime/src/proto/types.rs` | — | AgentContextSpec, ContextResourceSpec, ScopeKind |
| `crates/cli/src/cmd/agent_serve.rs` | — | 集成层：工厂注册表 + 链构建 + 信封组装 |
| `crates/cli/build.rs` | — | build.rs 双路径处理（plugin.json / pure skill） |
| `crates/cli/src/adapter.rs` | — | reset_session Trait 默认实现 |
| `crates/cli/src/acp.rs` | — | reset_session 运行时实现 |
| `crates/cli/src/interactive.rs` | — | reset_session CLI 入口 |

### Plugin 仓库（loom-plugin-context-tier）

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | inventory::submit! 自注册（warm-summary + message-list） |
| `src/warm_summary.rs` | WarmSummaryContextResource 实现 |
| `src/message_list.rs` | MessageListProvider 实现 |
| `plugin.json` | Plugin 清单（id, layer, context_resources, skills） |
| `Cargo.toml` | Crate 配置（依赖 context-layer-core） |

---

## context-layer-core/src/lib.rs — 共享接口 crate

| 行号 | 符号 | 说明 |
|---|---|---|
| L37-42 | `PromptSection` | 命名的提示词段落（`name: &'static str`, `content: String`） |
| L57-87 | `ContextResource` trait | 核心 AOP 接口：`scheme()`, `priority()`, `effective_scope()`, `assemble()` |
| L93-113 | `AssemblyContext` | 只读上下文（scope, channel_id, actor_id, profile_dir, budget_remaining, budget_total, delivery_context, first_turn） |
| L120-127 | `ContextResourcePlugin` | inventory 自注册类型（scheme + factory 函数） |
| L130 | `inventory::collect!` | 启用 ContextResourcePlugin 的 inventory 收集 |
| L140-165 | `estimate_tokens()` | Token 估算（ASCII ~4字符/token, CJK 1字符/token） |

---

## context_layer.rs — 核心模块

| 行号 | 符号 | 说明 |
|---|---|---|
| L27 | `extern crate loom_plugin_context_tier` | 强制链接 plugin crate（防止 inventory 注册被剥离） |
| L34-38 | Re-exports | 从 context-layer-core 重导出 ContextResource, AssemblyContext 等 |
| L43-65 | `ResourceProvider` trait | 外部资源的数据访问层：`list()`、`read()` |
| L117-129 | `ResourceHandle` / `ResourceContent` | Provider 返回的数据结构 |
| L137-180 | `ContextResourceRegistry` | 资源链持有者 + `assemble_chain()` 预算瀑布算法 |
| L191-196 | `discover_plugins()` | 通过 `inventory::iter` 发现所有自注册 plugin |

### assemble_chain 算法（L137-232）

```
输入: resources (按 priority 升序), budget_remaining
循环:
  1. 跳过 effective_scope 不匹配的资源
  2. 调用 resource.assemble(ctx)
  3. 对每个返回的 PromptSection:
     - 估算 Token 数
     - if priority != 0 AND section_tokens > budget_remaining:
         跳过 (不截断, C-3/C-6)
     - else:
         budget_remaining -= section_tokens
         包含该段落
输出: (Vec<PromptSection>, budget_remaining)
```

---

## plugin-memory — MemoryResource（迭代 2 官方插件）

memory 业务已迁入独立 crate `crates/plugin-memory`（仅依赖 `context-layer-core` + `proto`，无 agent-runtime 特权）。`crates/agent-runtime/src/memory/mod.rs` 仅保留重导出 shim，`agent_runtime::memory::*` import 路径不变。

| 符号 | 说明 |
|---|---|
| `MemoryResource::new(spec: Option<MemorySpec>)` | 捕获 per-agent spec；`None` = 不贡献段落 |
| `MemoryResource` (impl ContextResource) | scheme `"memory"` / priority 5 / Thread + Channel |
| `assemble()` | 链装配阶段检索 + 渲染 `bootstrap_memory` / `turn_memory`；读取 `ctx.turn_input` / `ctx.delivery_context`；错误 warn + 空输出降级 |
| `open_memory_store(profile_dir, spec)` | 打开 JSONL 存储（公开供 MCP bridge 复用） |

---

## message_list.rs — MessageListProvider

| 行号 | 符号 | 说明 |
|---|---|---|
| L15-25 | `MessageListProvider` struct | 无内部状态 |
| L27-35 | `new()` | 创建 Provider |
| L37-45 | `scheme()` → `"message-list"` | URI scheme 标识 |
| L47-55 | `priority()` → `10` | 组装优先级 |
| L65-90 | `assemble()` | 将 `delivery_context` 包装为单个段落 |

---

## filesystem.rs — FileSystemProvider

| 行号 | 符号 | 说明 |
|---|---|---|
| L20-30 | `FileSystemProvider` struct | 持有 `mount_path` 和 `max_files` |
| L32-45 | `new(mount_path, max_files)` | 创建 Provider |
| L47-55 | `scheme()` → `"file"` | URI scheme 标识 |
| L60-80 | `list()` | 枚举目录中的文件，返回 `ResourceHandle` 列表 |
| L85-120 | `read()` | 读取单个文件（限制 128KB），返回 `ResourceContent` |
| L125-140 | 路径遍历保护 | canonicalize 检查防止 `../` 逃逸 |
| L145-160 | 媒体类型推断 | 根据扩展名推断 `media_type` |

### 常量

| 常量 | 值 | 说明 |
|---|---|---|
| `MAX_READ_BYTES` | 131072 (128KB) | 单文件读取上限 |

---

## context_layer/warm_summary.rs — WarmSummaryContextResource

| 行号 | 符号 | 说明 |
|---|---|---|
| L26 | `WARM_SUMMARY_BUDGET_FRACTION` | `0.2` — Warm 摘要占剩余预算的 20% |
| L33-35 | `WarmSummaryContextResource` struct | 持有 `effective_scopes` |
| L38-42 | `new()` | 创建实例，默认 `[Thread, Channel]` |
| L49-51 | `summary_dir()` | 目录路径：`{profile_dir}/summaries/` |
| L54-56 | `summary_path()` | 文件路径：`{profile_dir}/summaries/{scope_id}.md` |
| L60-69 | `load()` | 读取持久化摘要，空文件返回 `None` |
| L74-85 | `persist()` | 原子写入摘要（临时文件 + 重命名） |
| L90-97 | `clear()` | 删除摘要文件（scope 删除时调用） |
| L106-148 | `ContextResource` trait impl | scheme=`"warm-summary"`, priority=7, assemble 逻辑 |
| L126 | budget 计算 | `(budget_remaining as f64) * 0.2` |
| L128-136 | 超预算处理 | 跳过注入（不截断，C-3） |

---

## context_layer/builder.rs — ContextResourceBuilder

| 行号 | 符号 | 说明 |
|---|---|---|
| L32-37 | `ContextResourceBuilder` struct | scheme, priority, scopes, assembler closure |
| L42-49 | `new(scheme)` | 创建 Builder，默认 priority=15, scopes=[Thread, Channel] |
| L52-55 | `priority(n)` | 设置优先级 |
| L58-61 | `scopes(vec)` | 设置生效作用域 |
| L64-70 | `assemble(closure)` | 提供 assemble 闭包（必须调用） |
| L74-85 | `build()` | 返回 `Box<dyn ContextResource>`，内部创建 `ClosureContextResource` |
| L89-94 | `ClosureContextResource` struct | 内部适配器，存储闭包 |
| L96-112 | `ContextResource` impl | 通过闭包委托 assemble 调用 |

---

## agent_serve.rs — 集成层

### 工厂注册表 + 链构建

| 行号 | 符号 | 说明 |
|---|---|---|
| L8634-8679 | `builtin_resource_factories()` | 工厂注册表 HashMap：memory, message-list, file, warm-summary |
| L8674-8676 | warm-summary 工厂 | `Box::new(WarmSummaryContextResource::new())` |
| L8683-8703 | `build_context_resource_chain()` | 遍历 spec.resources，从工厂注册表查找并注册 |
| L8769 | `compose_envelope_prompt()` | 始终使用 D2 chain（D1 fallback 已删除） |
| L8799 | D2 chain 注释 | "Always use D2 context resource chain" |
| L8809 | `unwrap_or_else(default_agent_context_spec)` | context_layer 为 None 时使用默认 chain |
| L8828 | `compose_with_context_chain()` | 实际组装调用点 |

### Session Reset 集成

| 行号 | 符号 | 说明 |
|---|---|---|
| L8460 | `SESSION_RESET_TOKEN_THRESHOLD` | `0.8` — Token 使用率超过 80% 触发 |
| L8463 | `SESSION_RESET_MESSAGE_THRESHOLD` | `50` — 消息数超过 50 条触发 |
| L8466 | `SESSION_RESET_MIN_MESSAGES` | `20` — 最少 20 条消息才考虑 Reset |
| L8477-8490 | `should_trigger_session_reset()` | 触发条件判定函数 |
| L4353 | `WarmSummaryContextResource::clear()` | CHANNEL_DELETED handler 中清除摘要 |
| L9725 | `WarmSummaryContextResource::persist()` | Finished handler 中持久化摘要 |
| L5597 | reset 触发检查 | 在 agent serve 主流程中调用 |

---

## adapter.rs / acp.rs / interactive.rs — Session Reset

| 文件 | 行号 | 符号 | 说明 |
|---|---|---|---|
| `adapter.rs` | L82-84 | `reset_session()` 默认实现 | 返回 `Ok(())`（无操作） |
| `acp.rs` | L431-446 | `reset_session_internal()` | 执行 6 步 Session Reset 时序 |
| `acp.rs` | L532-534 | `reset_session()` | 调用 `reset_session_internal()` |
| `interactive.rs` | — | `reset_session()` 入口 | CLI 命令入口 |

### Session Reset 6 步时序

```
1. 检查触发条件 (token > 80% OR messages > 50) AND messages >= 20
2. 生成 Warm 摘要 (摘要 Prompt → 模型调用)
3. 持久化摘要: WarmSummaryContextResource::persist() → {profile_dir}/summaries/{scope_id}.md
4. 清空当前 Session 的消息历史
5. D2 chain 中 WarmSummaryContextResource::assemble() 自动注入摘要
6. 记录 Reset 事件
```

---

## proto/types.rs — 类型定义

| 行号 | 符号 | 说明 |
|---|---|---|
| L3323-3330 | `AgentSpec.context_layer` | `Option<AgentContextSpec>` 字段 |
| L3358-3367 | `AgentContextSpec` | 顶层配置对象：`version`、`effective_scope`、`resources` |
| L3374-3383 | `ContextResourceSpec` | 资源声明：`scheme`、`mount`、`priority`、`config` |
| L3386-3391 | `ContextScopeKind` | 枚举：`Thread`、`Channel`（序列化为小写字符串） |

### proto/methods.rs — 默认 chain

| 行号 | 符号 | 说明 |
|---|---|---|
| L3403 | `default_agent_context_spec()` | 返回默认 chain：memory(5) + warm-summary(7) + message-list(10)。当 `context_layer` 为 None 时使用 |

---

## 测试覆盖（commit 220fc50）

| 文件 | 测试 | 说明 |
|---|---|---|
| `context_layer.rs` | `assemble_chain_budget_waterfall_includes_within_budget` | 预算内资源正常包含 |
| `context_layer.rs` | `assemble_chain_budget_waterfall_skips_over_budget` | 超预算资源被跳过 |
| `context_layer.rs` | `assemble_chain_priority_zero_always_included` | 优先级 0 始终包含 |
| `context_layer.rs` | `assemble_chain_respects_scope_filtering` | 作用域过滤生效 |
| `warm_summary.rs` | `assemble_with_summary_produces_formatted_section` | 摘要生成格式化段落 |
| `warm_summary.rs` | `assemble_skips_when_summary_exceeds_budget` | 超预算时跳过摘要 |
| `warm_summary.rs` | `persist_and_clear_file_io` | persist/clear 文件 I/O |
| `agent_serve.rs` | `builtin_resource_factories_registers_all_schemes` | 工厂注册所有 scheme |
| `agent_serve.rs` | `builtin_resource_factories_produces_correct_schemes` | 工厂产出正确 scheme |
| `agent_serve.rs` | `build_context_resource_chain_assembles_default_spec` | 默认 spec 正确组装 |
| `agent_serve.rs` | `build_context_resource_chain_skips_unknown_scheme` | 未知 scheme 被跳过 |

**总计：31 个测试，ALL PASS，0 failures，0 flaky**

---

## 提交历史

| Commit | 阶段 | 变更 |
|---|---|---|
| `d301b22` | D1 — Warm 摘要 + Session Reset | 4 files, +496/-5 |
| `ba34a09` | D2 — ContextResource Trait + Provider | 7 files, +1123/-11 |
| `4172e3c` | D2 修复 | 4 files, +8/-1 |
| `07f2406` | WarmSummary D2 化 + 插件 DX | 5 files, +454/-190 (warm_summary.rs + builder.rs 新增, 8 旧函数删除, 工厂 HashMap 替代 match) |
| `3b391ba` | D2 默认化 — 始终使用 ContextResource chain | 2 files, +62/-78 (default_agent_context_spec + D1 fallback 删除) |
| `220fc50` | 测试补充 — 6 个缺失场景 | 3 files, +253 (context_layer + warm_summary + agent_serve 测试) |
| Phase 3 commits | Plugin Independence — context-layer-core + inventory 自注册 | context-layer-core crate 提取, loom-plugin-context-tier Rust crate 化, discover_plugins(), build.rs 双路径 |
| `761405f` | Plugin 命名规范执行 (loom) | 7 files, +32/-31 (context-tier-skill → loom-plugin-context-tier) |
| `9111d06` | Plugin 命名规范执行 (plugin repo) | 3 files, +8/-7 (Cargo.toml + plugin.json + README) |