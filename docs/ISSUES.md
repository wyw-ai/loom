# Loom Issues

> 本文档追踪 Loom 项目中已发现的问题、Bug 与待改进项。
> 关联 Obsidian 笔记：[[Loom 文档索引 (Loom Index)]]

## 活跃问题

### #1 Copilot CLI 首次运行 `--resume` 参数错误 ✅ 已修复 (2026-06-14)

- **严重程度**: 🔴 Critical — Agent 完全无法启动
- **影响范围**: Copilot CLI provider 首次运行
- **根因**: `copilot_manifest()` 将 `--resume {session.id}` 同时用作首次运行参数和恢复运行参数，导致 Copilot CLI 收到尚未创建的 session UUID 而报错 `No session, task, or name matched`
- **修复**: 拆分 `first_run_args`（`--session-id`）和 `resume_args`（`--resume`）
- **文件**: `crates/agent-runtime/src/provider.rs`, `crates/agent-runtime/src/discovery.rs`
- **关联**: [[Copilot CLI 首次运行 Bug 分析]]

### #2 GUI Agent Env 键值编辑器缺失 ✅ 已修复 (2026-06-14)

- **严重程度**: 🟡 Medium — 功能缺口
- **影响范围**: GUI Desktop Agent 创建/编辑界面
- **描述**: `AgentProviderRef.env` 字段已添加到 proto 和运行时层，但 GUI 端缺少对应的键值编辑器，用户无法通过图形界面为 agent 配置环境变量
- **修复**: 在 Agent 创建对话框和成员详情面板中增加 Env 键值编辑器（添加/编辑/删除）
- **文件**: `apps/gui-web/src/App.tsx`, `apps/gui-web/src/ipc/bridge.ts`, `apps/gui-web/src/ipc/types.ts`, `crates/cli/src/cmd/daemon.rs`, `crates/gui/src/ipc.rs`
- **关联**: [[GUI Agent Env 键值编辑器设计]], [[2026-06-13 AgentProviderRef Env 注入 (实施计划)]]

### #3 `resolve_actor_alias` 返回僵尸 Actor 导致 `message.send` 失败 ✅ 已修复 (2026-06-14)

- **严重程度**: 🔴 Critical — 消息发送完全失败
- **影响范围**: 所有 `@actor` 提及解析，影响 message.send、task.assign 等
- **根因**: Daemon 重启后 `machine_id` 变化导致 agent 的 actor ID 改变（如 `b1a29c41` → `31961458`），但旧 actor 残留在 server 内存中。`resolve_actor_alias()` 使用 `HashMap::values().find_map()` 返回第一个匹配 `display_name` 的 actor，由于 HashMap 迭代顺序不确定，可能返回僵尸 actor。僵尸 actor 不在频道的 explicit member 列表中，导致 `validate_scope_routing_actor()` 拒绝消息发送。
- **修复**:
  1. `upsert_actor()` 改为 remove-then-insert，确保最新 upsert 的 actor 在 HashMap 迭代顺序末尾
  2. `resolve_actor_alias()` 改用 `filter().last()` 收集所有匹配并返回最后（最新）的 actor
  3. `apply()` 中 `Mutation::ActorUpsert` 同样改为 remove-then-insert 保持 journal 回放一致性
  4. `reconcile_agents()` 在停止 stale agent 前异步调用 `actor/delete` 清理 server 端僵尸
- **文件**: `crates/server/src/store.rs`, `crates/cli/src/cmd/daemon.rs`
- **关联**: [[2026-06-14 resolve_actor_alias 僵尸 Actor Bug]]

### #4 Agent 在频道删除后继续执行导致错误循环 🔴 进行中 (2026-06-14)

- **严重程度**: 🔴 Critical — Agent 持续重试已删除频道的操作，浪费资源并产生噪音
- **影响范围**: 所有 agent worker，影响频道删除后的 agent 运行稳定性
- **根因**: Agent 的 `notification_loop()` 只处理 `MESSAGE_CREATED`、`EVENT_CREATED`、`RUN_UPDATED` 三种流更新，完全忽略了 `CHANNEL_DELETED` 事件。当频道被删除时：
  1. Server 正确广播 `CHANNEL_DELETED` 事件（ws.rs:371-410），public 频道广播给所有连接，private 频道推送到成员 inbox
  2. Agent worker 收到 `STREAM_UPDATE` 但 `kind` 不匹配任何已知类型，直接 `continue` 跳过
  3. Agent 继续处理该频道的 pending trigger 和 inbox delivery，反复尝试 `scope/subscribe` 和 `run.open`，均失败于 "not a member of channel"（code -32002）
  4. `drain_pending_inbox` 中的 `is_unreachable_scope_error` 可以 ack 这些失败消息，但 agent 仍在浪费 CPU/网络资源重试
- **修复**:
  1. `WorkerState::cancel_channel_work(channel_id)` — 遍历所有 `active_turns`，标记属于该频道的 turn 为 `cancel_requested`；清除 `pending_triggers` 中该频道的队列；释放 `scope_busy` 锁
  2. `notification_loop` 新增 `CHANNEL_DELETED` 分支 — 调用 `cancel_channel_work()` + `adapter.cancel()` 停止正在运行的 LLM 推理
- **文件**: `crates/cli/src/cmd/agent_serve.rs`
- **关联**: [[2026-06-14 频道删除 Agent 未停止 Bug]]

## 已关闭

_（暂无）_

## 变更日志

| 日期 | 描述 |
|------|------|
| 2026-06-14 | 修复频道删除后 Agent 继续执行导致错误循环（新增 CHANNEL_DELETED 处理） |
| 2026-06-14 | 修复 `resolve_actor_alias` 返回僵尸 Actor 导致 `message.send` 失败 |
| 2026-06-14 | 修复 Copilot CLI 首次运行 `--session-id` 与 `--resume` 参数分离 |
| 2026-06-14 | 实现 GUI Agent Env 键值编辑器 |
| 2026-06-13 | 添加 `AgentProviderRef.env` 字段（proto → runtime → CLI） |
