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

## 已关闭

_（暂无）_

## 变更日志

| 日期 | 描述 |
|------|------|
| 2026-06-14 | 修复 Copilot CLI 首次运行 `--session-id` 与 `--resume` 参数分离 |
| 2026-06-14 | 实现 GUI Agent Env 键值编辑器 |
| 2026-06-13 | 添加 `AgentProviderRef.env` 字段（proto → runtime → CLI） |
