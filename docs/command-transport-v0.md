# Command Transport v0

> **状态**：已落地。Adapter 抽象见 [architecture.md](architecture.md)；
> 实现位于 `crates/agent-runtime/src/command.rs`。本文档作为协议规范保留。

---

## 1. 背景

### 1.1 为什么需要 command transport

[v1 Adapter 抽象](architecture.md) 已经把 ACP 作
为第一类 transport 落地。但现实里很多 agent 不长成 ACP 形态：

- `claude -p "<prompt>"` 一次性 CLI；
- `codex run -p "<prompt>"` 同上；
- `aider --message "<prompt>"`；
- 用户自己写的 shell / python 脚本：`./my-agent.sh "$@"`。

这些工具的共同特征：**一次性子进程，stdin/argv 给 prompt，stdout 出结果，进程退
出 = turn 结束**。它们不暴露 long-lived session 协议。

直接放弃接入这类工具的代价太大：

- 用户已经付费的 LLM CLI（`claude`、`codex`）都属于此类；
- "把 agent 写成 shell 脚本"是最低门槛的扩展方式；
- 内部团队在做 agent 实验时，往往 prefer 一个"事件触发就跑一个脚本"的模型。

### 1.2 与 ACP 的关键差别

| 维度 | ACP transport | Command transport |
| --- | --- | --- |
| 进程生命周期 | long-lived，全程一个 child | per-prompt，每条 prompt 起一个 child |
| 上下文 | child 自己在内存里维护 | **外包给底层 CLI 的 `--resume <session_id>`** |
| 流式输出 | 原生 `session/update` 帧流 | 取决于 CLI 是否支持 stream output（`text` 模式没流，`stream-json` 模式有） |
| 工具调用可见度 | 高（`tool_call` / `tool_call_update` 帧） | 取决于 stream 格式；`text` 模式完全不可见 |
| Permission request | 原生 `session/request_permission` | v0 不支持（command 子进程不便往 loom 反向请求权限） |
| 启动开销 | 一次启动，复用 | 每条 prompt 都冷启动一次 LLM client |

### 1.3 设计原则

1. **不在 loom 内重新发明上下文管理**。LLM CLI 自己已经把 session 持久化做得很好
   （`~/.claude/sessions/`、`~/.codex/sessions/` 等），loom 只簿记
   `(actor, scope) → session_id` 这个最小映射。
2. **schema 必须能描述任意一次性 CLI**。即便用户写一个 echo 脚本也能跑通。
3. **保真度由 CLI 自己决定**。loom 提供 `output_format` 旋钮让用户告诉 adapter
   "我跑的这个 CLI 输出长什么样"，loom 按格式解析；解析不到的细节就当没有，不强
   行造数据。

---

## 2. `AgentTransport` schema 扩展

### 2.1 新增字段

[v0 `AgentTransport`](../crates/proto/src/methods.rs#L393-L406) 在 v1 扩展为：

```rust
pub struct AgentTransport {
    pub kind: String,                  // "acp_stdio" | "command"  ← 增加 "command"
    pub command: String,
    #[serde(default)] pub args: Vec<String>,
    #[serde(default)] pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,

    // ↓ 仅 kind == "command" 时使用，其他 transport 忽略
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<CommandSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<CommandOutputFormat>,
    #[serde(default = "default_prompt_via")]
    pub prompt_via: PromptVia,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,       // hard timeout for one turn
}

pub struct CommandSession {
    /// 第一次调用之后用什么规则把 session_id 抓出来；
    /// 没填表示这个 CLI 不支持 resume，每次都按"first run"跑。
    pub first_run_capture: Option<String>,
    /// 后续调用拼参的模板；占位符 {session_id} {prompt}。
    /// 没填表示不 resume。
    pub resume_args: Option<Vec<String>>,
}

pub enum CommandOutputFormat {
    /// 整段 stdout 当成一条 message（默认）。
    Text,
    /// Anthropic Claude Code `--output-format stream-json` 格式。
    ClaudeStreamJson,
    /// GitHub Copilot CLI `--output-format json` 的 JSONL session event 格式。
    CopilotJson,
    /// OpenAI codex CLI `--output-format stream-json` 格式（占位，schema 待定）。
    CodexStreamJson,
    /// 通用：每行一条 JSON，按 `{"type":"text","text":"..."}` 这种 envelope 翻译。
    NdjsonLines,
}

pub enum PromptVia {
    /// prompt 作为 args 的最后一个 token 追加（适合 `claude -p <prompt>`）。
    Args,
    /// prompt 写到子进程 stdin，进程读完关闭。
    Stdin,
    /// prompt 写到 env var LOOM_PROMPT。
    Env,
}

fn default_prompt_via() -> PromptVia { PromptVia::Args }
```

### 2.2 完整 JSON schema 块

```json
{
  "provider": {
    "id": "claude_cmd",
    "displayName": "Claude Code Command"
  },
  "transport": {
    "kind": "command",
    "command": "claude",
    "args": ["-p"],
    "env": {
      "ANTHROPIC_API_KEY": "{env.ANTHROPIC_API_KEY}"
    },
    "session": {
      "firstRunCapture": "stdout_json:.session_id",
      "resumeArgs": ["--resume", "{session_id}", "-p"]
    },
    "outputFormat": "claude_stream_json",
    "promptVia": "args",
    "timeoutMs": 900000
  },
  "defaults": {
    "autostart": true
  },
  "actors": [
    {
      "id": "actor_claude_cmd",
      "displayName": "Claude (command mode)"
    }
  ]
}
```

模板变量：`{agent.workspace}` / `{agent.profile}` / `{agent.logs}` /
`{agent.root}` / `{channel.root}` / `{channel.shared}` /
`{channel.sharedArtifacts}`。`{agent.root}` 不表示 `{agent.profile}`，
也不表示进程级 `$HOME`。
`{agent.profile}` 仍然专指 identity / memory / MCP 配置等 actor 持久状态目录。

bundle 相关变量为：`{agent.bundle_root}`、`{agent.bundle}`。它们表示 runtime
管理的版本化资产目录，和 profile 平级，典型目录结构为：

```text
{agent.root}/
  workspace/
  profile/
  logs/
  bundles/
```

再加上 v1 新增的 `{env.NAME}`（从 agent client 进程 env 取值）。

---

## 3. Session 簿记

### 3.1 数据结构

```text
~/.agentx/sessions/<actor_id>/<scope_id>.json
```

每个文件：

```json
{
  "actor_id": "actor_claude_cmd",
  "scope": { "kind": "thread", "id": "thr_abc123" },
  "session_id": "01J6Q0R5K3HZ2A1XW8Y9P7T6FE",
  "created_at": "2026-04-21T03:14:22Z",
  "last_used_at": "2026-04-21T03:18:55Z",
  "command_signature": "claude:sha256:abcd…"
}
```

字段说明：

- **`scope`**：loom 协议里的 scope（channel 或 thread）。一个 thread 的对话历史
  对应底层 CLI 的一个 session。
- **`session_id`**：从底层 CLI 启动输出里抓出来的那串 ID（`first_run_capture`
  规则解出）。loom 不解释它，仅作为 `--resume` 的入参。
- **`command_signature`**：`command` + `args` 模板（替换前）的哈希。如果用户改了
  spec 里的 command，loom 应该作废旧映射并按 first-run 重跑——避免把"用 model A
  起的 session" 喂给"现在配置成 model B 的 CLI"。

### 3.2 失效处理

调 `--resume <session_id>` 失败的情况（CLI 报"session not found"、用户外部清了
`~/.claude/sessions/`、CLI 升级了 schema）：

1. Adapter 捕获子进程非零退出 + stderr 包含 `"session not found"` 之类信号；
2. 删除对应 `<scope_id>.json`；
3. 回退按"first run"重跑一次（不带 `--resume`），重新抓 session_id；
4. 如果二次仍然失败，发 `AdapterEvent::Error`，loom 端写 `error` trace 帧。

> **失效信号清单**（adapter 里硬编码或 spec 配置）：
> - 退出码非 0
> - stderr 匹配下列任一正则（默认）：
>   - `session\s+not\s+found`
>   - `unknown\s+session`
>   - `no\s+such\s+session`
> - 第一次 first-run 抓不到 session_id（说明配置有错，不算"失效"，直接报 Error
>   不再回退）

### 3.3 并发保护

同一个 `(actor_id, scope_id)` 同一时刻只允许一个 in-flight 子进程。Adapter 内部
持一个 `tokio::sync::Mutex<()>`，per-key 锁住整个 "spawn → 抓 session_id → 写文
件 → wait" 的过程。

---

## 4. `first_run_capture` DSL

第一次调用（spec 里没记 session_id 时）跑完之后，loom 需要把底层 CLI 给出的
session_id 抓回来。规则形态有三种：

### 4.1 `stdout_json:<jq-style-path>`

适用于：CLI 在 stdout 输出一行（或最后一行）JSON，里面有 session_id。

```
first_run_capture: "stdout_json:.session_id"
```

行为：

1. 把整个 stdout 收集起来；
2. 先尝试整体 parse 为 JSON；
3. 失败则按行扫，找最后一行能 parse 的 JSON；
4. 用 jq-style path（仅支持 `.field.subfield` 与数组下标 `.items[0].id`，**不**
   支持 filter / pipe / 函数）取值；
5. 取到的值必须是 string 或 number，否则视为 capture 失败。

### 4.2 `stderr_regex:<regex>`

适用于：CLI 把 session 信息打到 stderr 的友好消息里。

```
first_run_capture: "stderr_regex:Session: ([a-f0-9-]{36})"
```

行为：必须有且仅有一个 capture group，整段 stderr 跑 regex.find，取第一组。

### 4.3 `file:<path>`

适用于：CLI 把 session_id 写在一个固定文件里。

```
first_run_capture: "file:{agent.profile}/last_session_id"
```

行为：进程退出后读文件内容，trim 空白后当作 session_id。

### 4.4 模板变量

`first_run_capture` 与 `resume_args` / `args` / `env` 共享同一套变量替换，在
spec 加载时不展开（因为部分变量依赖 scope），运行时按需展开。进程 cwd 不在
spec 里配置，由 runtime 固定设为当前 channel 下该 actor 的 workspace：

| 变量 | 来源 |
| --- | --- |
| `{actor.id}` | 当前 actor id |
| `{scope.id}` | 当前 scope id（trigger message 的 scope） |
| `{scope.kind}` | `"thread"` 或 `"channel"` |
| `{agent.workspace}` | `~/.agentx/channels/<channel-id>/agents/<actor-id>/workspace` |
| `{agent.profile}` | `~/.agentx/agents/<actor-id>/profile`（per-actor 持久化状态：identity / memory / MCP 配置等） |
| `{agent.logs}` | `~/.agentx/channels/<channel-id>/agents/<actor-id>/logs` |
| `{agent.root}` | `~/.agentx/channels/<channel-id>/agents/<actor-id>` |
| `{agent.bundle_root}` | `~/.agentx/agents/<actor-id>/bundles`（runtime 管理的版本化 bundle 根目录） |
| `{agent.bundle}` | 当前激活 bundle 的目录（通常是 `bundles/current` 指向的版本目录） |
| `{channel.root}` | `~/.agentx/channels/<channel-id>` |
| `{channel.shared}` | `~/.agentx/channels/<channel-id>/shared` |
| `{channel.sharedArtifacts}` | `~/.agentx/channels/<channel-id>/shared/artifacts` |
| `{env.NAME}` | agent client 进程的 env var |
| `{session_id}` | 仅 `resume_args` 可用 |
| `{prompt}` | 仅 `resume_args` / `args`（当 `prompt_via=args`）可用 |

---

## 5. `resume_args` 模板

第二次及之后的调用使用 `resume_args` 拼接：

```json
"resume_args": ["--resume", "{session_id}", "-p"]
```

实际命令行 = `command` + `resume_args`（替换占位符之后）+（视 `prompt_via`）prompt
本身。

### 5.1 `prompt_via` 与命令行拼接

| `prompt_via` | first run 命令行 | resume 命令行 |
| --- | --- | --- |
| `args` | `command` + `args` + `prompt` | `command` + `resume_args`（其中 `{prompt}` 展开为实际文本） |
| `stdin` | `command` + `args`，prompt 写 stdin | `command` + `resume_args`（无 `{prompt}`），prompt 写 stdin |
| `env` | `command` + `args`，env `LOOM_PROMPT=<text>` | `command` + `resume_args`，env `LOOM_PROMPT=<text>` |

### 5.2 空 prompt 的处理

不应该出现"directed-message 事件没带文本"的情况——`loom-daemon` 的
`render_prompt` 会用 fallback（`text` → `message` → 整个 payload JSON）保证非空。
但 adapter 要做防御性检查：

- prompt 为空字符串时，发 `AdapterEvent::Error{ message: "empty prompt" }` 并跳
  过 spawn——不假装跑了一次。

### 5.3 不支持 resume 的 CLI

`session.first_run_capture` 与 `session.resume_args` 都不填时，每次调用都按 first
run 跑——也就是每次都是"裸 prompt，无上下文"。这等价于"无记忆 agent"。loom 仍然
会按 thread 维护 turn / event 历史，但 CLI 自己看不到上一轮。

---

## 6. `output_format` 与 `AdapterEvent` 映射

### 6.1 `text`

最朴素：把整个 stdout 读完，进程退出后发：

```
AdapterEvent::Text { content: <stdout 全文>, is_partial: false }
AdapterEvent::Finished { success: <exit==0>, summary: "" }
```

不发任何 `ToolUse` / partial Text / trace 帧。Owner 视图只能看到最终消息。

### 6.2 `claude_stream_json`

Anthropic Claude Code `--output-format stream-json` 输出长这样（每行一条 JSON）：

```json
{"type":"system","subtype":"init","session_id":"01J6Q0R5K3HZ2A1XW8Y9P7T6FE","model":"claude-sonnet-4"}
{"type":"assistant","message":{"content":[{"type":"text","text":"Let me check the file."}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file":"/tmp/x"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"contents..."}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"The file says hello."}]}}
{"type":"result","subtype":"success","session_id":"01J6Q0R5K3HZ2A1XW8Y9P7T6FE","total_cost_usd":0.0123}
```

翻译表：

| 输入帧 | `AdapterEvent` |
| --- | --- |
| `type=system, subtype=init` | （不发；`session_id` 由 `first_run_capture: stdout_json:.session_id` 抓走） |
| `type=assistant, content[].type=text` | `Text { content, is_partial: true }`（每帧一次） |
| `type=assistant, content[].type=tool_use` | `ToolUse { tool_name=name, input }` |
| `type=user, content[].type=tool_result` | （不发；loom 不需要看到 tool 结果，那是 CLI 内部的事） |
| `type=result, subtype=success` | 流末尾再补 `Text { is_partial: false }`（空字符串）触发 flush；然后 `Finished { success: true, summary: total_cost_usd }` |
| `type=result, subtype=error_*` | `Finished { success: false, summary: error_message }` |

注意：`AdapterEvent::Text` 的 `is_partial` 语义是"非 partial 即 flush 信号"。
stream-json 没有显式 flush，所以约定"`type=result` 之前所有的 text 都按 partial
累积，遇到 result 时 emit 一次 `is_partial=false` 的空 text 强制 flush"。

### 6.3 `copilot_json`

GitHub Copilot CLI `--output-format json --stream off` 输出 JSONL session
events。loom 只把根 agent 的最后一条 `assistant.message.data.content` 当成最终
回答发给用户；`tool.*`、`assistant.reasoning*`、带 `agentId` 的 sub-agent 事件
都保留在子进程 stdout 日志里，不转换成用户可见消息。

如果只收到 `assistant.message_delta.data.deltaContent` 而没有完整
`assistant.message`，adapter 会把 delta 拼接后作为 fallback 输出。

### 6.4 `codex_stream_json`

OpenAI codex CLI 也支持 stream-json 但 schema 不同。具体 envelope 待确认；占位映
射：

| 输入帧 | `AdapterEvent` |
| --- | --- |
| `{"type":"output_text.delta","delta":"..."}` | `Text { content: delta, is_partial: true }` |
| `{"type":"output_text.done"}` | `Text { content: "", is_partial: false }` |
| `{"type":"tool_call","name":"...","arguments":...}` | `ToolUse` |
| `{"type":"response.completed","status":"completed"}` | `Finished { success: true, summary: "" }` |

E2 阶段实现时再根据实际 codex 输出 fix schema。

### 6.5 `ndjson_lines`

最通用、给"自己写脚本"的人用：

每行一条 JSON，按 envelope 翻译：

```json
{"type":"text","text":"hello"}
{"type":"text","text":" world"}
{"type":"tool","name":"web.search","input":{"q":"x"}}
{"type":"done","ok":true}
```

| `type` | `AdapterEvent` |
| --- | --- |
| `text` | `Text { content: text, is_partial: true }` |
| `tool` | `ToolUse { tool_name: name, input }` |
| `status` | `StatusChange { status }` |
| `error` | `Error { message }` |
| `done` | `Text { is_partial: false }` 强制 flush + `Finished { success: ok, summary: "" }` |

不能 parse 的行被静默 drop（但写入 agent log），不会让一行错误格式毁了整次调用。

### 6.6 子进程 stderr

不论 `output_format` 取何值，stderr 始终：

- 行式追加到该 agent 的 log（`~/.local/share/loom/agents/<actor>/logs/run.log`）；
- 不进 `AdapterEvent` 流；
- 用户直接查看 daemon data root 下的 agent log 文件。

例外：`first_run_capture: stderr_regex:...` 时，stderr 同时被 regex 扫描以抓
session_id。

---

## 7. Worked Example: `claude -p`

### 7.1 完整 spec

```json
{
  "provider": {
    "id": "claude_cmd",
    "displayName": "Claude Code Command"
  },
  "transport": {
    "kind": "command",
    "command": "claude",
    "args": ["-p", "--output-format", "stream-json", "--verbose"],
    "env": {},
    "session": {
      "firstRunCapture": "stdout_json:.session_id",
      "resumeArgs": ["--resume", "{session_id}", "-p", "--output-format", "stream-json", "--verbose"]
    },
    "outputFormat": "claude_stream_json",
    "promptVia": "args"
  },
  "defaults": {
    "autostart": true
  },
  "actors": [
    {
      "id": "actor_claude_cmd",
      "displayName": "Claude Code (command)"
    }
  ]
}
```

### 7.2 第一次调用

人在 thread `thr_abc` 里发 `message` + `directed_to=actor_claude_cmd`。

Agent client 查 `~/.agentx/sessions/actor_claude_cmd/thr_abc.json`：
**不存在**。

走 first run 路径：

```bash
cd ~/.local/share/loom/agents/actor_claude_cmd/workspace
claude -p --output-format stream-json --verbose "<prompt 文本>"
```

抓输出第一行的 `session_id`，写到 `thr_abc.json`：

```json
{
  "actor_id": "actor_claude_cmd",
  "scope": {"kind":"thread","id":"thr_abc"},
  "session_id": "01J6Q0R5K3HZ2A1XW8Y9P7T6FE",
  "created_at": "2026-04-21T03:14:22Z",
  "last_used_at": "2026-04-21T03:14:22Z",
  "command_signature": "sha256:f17a..."
}
```

stream-json 翻译为 `AdapterEvent`，loom 端 turn 跑完写若干 trace 帧 + 一条
`message` + `turn.close`。

### 7.3 同一 thread 的第二条 prompt

人再发一条 directed-message。Agent client 查文件：**存在**。

走 resume 路径：

```bash
cd ~/.local/share/loom/agents/actor_claude_cmd/workspace
claude --resume 01J6Q0R5K3HZ2A1XW8Y9P7T6FE -p --output-format stream-json --verbose "<新 prompt>"
```

更新 `thr_abc.json` 的 `last_used_at`。

### 7.4 失效场景

用户外面手动删了 `~/.claude/sessions/01J6Q.../`。下一次 resume：

```bash
claude --resume 01J6Q0R5K3HZ2A1XW8Y9P7T6FE -p ...
# stderr: Error: session not found: 01J6Q...
# exit code: 1
```

Adapter 检测到（"session not found" 正则匹配）→ 删 `thr_abc.json` → 立刻按 first
run 重跑一次，抓新的 session_id。Owner 视图上只看到一条 trace 帧
`status: session expired, restarted`，主流程不中断。

---

## 8. Worked Example: 任意 shell script

### 8.1 一个最简 echo 脚本

```bash
#!/usr/bin/env bash
# ~/agents/echo-back.sh
set -euo pipefail

prompt="$1"
echo "{\"type\":\"text\",\"text\":\"You said: $prompt\"}"
echo "{\"type\":\"text\",\"text\":\" (in scope $LOOM_SCOPE_ID)\"}"
echo "{\"type\":\"done\",\"ok\":true}"
```

### 8.2 对应 spec

```json
{
  "provider": {
    "id": "echo",
    "displayName": "Echo Command"
  },
  "transport": {
    "kind": "command",
    "command": "/Users/me/agents/echo-back.sh",
    "args": [],
    "env": {},
    "outputFormat": "ndjson_lines",
    "promptVia": "args"
  },
  "defaults": {
    "autostart": true
  },
  "actors": [
    {
      "id": "actor_echo",
      "displayName": "Echo Bot"
    }
  ]
}
```

注意：

- 没填 `session.*` → 不走 resume，每次都是 first-run。echo bot 本来就无状态。
- `output_format: ndjson_lines` → 每行 JSON 按 §6.5 翻译。
- `prompt_via: args` 默认 → prompt 作为 `$1` 传进来。

### 8.3 注入给脚本的 env vars

Runtime 会为 command 子进程补齐以下默认环境变量。spec 里的同名 `env` 值优先；
`LOOM_SERVER` 默认会尽量改写为 loopback 地址，避免本机 agent sandbox 不能访问网卡
IP。也可以用 `LOOM_AGENT_SERVER` 显式覆盖。

| 环境变量 | 值 |
| --- | --- |
| `LOOM_SERVER` | 子进程 shell out 回 loom 时使用的 WS URL |
| `LOOM_ACTOR` | 当前 agent 的 actor id |
| `LOOM_SCOPE_ID` | 当前 turn 的 thread/channel scope id |
| `LOOM_SCOPE_KIND` | 当前 turn 的 scope kind：`thread` 或 `channel` |
| `LOOM_AGENT_PROFILE` | per-actor profile 目录 |
| `LOOM_AGENT_BUNDLE_DIR` | 当前 bundle 目录 |
| `AGENTX_CHANNEL_ID` | 当前 channel id |
| `AGENTX_CHANNEL_ROOT` | 当前 channel 根目录 |
| `AGENTX_CHANNEL_SHARED` | 当前 channel shared 目录 |
| `AGENTX_CHANNEL_SHARED_ARTIFACTS` | 当前 channel artifacts 目录 |
| `AGENTX_AGENT_ROOT` | 当前 channel 下该 agent 的私有根目录 |
| `AGENTX_AGENT_WORKSPACE` | 当前 channel 下该 agent 的默认 workspace |
| `AGENTX_AGENT_LOGS` | 当前 channel 下该 agent 的日志目录 |
| `PATH` | 继承 `loom-daemon` 进程的 PATH |

### 8.4 脚本里 shell out 回 loom

因为 `LOOM_SERVER` / `LOOM_ACTOR` / `PATH` 都齐了，脚本可以直接：

```bash
loom --json message read --in "$LOOM_SCOPE_ID" --limit 20
loom --json message read --in "$LOOM_SCOPE_ID" --channel --limit 20
loom --json artifact publish --name plan.md --text "$plan_body"
```

形成 "command agent 通过 cli 反向读写 server" 的闭环。这是 command transport 的
观察性下界——即便不支持 stream-json，至少 agent 还能通过 cli 主动汇报。

---

## 9. Caveats

设计上必须摆出来的限制，避免后续争议：

### 9.1 Trace 保真度

| 模式 | text.delta | tool.start/update/end | status | error |
| --- | --- | --- | --- | --- |
| ACP | ✓ | ✓ | ✓ | ✓ |
| Command + `text` | ✗ | ✗ | ✗ | ✓（exit≠0 时） |
| Command + `claude_stream_json` | ✓ | ✓（only `tool.start`） | ✗ | ✓ |
| Command + `ndjson_lines` | ✓ | ✓ | ✓ | ✓ |

→ 选 `text` 模式的用户应当预期 owner 视图只有最终消息。

### 9.2 启动开销

Command transport 每条 prompt 都重启一次 LLM client（解释器加载、auth 握手、
session restore）。对话频繁时 latency 比 ACP 高一个数量级。

→ 文档里建议高频对话 prefer ACP；偶发 / 批量处理 prefer command（可以利用 LLM
provider 的 prompt cache）。

### 9.3 Permission request 不可表达

ACP 有 `session/request_permission` → loom `action.request` 的反向通道。Command
子进程没法在 prompt 跑到一半暂停回头问人。

v0 决定：command transport 不发 `AdapterEvent::ActionRequest`，整个 turn 是"prompt
进、结果出"的同步往返。如果某个 command CLI 真的需要权限审批，应该让它的开发者
做成 ACP transport。

agent 主动需要人参与时不走 adapter permission 通道，而是 shell out 到 Loom 的
human-interaction 工具：`loom ask-user-question` 用于选择/补信息，
`loom request-approval` 用于批准/拒绝。命令自己 append `action.request`，阻塞
等待 `action.response`，然后把结果返回给当前 agent 进程；daemon worker 只广播
这条响应，不把 `loom:question:*` / `loom:approval:*` 回传给 adapter。

### 9.4 Session id 不是 loom 的概念

loom 不在 protocol 层面承认 session_id 的存在——它纯粹是 command transport 内部
状态。其他 transport（ACP、未来的 MCP）有自己的 session 抽象，但都不会泄漏到
event/turn 模型里。

### 9.5 命令外部修改

CLI 升级、用户 `rm -rf ~/.claude/`、底层 session schema 变更，都会让 loom 簿记的
session_id 突然作废。adapter 必须能优雅降级（§3.2 失效处理）。

### 9.6 命令注入

`prompt_via: args` 把模型输出（文本）直接拼到 argv 里，看似有命令注入风险——
但这条 argv 经过 `Command::arg(prompt)`（rust std）传给操作系统，**不**走 shell
解释，所以安全。`prompt_via: stdin` 完全绕开 argv，更稳。**禁止**实现把 prompt
拼成 shell string 后 `sh -c`：spec 里 `command` 字段就是裸 binary 名。

### 9.7 Output format 不能动态切换

`output_format` 是 spec 静态字段。同一个 actor 不能"小消息走 text，大消息走
stream-json"——切换需要改 spec 重启。

---

## 10. 与 architecture 的对应关系

| 本文档章节 | architecture 章节 |
| --- | --- |
| §2 schema 扩展 | §4.4（`transport.kind` 取值表） |
| §4 / §5 / §6 翻译规则 | §4.2（AdapterEvent）+ §6.2（translate_event 复用） |
| §3 / §7 session 簿记 | §7 Phase E2 的"覆盖 claude -p 端到端"验收点 |
| §8 shell script | §3 进程边界（agent client cli 提供 PATH 注入） |

E2 的验收 = 本文档 §7（claude -p 完整跑通）+ §8（最小 shell 脚本跑通）+ §9.1
trace 保真度表得到验证。
