# Message List — 热/温/冷三层与 Session Reset

> Loom 的上下文分层管理机制：Hot（热）、Warm（温）、Cold（冷）三层模型，
> 以及基于阈值的 Session Reset 流程。

---

## 三层模型概览

| 层级 | 内容 | 存储位置 | 生命周期 |
|---|---|---|---|
| **Hot（热）** | 当前轮的完整投递上下文（Thread 消息、收件箱条目） | 运行时内存 | 单轮 |
| **Warm（温）** | 历史消息的摘要 | `<profile>/warm_summary.md` | 跨轮持久化 |
| **Cold（冷）** | 完整历史消息归档 | Loom 服务器端存储 | 永久 |

### 设计目标

- **Hot 层**保持当前轮所需的精确上下文，确保 Agent 有完整的对话历史来理解当前任务
- **Warm 层**在 Session Reset 后提供历史摘要，避免完全丢失上下文
- **Cold 层**由 Loom 服务器管理，可通过 `loom message read` 检索，不进入模型上下文

---

## Hot 层 — 投递上下文

### 数据来源

每轮启动时，Loom 运行时将以下内容组装为 `delivery_context` 字符串：

1. **Thread 消息历史** — 当前 Thread 中截至本轮的所有可见消息
2. **收件箱条目** — 本轮投递的待处理收件箱项
3. **系统上下文** — Actor ID、Channel ID、作用域信息

### 注入方式

- **D1 模式**：直接拼接到 `compose_envelope_prompt()` 的标准信封中
- **D2 模式**：通过 `MessageListProvider.assemble()` 生成 `delivery_context` 段落，
  注入到 ContextResource 链中

### Token 占用

Hot 层是 Token 消耗的主要来源。长 Thread 中消息历史可能占据大量上下文预算。
这是 Session Reset 机制存在的原因。

---

## Warm 层 — 摘要持久化

### 触发条件

Session Reset 触发时，运行时执行以下步骤：

```
条件 1: token_usage > SESSION_RESET_TOKEN_THRESHOLD (80%)
条件 2: message_count > SESSION_RESET_MESSAGE_THRESHOLD (50)
条件 3: message_count >= SESSION_RESET_MIN_MESSAGES (20)

触发 = (条件 1 OR 条件 2) AND 条件 3
```

### 摘要生成流程

1. **检测触发** — `should_trigger_session_reset()`（agent_serve.rs L8477）在每轮开始时检查 Token 使用率和消息数
2. **生成摘要** — 使用摘要生成 Prompt 调用模型生成摘要
3. **持久化** — 调用 `WarmSummaryContextResource::persist()`（warm_summary.rs L74）将摘要原子写入 `{profile_dir}/summaries/{scope_id}.md`
4. **清空历史** — 清空当前 Session 的内存消息历史
5. **注入摘要** — D2 chain 中 `WarmSummaryContextResource::assemble()`（warm_summary.rs L119）自动读取并注入摘要
6. **记录事件** — 记录 Session Reset 事件用于审计

### 摘要预算

```rust
const WARM_SUMMARY_BUDGET_FRACTION: f64 = 0.2;  // warm_summary.rs L26
```

Warm 摘要最多占用 **剩余** 上下文预算的 20%（`budget_remaining * 0.2`）。
超预算时跳过注入（不截断，C-3 约束）。

### 摘要文件格式

`{profile_dir}/summaries/{scope_id}.md` 是纯 Markdown 文件：

```markdown
# Session Summary

## Key Decisions
- ...

## Active Tasks
- ...

## Important Context
- ...
```

每次 Session Reset 会覆盖此文件。

---

## Cold 层 — 服务器端归档

### 存储机制

完整消息历史永久存储在 Loom 服务器端。Agent 运行时不直接访问 Cold 层。

### 检索方式

通过 Loom CLI 检索历史消息：

```bash
# 读取 Thread 的完整消息历史
loom --json message read --target '#<channel_id>:<root_message_id>'

# 搜索特定关键词
loom --json message search --query "keyword" --target '#<channel_id>:<root_message_id>'

# 分页读取（按消息 ID 向前翻页）
loom --json message read --target '#<channel_id>:<root_message_id>' --before <message_id>
```

### 与 Hot/Warm 层的关系

- Cold 层不进入模型上下文，不消耗 Token
- Agent 可通过 Loom CLI 主动检索 Cold 层数据（如需要历史细节）
- Warm 摘要是 Cold 层的"索引" — 帮助 Agent 判断是否需要检索完整历史

---

## Session Reset 详细时序

```
轮开始
  │
  ├─ 检查触发条件
  │   ├─ token_usage > 80%? ──────────┐
  │   ├─ message_count > 50? ─────────┤
  │   └─ message_count >= 20? ────────┤
  │                                   │
  │   触发? ──── No ──→ 正常处理本轮
  │              │
  │              Yes
  │              │
  ├─ 1. 生成 Warm 摘要
  │     └─ 摘要 Prompt → 模型调用 → 摘要文本
  │
  ├─ 2. 持久化摘要
  │     └─ WarmSummaryContextResource::persist() → {profile_dir}/summaries/{scope_id}.md
  │
  ├─ 3. 清空 Session 消息历史
  │     └─ 内存中的 Vec<Message> 清空
  │
  ├─ 4. 注入 Warm 摘要（D2 模式）
  │     └─ WarmSummaryContextResource::assemble() → 通过 D2 chain 自动注入
  │
  ├─ 5. 记录 Reset 事件
  │     └─ 日志记录（审计用途）
  │
  └─ 继续处理本轮（使用精简后的上下文）
```

### reset_session Trait

```rust
// adapter.rs L82-84 — 默认实现（无操作）
fn reset_session(&mut self) -> Result<()> {
    Ok(())
}

// acp.rs L431-446 — 实际实现
fn reset_session_internal(&mut self) -> Result<()> {
    // 执行上述 6 步时序
}
```

---

## 配置参数参考

| 常量 | 位置 | 默认值 | 说明 |
|---|---|---|---|
| `SESSION_RESET_TOKEN_THRESHOLD` | agent_serve.rs L8460 | `0.8` | Token 使用率阈值（80%） |
| `SESSION_RESET_MESSAGE_THRESHOLD` | agent_serve.rs L8463 | `50` | 消息数阈值 |
| `SESSION_RESET_MIN_MESSAGES` | agent_serve.rs L8466 | `20` | 最小消息数（低于此数不触发） |
| `WARM_SUMMARY_BUDGET_FRACTION` | warm_summary.rs L26 | `0.2` | Warm 摘要占剩余预算比例（20%） |

---

## 与 ContextLayer 的关系

### D1 模式（`context_layer = None`）

- Warm 摘要 **不注入**（AC-WS-3 设计意图）
- D1 是纯遗留兼容路径，不参与 D2 chain 的预算瀑布控制
- Session Reset 机制仍然生效（摘要仍会生成和持久化），但不注入提示词

### D2 模式（`context_layer = Some`）

- `warm-summary` scheme 在 `agentcontext.json` 中声明，优先级 7
- `WarmSummaryContextResource` 通过 D2 chain 自动组装
- 预算瀑布控制：`budget_remaining * 0.2`，超预算跳过（不截断，C-3）
- 与 Memory(5)、Skill(8)、MessageList(10)、File(20) 共同参与链式组装

### 混合模式

Agent 从 D1 迁移到 D2 时：
- Session Reset 机制始终生效（无论是否配置 ContextLayer）
- 启用 `context_layer` 后，Warm 摘要自动通过 D2 chain 注入
- ContextLayer 链在 Session Reset 后重建（使用精简后的上下文）

---

## 已知限制

1. **Warm 摘要质量依赖模型** — 摘要由模型生成，质量取决于模型能力
2. **Session Reset 是不可逆的** — 一旦执行，内存中的消息历史被清空（Cold 层仍有完整归档）
3. **阈值是全局的** — 当前不支持按 Agent 或 Thread 自定义阈值
4. **Warm 摘要文件无版本管理** — 每次 Reset 覆盖之前的摘要（可通过 git 跟踪 profile 目录缓解）