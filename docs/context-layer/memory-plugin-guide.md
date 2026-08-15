# Memory 插件使用指南

> 迭代 2 起，memory 上下文资源以**官方插件形态**（独立 `plugin-memory` crate）接入 ContextResource 链。本文说明插件架构、配置三态（禁用/覆盖/定制）与行为语义。配置字段完整语法参见[配置参考](./configuration.md)。

---

## 背景：什么是 memory 插件化

迭代 2 之前，memory 段落由 compose 路径**预渲染**：composer 在装配链之外单独执行记忆选择，把渲染好的字符串塞给一个包装 Provider。这意味着 memory 是装配链的特判——它不参与 priority 排序的普通路径，第三方也无法用同样方式提供自己的记忆资源。

迭代 2 将 memory 业务整体迁入独立 crate `crates/plugin-memory`，以标准 `ContextResource` 形态接入:

```
agentcontext.json 声明 → builtin_resource_factories 捕获 MemorySpec
  → MemoryResource（scheme="memory", priority=5）
  → 链装配阶段自行检索 + 渲染（assemble 六步）
```

compose 路径的预渲染特判已删除，memory 与 file / warm-summary / message-list 走完全相同的工厂表与装配链。

**零行为变更保证**: 默认配置下段落内容、顺序、降级、预算瀑布与旧预渲染管线 golden 等价（`matches_legacy_prerender_pipeline` 测试锁定）。

## 插件架构

### 无特权依赖边界

`plugin-memory` 的 Cargo.toml 仅依赖:

- `context-layer-core`（公共契约: ContextResource / AssemblyContext / PromptSection）
- `proto`（MemorySpec 类型）
- 基础库（anyhow / serde / chrono / tracing）

**编译期禁止依赖 `agent-runtime`**——这正是「无特权接缝」的实证: 官方插件与第三方插件使用同一契约面，没有隐藏后门。第三方可以写出与官方 memory 完全同权的替身插件（参见[第三方资源扩展指南](./third-party-resource-guide.md)）。

### assemble 六步

`MemoryResource::assemble` 在链装配阶段执行:

1. 检查捕获的 `MemorySpec`——`None`（未配置）或 `delivery.prompt=false`（MCP-only）→ 返回空（不贡献段落）
2. 打开 JSONL 存储（`spec.store.root`，相对路径基于 `ctx.profile_dir`）
3. 构造 `MemorySelector`
4. `load_bootstrap_and_turn(ctx.turn_input, ctx.delivery_context)`——用当前回合输入检索
5. 渲染 bootstrap / turn 两段落（空内容跳过）
6. 任何错误 → `tracing::warn` + 返回空（**降级不中断**，坏存储不能卡死回合）

### 为什么不经 inventory 注册

`plugin-memory` 刻意**不**用 `inventory::submit!` 自注册: inventory 工厂是零参闭包，拿不到 per-agent 的 `MemorySpec`；注册反而会遮蔽捕获 spec 的内置工厂。这是 ARCH 迭代 2 裁决二的设计约束——需要 per-agent 状态的官方资源走工厂捕获，无状态第三方资源走 inventory。

## 配置三态

三态操作的都是 `agentcontext.json` 的 `resources` 数组中 `scheme: "memory"` 的条目。三级继承（spec → profile → workspace）按 per-field 合并: `Some` 覆盖，`None` 继承。

### 禁用（disable）

逐字段合并没有删除语义，禁用 = 在最高层用**不含 memory 条目的完整 resources 数组**覆盖:

```json
{
  "resources": [
    { "scheme": "warm-summary", "priority": 7 },
    { "scheme": "message-list", "priority": 10 }
  ]
}
```

注意这会同时丢弃下层对其它资源的定制——如需保留，把保留条目一并写入。

### 覆盖（override）

更高层级声明同 scheme 条目并显式指定字段。显式 `priority` 真实作用于装配顺序（迭代 2 行为变更: 此前显式 priority 与内置值不同时被静默忽略）:

```json
{ "resources": [ { "scheme": "memory", "priority": 25 } ] }
```

priority 越小越先装配、越先扣预算。默认声明 5；覆盖为 25 后 memory 会在 warm-summary(7) / message-list(10) 之后装配。

### 定制（customize）

`config` 字段会传入资源工厂。memory 插件**当前忽略** `config`（业务配置以 `spec.json` 的 `MemorySpec` 为准，与历史行为一致），为未来覆盖 `MemorySpec` 子集（如 `top_k`）预留。第三方替身插件则完全依赖自身 `config`。

## 行为语义速查

| 情形 | 结果 |
|---|---|
| Agent 未配置 MemorySpec | 两段落均为空，不贡献内容 |
| `delivery.prompt=false`（MCP-only） | 同上 |
| 存储文件损坏 / 检索失败 | warn 日志 + 空输出降级，回合不中断 |
| 预算不足 | 整段跳过（skip-not-truncate，绝不截断） |
| 显式 `priority` 与内置值不同 | 真实生效于装配顺序（迭代 2 起） |
| 默认 spec（不覆盖） | 声明值与内置值一致，行为与迭代 1 前完全相同 |

## 迁移说明

- **存量配置无需改动**: 默认 spec 的声明 priority 与内置值一致，等价性由 golden 测试锁定。
- **依赖隐式 priority=0 的配置**: 迭代 1 已公告（CHANGELOG [Unreleased]）——省略 priority 现为继承/100，显式 `"priority": 0` 仍是永不跳过的保留值。
- **代码消费方**: `agent_runtime::memory::*` import 路径不变（重导出 shim）；新代码建议直接依赖 `plugin-memory`。

## 故障排查

| 症状 | 排查 |
|---|---|
| memory 段落消失 | 检查是否被高层 resources 覆盖禁用; MemorySpec 是否存在; `delivery.prompt` 是否为 false |
| 顺序不对 | 检查各层是否有显式 priority 覆盖（迭代 2 起真实生效） |
| 回合日志出现 `memory selection failed; skipping memory section` | JSONL 存储损坏或路径不可读——插件已降级为空输出，修复存储后自动恢复 |
