# Loom Issues

> 本文档追踪 Loom 项目中已发现的问题、Bug 与待改进项。
> 关联 Obsidian 笔记：[[Loom 文档索引 (Loom Index)]]

> 注：2026-07-06 后，Copilot instructions 注入策略已被
> `docs/protocol/agent-runtime-awareness.md` 中的方案取代：Loom 不再通过默认
> system prompt 注入 runtime guidance，而是使用 workspace `AGENTS.md`、默认
> `loom` skill 和 `loom guide`。

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

### #5 Copilot CLI Session 上下文膨胀导致长需求无响应 ✅ 已修复 (2026-06-14)

- **严重程度**: 🔴 Critical — Agent 收到长消息后无限等待 API 响应
- **影响范围**: 所有使用 Copilot CLI provider 的 agent
- **根因**: Copilot CLI 不支持 `--system-prompt` 参数（与 Codex/CC 不同），LOOM 将所有 prompt 内容通过 `-p "{prompt.full}"` 传入。每次 resume，copilot 的 events.jsonl 都会新增一条 user.message（~47KB），其中 40KB 为固定不变的 `agent_instructions`。52 轮对话后 session 膨胀至 10MB/1769 条事件，传递给模型 API 时超时。
- **修复**（Provider manifest 注入策略 — `instructions_via`）:
  1. `ProviderModeSpec` 新增 `instructions_via` 字段：`"prompt"`（默认）或 `"agents_md"`
  2. Copilot manifest 设置 `instructions_via: "agents_md"`
  3. `ensure_agents_md` 扩展，接受 `agent_instructions` 和 `actor_context` 写入 AGENTS.md
  4. `compose_envelope_prompt` 对 `instructions_via == "agents_md"` 的 provider 跳过 prompt 中的指令段落
  5. Copilot CLI 通过 `--add-dir` 自动加载 `AGENTS.md`（官方标准机制）
  6. 编译通过，全部 677 测试无回归
- **预期效果**: 每条 user.message 从 ~47KB → ~5KB（减少 89%），session 总量从 ~10MB → ~3MB
- **文件**: `crates/agent-runtime/src/provider.rs`, `crates/agent-runtime/src/agents_md.rs`, `crates/proto/src/methods.rs`, `crates/cli/src/cmd/agent_serve.rs`
- **设计文档**: [[2026-06-14-copilot-instructions-injection-design]]
- **关联**: [[Session 上下文去重优化设计]]

### #6 HANSIONSTATION Host 无法删除 + loom-shell ANSI 乱码 ✅ 已修复 (2026-06-18)

- **严重程度**: 🔴 Critical — GUI 无法删除遗留 host，loom-shell 日志显示乱码
- **影响范围**: GUI Host 管理、loom-shell 控制面板
- **问题 A — HANSIONSTATION 无法删除**:
  - 遗留 `actor_service_local`（displayName: "HANSIONSTATION"）存储在默认服务器数据库 `%APPDATA%\loom\server\loom.sqlite3`（85 MB）中
  - GUI `machine_remove` 命令是硬编码 stub，始终返回 "host is daemon-owned; stop or reconfigure the daemon instead"
  - 通过 CLI `actor delete actor_service_local` 连接默认数据目录删除
- **问题 B — loom-shell 日志 ANSI 乱码**:
  - 三层缺陷：`log_viewer.rs` 无 ANSI 剥离 → `ui.rs` `SetWindowTextW` 不支持 ANSI → `agent-runtime` `strip_ansi()` 仅处理 CSI
  - 修复：扩展 `strip_ansi()` 覆盖 OSC/DCS/APC/SOS 全部 ESC 变体，在 `command.rs`、`log_viewer.rs`、`ui.rs` 三处添加 ANSI 剥离
- **问题 C — machine_remove stub**:
  - `crates/gui/src/ipc.rs` `machine_remove()` 从硬编码错误改为完整实现：删除服务器 actor + 清理本地配置目录
- **文件**: `crates/agent-runtime/src/interactive.rs`, `crates/agent-runtime/src/command.rs`, `crates/loom-shell/src/log_viewer.rs`, `crates/loom-shell/src/ui.rs`, `crates/gui/src/ipc.rs`
- **测试**: 699/699 测试通过
- **关联**: [[2026-06-18 HANSIONSTATION 删除与 ANSI 显示修复]]

### #7 同频道并发根消息触发 ActiveTurn 覆盖与 session 冲突 ✅ 已修复 (2026-07-07)

- **严重程度**: 🔴 Critical — 同频道两条并发根消息会互相覆盖运行状态，并可能让该会话永久失活
- **影响范围**: 所有 agent worker 的唤醒调度
- **根因**: 调度键（`turn_key`，按 thread-root 族）与执行串行键（`scope.id`）粒度不一致。同一 channel 的两条根消息生成两个 turn_key，`begin_or_enqueue` 允许并发 dispatch，但 `active_turns`、adapter in-flight 槽和 provider session 都以 `scope.id` 为键：第二个 turn 的 `set_turn` 覆盖第一个 turn 的记录；command transport 直接报 "session already in flight" 失败；且失败路径的 `finish_and_next` 会误删仍在运行的第一个 turn 的 active 记录，导致其 Finished 事件匹配不到 turn、run 永不关闭、busy 键永久泄漏——该会话族此后所有触发只入队不派发
- **修复**:
  1. `turn_key_for_trigger` 统一为 `scope:{kind}:{id}`，与执行粒度一一对应（同频道根消息串行排队，不同 thread / DM 对话保持并行；DM 由此从"全部 DM 串行"修正为按对端并行）
  2. 顺带引入唤醒合并（wake coalescing，见 `docs/agent-turn-input-analysis.md` §6.2）：`finish_and_next_batch` 把排队期间积压的同目标、同可见性普通消息合并为一个 turn，一次性回应并逐条 ack delivery；`AgentSpec.wake`（`coalesce`/`debounceMs`/`replyReminder`）可调
  3. Response delivery reminder 按 `replyReminder` 降频（默认首回合全文、后续单行指针），prompt 头新增 `Intent:` 行
- **文件**: `crates/cli/src/cmd/agent_serve.rs`, `crates/proto/src/methods.rs`, `crates/cli/src/cmd/daemon.rs`
- **测试**: loom-cli 391/391、loom-server 123/123、proto 19/19 通过
- **关联**: [[agent-turn-input-analysis]]（docs/agent-turn-input-analysis.md §5.8、§6.2、§6.4-6.6）

## 已关闭

_（暂无）_

## 变更日志

| 日期 | 描述 |
|------|------|
| 2026-07-07 | 统一唤醒调度键为 scope 粒度、新增唤醒合并与 `AgentSpec.wake` 策略、reminder 降频、prompt 增加 Intent 行（loom-cli 391 测试通过） |
| 2026-06-18 | 修复 HANSIONSTATION 无法删除、loom-shell ANSI 乱码、实现 machine_remove（699 测试通过） |
| 2026-06-14 | 修复 Copilot CLI Session 上下文膨胀（instructions_via 注入策略，677 测试通过） |
| 2026-06-14 | 修复频道删除后 Agent 继续执行导致错误循环（新增 CHANNEL_DELETED 处理） |
| 2026-06-14 | 修复 `resolve_actor_alias` 返回僵尸 Actor 导致 `message.send` 失败 |
| 2026-06-14 | 修复 Copilot CLI 首次运行 `--session-id` 与 `--resume` 参数分离 |
| 2026-06-14 | 实现 GUI Agent Env 键值编辑器 |
| 2026-06-13 | 添加 `AgentProviderRef.env` 字段（proto → runtime → CLI） |
