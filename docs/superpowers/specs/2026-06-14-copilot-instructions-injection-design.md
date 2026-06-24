标题：Copilot CLI 指令注入方式改造设计 ✅ 已实施 (2026-06-14)

## 背景

Copilot CLI 不支持 `--system-prompt` 参数（与 Codex/CC 不同），LOOM 通过 `-p "{prompt.full}"` 将所有内容传入。每次 `--resume`，copilot 的 events.jsonl 都会新增一条 user.message（~47KB），其中 40KB 为固定不变的 `agent_instructions`。52 轮对话后 session 膨胀至 10MB/1769 条事件，传递给模型 API 时超时。

## 方案：Provider manifest 注入策略

在 `ProviderModeSpec` 中新增 `instructions_via` 字段，声明指令注入方式：
- `"prompt"`（默认）：指令通过 prompt 文本注入（Codex/CC 用 `--append-system-prompt`）
- `"agents_md"`：指令写入工作区 `AGENTS.md`，由 Copilot CLI 自动加载

## 改动文件

1. `crates/proto/src/methods.rs` - `ProviderModeSpec` and `AgentTransport` 新增 `instructions_via`
2. `crates/agent-runtime/src/provider.rs` - `copilot_manifest()` 设 `instructions_via: "agents_md"`, `ProviderRuntimePlan` 新增字段，数据流传递
3. `crates/agent-runtime/src/agents_md.rs` - `ensure_agents_md` 扩展，接受可选的 `agent_instructions` 和 `actor_context`
4. `crates/cli/src/cmd/agent_serve.rs` - `WorkerState` 新增 `instructions_via`, `compose_envelope_prompt` 跳过指令段落, `ensure_scope` 传参

## 数据流

```
AgentSpec.instructions
       │
       ▼
ensure_scope() → ensure_agents_md(workspace, actor_id, instructions, actor_context)
       │                                          │
       │  instructions_via == "agents_md"         ▼
       │  写入完整指令到 AGENTS.md               copilot 自动加载
       │
       ▼
compose_envelope_prompt()
       │  instructions_via == "agents_md"
       │  跳过 agent_instructions + actor_context section
       │
       ▼
prompt.content（无 40KB 固定指令，仅有变化的对话上下文）
       │
       ▼
copilot -p "{prompt.full}" --add-dir <workspace>/
       │  copilot 自动加载 AGENTS.md → 获得完整指令
       ▼
events.jsonl: user.message ~5KB（减少 89%）
```

## Prompt Templates 兼容性

`compose_envelope_prompt` 在组装 sections 之前根据 `instructions_via` 决定是否包含 `agent_instructions` 和 `actor_context`。跳过的段落不会进入 `prompt.content`，因此也不会进入 `{prompt.full}`。Prompt Template 的 prefix/suffix 注入在 section 组装之后，不受影响。

## 测试计划

1. 编译验证：`cargo build`
2. 现有测试：`cargo test` 确认无回归
3. 手动验证：启动 Copilot CLI agent，检查 AGENTS.md 是否包含完整指令，检查 events.jsonl user.message 大小是否显著减少

## 边缘情况

- `instructions_via` 为空或未知值时，默认按 `"prompt"` 处理
- `agent_instructions` 为空时，AGENTS.md 仅包含 actor_context
- 用户手动编辑 AGENTS.md loom block 外的内容不受影响（`<!-- BEGIN loom -->` / `<!-- END loom -->` 标记保护）

## 实施状态

- ✅ 编译通过 (`cargo build`)
- ✅ 全部 677 测试无回归 (`cargo test`)
- ✅ 手动验证通过：AGENTS.md (27KB) 包含完整指令 + actor_context；copilot prompt 不含 agent_instructions；copilot 进程 cwd 为 workspace 目录，默认加载 AGENTS.md
