# Interactive Command Agent Transport 设计

> 目标：为 Claude / Copilot 这类可用 CLI session id 恢复、但不一定以进程退出作为单轮结束信号的 agent，新增一个 Loom 托管的 `interactive_command` transport。

## 1. 背景

Loom 当前的 agent runtime 主要有两类 transport：

| Transport | 进程形态 | 单轮结束信号 | Session 归属 |
| --- | --- | --- | --- |
| `acp_stdio` | 长驻 ACP 子进程 | ACP 协议事件，例如 `session/update` / stop reason | ACP session |
| `command` | 每个 prompt 启一个子进程 | 子进程退出 | Loom 记录 `(actor, scope) -> provider session_id` |

这两类都依赖一个明确的结束边界：

- ACP 由协议告诉 Loom 何时完成。
- `command` 由进程退出告诉 Loom 何时完成。

但 Claude / Copilot 的某些 CLI 用法更像：

```sh
# Claude：首次创建
claude "<prompt>" --session-id <uuid>

# Claude：恢复
claude "<prompt>" --resume <session_id>

# Copilot：首次/恢复都可能走 resume
copilot "<prompt>" --resume <session_id>
```

这类命令可以接收 prompt 和 session id，但进程可能进入交互式状态，或者输出中没有机器可读的“完成”帧。Loom 需要自己定义：

1. 何时创建 provider session。
2. 何时 resume provider session。
3. 如何告诉 agent 回复格式和完成标识。
4. 如何从输出里判断本轮结束。
5. 如何在完成、取消、超时时终止进程。

因此新增 `interactive_command`，不要把这些语义混入现有 `command`。

## 2. 设计原则

### 2.1 Scope 是 session 边界

Loom 的协作作用域只有两种：

```text
ScopeRef =
  thread:<thread_id>
  channel:<channel_id>
```

这里的 `thread:<thread_id>` / `channel:<channel_id>` 是 runtime 内部 session
边界，不是 agent-facing message target。CLI target 的 canonical 形式是
`#<channel_id>` 或 `#<channel_id>:<root_message_id>`；thread 由 channel 公共区
的一条 root message 锚定，不能嵌套。

`interactive_command` 的 provider session 生命周期直接跟随 `ScopeRef`：

```text
(actor_id, scope.kind, scope.id) -> provider_session_id
```

也就是说：

- 一个 agent 对一个 thread 默认只有一个 provider session。
- 一个 agent 对一个 channel 公共区默认只有一个 provider session。
- thread session 和 channel 公共区 session 不共享。
- 同一个 thread 中不同 agent 不共享 session。
- 配置签名变化或用户 reset 时，新建 session。

### 2.2 Server 不托管 agent 进程

`loom-server` 继续只做 message hub：

- actor connection
- scope subscribe
- event / turn / trace / artifact store
- fanout

agent 进程仍由 `loom agent serve` 托管。`interactive_command` 只是在 agent runtime 里新增一种 adapter。

### 2.3 Scope skills 是 workspace 能力，不是 provider session 状态

为了兼容 classroom / 多 agent 场景，Loom server 可以把 channel 成员 actor 的已发布 bundle 投影到 scope 级 `skills/` 目录：

```text
data/workspaces/channel/{channel_id}/skills/{actor_id} -> data/agents/{actor_id}/bundle source
data/workspaces/thread/{thread_id}/skills/{actor_id}  -> data/agents/{actor_id}/bundle source
```

当前实现采用 file-backed 方案：server 读取 `data/agents/{actor_id}/bundle-release.json` 的 `source` 字段作为 symlink target。这个数据源被隔离在 actor skill source 解析层，未来可以扩展为 agent serve 通过 RPC 上报的 registry-backed 方案，而不需要重写 scope projection 规则。

`loom agent serve` 在创建 scope workspace 时会把对应 scope skills 桥接进 agent 当前工作目录：

```text
{agent workspace}/skills -> {scope workspaces root}/{scope.kind}/{scope.id}/skills
```

默认 `{scope workspaces root}` 是 `LOOM_AGENT_DATA_ROOT/workspaces`。当 `loom-server --data-dir` 和 `LOOM_AGENT_DATA_ROOT` 不是同一个目录时，可以通过 `LOOM_SCOPE_WORKSPACES_ROOT` 显式指向 server 的 `data/workspaces`。这保证 `interactive_command` provider 从 `cwd` 看见的是当前 scope 的 skills，而 provider session record 仍按 `(actor, scope)` 独立管理。

### 2.4 完成信号必须显式

不能只靠 idle timeout 判断完成。慢模型输出、工具调用等待、网络抖动、等待用户输入，都可能表现为“暂时没输出”。

默认完成机制应是 prompt contract + sentinel：

```text
When your final user-visible answer is complete, output this exact marker on a line by itself:
__LOOM_DONE__
Do not output anything after the marker.
```

adapter 看到 sentinel 后：

1. 认为本轮完成。
2. 从最终用户可见输出中移除 sentinel。
3. flush 文本为 `message`。
4. emit `AdapterEvent::Finished`。
5. 按 kill policy 结束子进程。

## 3. 总体架构

```text
Human / GUI / TUI
      │
      │ message.send(message + directed_to)
      ▼
┌─────────────────────┐
│ loom-server          │
│ - journal/store     │
│ - delivery/fanout   │
│ - turn/trace        │
└──────────┬──────────┘
           │ actor inbox / scope update
           ▼
┌──────────────────────────────────────────────┐
│ loom agent serve                              │
│                                              │
│  Worker(actor_claude)                        │
│    ├─ resolve Loom scope                      │
│    ├─ resolve/create provider session id     │
│    ├─ compose Loom prompt envelope            │
│    ├─ append completion contract             │
│    └─ InteractiveCommandAdapter              │
│         ├─ spawn claude/copilot              │
│         ├─ read output                       │
│         ├─ detect __LOOM_DONE__               │
│         ├─ strip/normalize output            │
│         └─ kill child according to policy    │
└──────────────────────────────────────────────┘
```

## 4. Transport schema 草案

### 4.1 Claude 示例

```jsonc
{
  "actor": {
    "id": "actor_claude_interactive",
    "kind": "agent",
    "displayName": "Claude Interactive",
    "capabilities": {}
  },
  "transport": {
    "kind": "interactive_command",
    "command": "claude",
    "args": [],
    "env": {},
    "model": "claude-sonnet-4.6",

    "session": {
      "idStrategy": "loom_uuid_per_scope",
      "newArgs": ["{prompt}", "--session-id", "{session_id}"],
      "resumeArgs": ["{prompt}", "--resume", "{session_id}"],
      "onMissing": "create",
      "onSignatureChanged": "create",
      "onResumeFailed": "fail"
    },

    "prompt": {
      "template": "{loom_envelope}",
      "completionContract": {
        "sentinel": "__LOOM_DONE__",
        "instruction": "When your final user-visible answer is complete, output __LOOM_DONE__ on a line by itself. Do not output anything after it."
      }
    },

    "completion": {
      "detect": "sentinel",
      "stripSentinel": true,
      "idleTimeoutMs": 30000,
      "maxTurnMs": 900000
    },

    "output": {
      "stripAnsi": true,
      "stderr": "trace",
      "streamPartial": true
    },

    "kill": {
      "onComplete": { "action": "sigterm", "graceMs": 3000, "fallback": "sigkill" },
      "onCancel": { "action": "sigterm", "graceMs": 1000, "fallback": "sigkill" },
      "onTimeout": { "action": "sigkill" }
    },

    "provider": {
      "kind": "claude",
      "settings": {
        "mode": "actor_profile"
      }
    }
  },
  "autostart": false
}
```

### 4.2 Copilot 示例

```jsonc
{
  "actor": {
    "id": "actor_copilot_interactive",
    "kind": "agent",
    "displayName": "Copilot Interactive",
    "capabilities": {}
  },
  "transport": {
    "kind": "interactive_command",
    "command": "copilot",
    "model": "gpt-5.5",
    "session": {
      "idStrategy": "loom_uuid_per_scope",
      "newArgs": ["{prompt}", "--resume", "{session_id}"],
      "resumeArgs": ["{prompt}", "--resume", "{session_id}"],
      "onMissing": "create",
      "onSignatureChanged": "create",
      "onResumeFailed": "fail"
    },
    "completion": {
      "detect": "sentinel",
      "sentinel": "__LOOM_DONE__",
      "stripSentinel": true,
      "maxTurnMs": 900000
    },
    "kill": {
      "onComplete": { "action": "sigterm", "graceMs": 3000, "fallback": "sigkill" }
    }
  },
  "autostart": false
}
```

Copilot 的首次创建语义需要用真实 CLI 验证。如果 `copilot --resume <uuid>` 对不存在的 session id 不会自动创建，则应调整 `newArgs` 为 Copilot 实际支持的新建命令。

### 4.3 Model 参数

`interactive_command` 允许在 transport 或现有 Loom model selection 中解析出一个 active model。规则建议保持简单：

- 如果 active model 为空，不给 provider CLI 增加任何 model 参数。
- 如果 active model 非空，adapter 在解析完 `newArgs` / `resumeArgs` 后追加一个 argv token：`--model=<model>`。
- model 参数参与 session signature；model 变化默认新建 provider session。

示例：

```jsonc
{
  "transport": {
    "kind": "interactive_command",
    "command": "claude",
    "model": "claude-sonnet-4.6",
    "session": {
      "newArgs": ["{prompt}", "--session-id", "{session_id}"],
      "resumeArgs": ["{prompt}", "--resume", "{session_id}"]
    }
  }
}
```

首次调用实际 argv：

```text
claude "<prompt>" --session-id "<session_id>" --model=claude-sonnet-4.6
```

后续调用实际 argv：

```text
claude "<prompt>" --resume "<session_id>" --model=claude-sonnet-4.6
```

如果未配置 model：

```text
claude "<prompt>" --resume "<session_id>"
```

这里使用单 token `--model=xxx`，避免不同 provider 对 `--model xxx` 两 token 形式解析不一致。若某个 provider 不支持 `--model=xxx`，可以在后续扩展 provider-specific `modelArgTemplate`，但默认行为先固定为 `--model=<model>`。

## 5. Session lifecycle

### 5.1 默认规则

```text
session_key =
  actor_id
  + scope.kind
  + scope.id
  + session_signature
```

默认行为：

| 场景 | 行为 |
| --- | --- |
| actor 第一次在 thread 中被 directed message | 创建 provider session |
| actor 再次在同一个 thread 中被 directed message | resume 同一个 provider session |
| actor 在另一个 thread 中被 directed message | 创建另一个 provider session |
| actor 在 channel 公共区被 directed message | 创建/恢复该 channel scope 的 provider session |
| 同一个 thread 中 directed message 给另一个 actor | 使用另一个 provider session |
| command/model/settings/prompt contract 变化 | 新建 provider session |
| resume 失败 | 默认 fail，不自动重跑 |
| 用户显式 reset | 删除映射，下次新建 |

### 5.2 持久化记录

建议沿用当前 agent data root 下的 session 目录，扩展记录结构：

```text
~/.agentx/sessions/<actor_id>/<scope_kind>-<scope_id>.json
```

示例：

```jsonc
{
  "actorId": "actor_claude_interactive",
  "scope": {
    "kind": "thread",
    "id": "thread_123"
  },
  "transportKind": "interactive_command",
  "provider": "claude",
  "sessionId": "018f6d96-4f45-7a3e-8e20-6f3c7f6a9a02",
  "createdAt": "2026-05-02T14:30:00Z",
  "lastUsedAt": "2026-05-02T14:45:00Z",
  "signature": {
    "command": "sha256:...",
    "model": "claude-sonnet-4.6",
    "settings": "sha256:...",
    "promptContract": "loom-done-v1"
  },
  "state": "active"
}
```

### 5.3 签名失效

session signature 至少包含：

- transport kind
- command
- `newArgs`
- `resumeArgs`
- selected model
- default model argv policy
- completion sentinel / prompt contract version
- Claude settings mode
- resolved Claude settings path
- optionally settings file content hash

如果 signature 不匹配，默认新建 session。原因是旧 session 可能已经加载了不同模型、权限、MCP、配置或提示词契约，继续 resume 会导致行为不可解释。

## 6. Prompt composition

### 6.1 现有 Loom envelope

`loom agent serve` 当前会为 agent 组合：

- identity
- soul
- bootstrap memory
- relevant memory
- scope bootstrap manifest
- user message

`interactive_command` 不应绕过这套 envelope，而是在 envelope 中增加 completion contract。

### 6.2 推荐 prompt 结构

```text
Agent identity:
...

Agent soul:
...

Bootstrap memory:
...

Relevant memory:
...

=== loom bootstrap ===
You are an agent driven by `loom agent serve`.
current scope = thread:thread_123
...

=== Loom interactive command completion contract ===
When your final user-visible answer is complete, output this exact marker on a line by itself:
__LOOM_DONE__
Do not output anything after the marker.

=== User message ===
<original user message>
```

channel 公共区可以增强 bootstrap：

```text
current scope = channel:chan_design
You are replying in a channel common area. Treat this as shared, long-lived context.
Do not assume this is a single focused task thread unless the user says so.
```

thread scope 可以增强 bootstrap：

```text
current scope = thread:thread_123
You are replying in a task thread. Maintain continuity for this thread.
```

## 7. Completion detection

### 7.1 Sentinel-first

默认：

```text
sentinel = "__LOOM_DONE__"
```

完成条件：

- 输出中出现 sentinel 独占行，或经 trim 后等于 sentinel。
- adapter 将 sentinel 之前的内容视为最终回复。
- sentinel 之后的输出默认忽略或作为 trace 记录。
- final answer 不包含 sentinel。

### 7.2 Done 约定（Definition of Done）

`interactive_command` 必须区分三层 “Done”：

| 层级 | Done 含义 | 由谁产生 | 结果 |
| --- | --- | --- | --- |
| Agent reply done | agent 已经输出完整的用户可见回答 | provider CLI 输出 sentinel | adapter 停止收集正文，准备 flush |
| Adapter turn done | adapter 已经完成输出归一化、session 记录、kill policy | `InteractiveCommandAdapter` | emit `AdapterEvent::Finished` |
| Loom turn done | Loom 已经把最终内容和 turn close 写回 server | `loom agent serve` translator | thread/channel timeline 可见最终结果 |

首版的 DoD 采用 **sentinel success** 作为唯一正常成功完成条件：

```text
normal success =
  sentinel detected
  AND final text flushed
  AND sentinel stripped when configured
  AND provider session record saved/updated
  AND completion kill policy applied or process already exited
  AND AdapterEvent::Finished { success: true } emitted
```

以下情况不算 successful Done：

| 情况 | Adapter 结果 | Loom turn 结果 |
| --- | --- | --- |
| 进程退出但没有 sentinel | failed，summary 说明 missing sentinel / exited early | turn failed |
| 达到 `maxTurnMs` | failed，summary 说明 timeout | turn failed |
| 用户取消 | cancelled / failed，summary 为 cancelled | turn cancelled 或 failed，取决于 server 已有状态 |
| spawn 失败 | failed，summary 包含 spawn error | turn failed |
| resume 失败 | failed，不自动重跑 | turn failed |
| kill policy fallback 仍未结束进程 | failed，summary 说明 kill failed，并记录 pid | turn failed |

这个约定避免把“进程退出”误判为成功。对 interactive CLI 来说，进程退出只能说明 provider 结束了，不代表它按 Loom 协议完成了回答。正常成功必须看到 sentinel。

### 7.3 Timeout fallback

建议区分两种 timeout：

| 配置 | 含义 | 默认用途 |
| --- | --- | --- |
| `idleTimeoutMs` | 多久没有输出后认为可能卡住 | 可选，默认不作为最终失败依据 |
| `maxTurnMs` | 本轮最大允许时长 | 硬失败 |

`idleTimeoutMs` 不建议默认直接失败，因为模型慢、网络慢、工具执行慢都会造成 idle。

### 7.4 Regex completion extension

可以预留：

```jsonc
{
  "completion": {
    "detect": "regex",
    "regex": "(?m)^DONE$"
  }
}
```

首版可以只实现 sentinel。

## 8. Output pipeline

```text
child stdout/stderr/pty
        │
        ▼
read chunks
        │
        ├─ strip ANSI?
        ├─ remove prompt echo?
        ├─ detect sentinel
        ├─ split final text / trace text
        ▼
AdapterEvent::Text
AdapterEvent::Finished
```

默认策略：

- stdout 作为候选 user-visible output。
- stderr 不混入最终回复，进入 trace 或 error summary。
- sentinel 从最终回复移除。
- ANSI stripping 默认可启用。
- partial streaming 可以启用，但最终仍以 sentinel 为完成边界。

需要注意：交互式 CLI 可能会输出 spinner、进度条、控制字符、prompt echo。首版应至少支持 ANSI stripping；更复杂的 provider-specific 清洗可以后续扩展。

## 9. Kill policy

### 9.1 配置模型

```jsonc
{
  "kill": {
    "onComplete": {
      "action": "sigterm",
      "graceMs": 3000,
      "fallback": "sigkill"
    },
    "onCancel": {
      "action": "sigterm",
      "graceMs": 1000,
      "fallback": "sigkill"
    },
    "onTimeout": {
      "action": "sigkill"
    }
  }
}
```

动作候选：

| action | 行为 |
| --- | --- |
| `stdin_eof` | 关闭 stdin |
| `ctrl_d` | 向终端发送 Ctrl-D |
| `ctrl_c` | 向终端发送 Ctrl-C |
| `sigterm` | Unix SIGTERM |
| `sigkill` | Unix SIGKILL |
| `none` | 不主动终止 |

### 9.2 默认值

建议默认：

```text
onComplete = sigterm, grace 3000ms, fallback sigkill
onCancel   = sigterm, grace 1000ms, fallback sigkill
onTimeout  = sigkill
```

如果后续验证 Claude / Copilot 对 Ctrl-C 或 stdin EOF 更友好，可以在 marketplace 示例里给 provider-specific 默认配置。

## 10. Claude settings policy

Claude settings 是 actor 级配置，而不是 scope 级上下文。建议放在 actor profile，而不是 channel workspace。

### 10.1 模式

| mode | 行为 | 用途 |
| --- | --- | --- |
| `global` | 不传 `--settings` | 使用用户全局 Claude 配置 |
| `actor_profile` | 传 `--settings {agent.profile}/claude/settings.json` | 每个 Loom actor 独立配置 |
| `custom` | 传 `--settings <resolved path>` | 用户完全自定义 |

### 10.2 示例

```jsonc
{
  "provider": {
    "kind": "claude",
    "settings": {
      "mode": "global"
    }
  }
}
```

```jsonc
{
  "provider": {
    "kind": "claude",
    "settings": {
      "mode": "actor_profile"
    }
  }
}
```

```jsonc
{
  "provider": {
    "kind": "claude",
    "settings": {
      "mode": "custom",
      "path": "{agent.profile}/claude/work-settings.json"
    }
  }
}
```

`actor_profile` 模式应确保 `{agent.profile}/claude/` 目录存在，但不应默认生成有副作用的 settings 内容。是否 scaffold 示例 settings 可以作为单独功能处理。

## 11. Adapter lifecycle

### 11.1 Start

`InteractiveCommandAdapter::start` 类似 `CommandAdapter::start`，只保存 event sender 和配置，不立即启动 provider CLI。

原因：provider CLI 是每个 prompt 启动一次，session continuity 由 provider session id 保证。

### 11.2 Send prompt

```text
send_prompt(AdapterPrompt)
  ├─ resolve scope key
  ├─ load session record
  ├─ compute active signature
  ├─ if no record or signature mismatch:
  │    ├─ generate session id
  │    └─ use newArgs
  ├─ else:
  │    └─ use resumeArgs
  ├─ append --model=<model> when active model is non-empty
  ├─ compose prompt + completion contract
  ├─ spawn child
  ├─ read output until sentinel / timeout / process exit
  ├─ save session record when first-run succeeds
  ├─ emit Text / Finished
  └─ apply kill policy
```

成功 Done 的顺序必须固定：

1. 检测到 sentinel。
2. 停止把后续 stdout 追加到用户可见正文。
3. strip sentinel 和配置要求移除的 terminal 噪声。
4. flush final text 为 `AdapterEvent::Text { is_partial: false }`。
5. 保存或更新 provider session record。
6. 应用 `kill.onComplete`；如果进程已退出则跳过。
7. emit `AdapterEvent::Finished { success: true, summary: "" }`。

如果第 5 或第 6 步失败，adapter 不应假装成功。它应 emit error/trace，并以 `Finished { success: false }` 收口，避免 Loom timeline 显示“成功完成”但 session 状态不可用或进程泄漏。

### 11.3 Cancel

`cancel(scope)`：

1. 找到该 scope 的 in-flight child。
2. 标记 cancel requested。
3. 应用 `kill.onCancel`。
4. 后续读循环结束时 emit `Finished { success: false, summary: "cancelled" }`。

### 11.4 Stop

`stop()`：

1. 对所有 in-flight child 应用 cancel/stop policy。
2. 清理 sender。
3. 不删除 session records。

session records 是 provider session 映射，不属于进程生命周期临时状态。

## 12. 与 `loom agent serve` 的集成

`loom agent serve` 当前负责：

- 读取 agent specs。
- 每个 agent 一条 WS connection。
- 监听 directed message。
- 打开 turn。
- 构造 `AdapterPrompt`。
- 调用 adapter。
- 翻译 `AdapterEvent` 为 server RPC。

新增集成点：

1. `build_adapter` 支持 `transport.kind == "interactive_command"`。
2. 传入 actor profile、scope workspace、scope env、template vars、selected model。
3. 保留 same-scope FIFO：同一个 scope 的多个 directed message 排队，前一轮完成后再执行下一轮。
4. channel scope 和 thread scope 在 session record key 中必须不同。

## 13. 用户操作建议

后续可以提供 session 管理 CLI，但首版不一定必须做。

建议命令：

| 命令 | 作用 |
| --- | --- |
| `loom agent session list <actor>` | 查看 actor 的 provider session 映射 |
| `loom agent session reset <actor> --in <scope>` | 删除某 actor 在某 scope 的映射，下次新建 |
| `loom agent session bind <actor> --in <scope> --session-id <id>` | 高级：绑定已有外部 provider session |
| `loom directed message <actor> --new-session --in <scope>` | 本次 directed message 前 reset 并新建 |

如果不做这些 CLI，仍可以先通过删除 session record 文件实现手动 reset。

## 14. 测试策略

### 14.1 Schema

- `interactive_command` spec 可反序列化。
- `global` / `actor_profile` / `custom` Claude settings 可反序列化。
- 旧 `acp_stdio` / `command` spec 不受影响。

### 14.2 Session lifecycle

- 同 actor + 同 thread：第二次 directed message resume。
- 同 actor + 不同 thread：不同 session id。
- 同 actor + channel common area：独立 session id。
- 不同 actor + 同 thread：不同 session id。
- command/model/settings/signature 变化：新建 session。

### 14.3 Prompt / completion

- 默认注入 `__LOOM_DONE__` contract。
- 自定义 sentinel 生效。
- sentinel 输出触发 completion。
- final answer 不包含 sentinel。
- 进程退出但没有 sentinel 不算 successful Done。
- successful Done 必须 flush final text、保存 session record、应用 completion kill policy，并 emit `Finished { success: true }`。
- maxTurnMs 超时会失败。

### 14.4 Kill policy

- completion 后应用 onComplete。
- cancel 后应用 onCancel。
- graceful action 未退出时应用 fallback。

### 14.5 Regression

- `acp_stdio` 仍走 `AcpAdapter`。
- `command` 仍走 `CommandAdapter`。
- command transport 仍以进程退出为完成边界。

## 15. 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| agent 不遵守 sentinel contract | 使用 maxTurnMs 硬失败，并在 UI/trace 中暴露原因 |
| sentinel 出现在正常内容里 | 默认 marker 足够独特，并允许 actor 自定义 |
| kill 破坏 provider session 状态 | 优先 graceful，provider-specific 调整 kill policy |
| Copilot 首次 `--resume uuid` 不创建 session | 保留独立 `newArgs`，用真实 CLI 验证后配置 |
| Claude settings 变化导致历史不可复用 | settings 参与 signature，默认新建保证正确性 |
| PTY 与 pipe 行为不同 | 首版可先 pipe；如 CLI 要求 TTY，再引入 PTY backend |
| 自动 retry 导致重复执行 | 默认 `onResumeFailed = fail`，不自动重跑 |

## 16. 开放问题

1. 首版是否需要 PTY，还是 pipe 足够覆盖 Claude / Copilot？
2. Claude 当前版本的 `--settings`、`--session-id`、`--resume` 精确语法是否稳定？
3. Copilot 当前版本对不存在的 `--resume <uuid>` 是创建还是报错？
4. session 管理 CLI 是否纳入首版，还是作为后续体验增强？
5. actor_profile settings 是否只建目录，还是 scaffold 一个空 settings 文件？

## 17. 推荐落地顺序

1. 扩展 schema，但保持旧 spec 兼容。
2. 实现 session key / record / signature。
3. 实现 `InteractiveCommandAdapter` 的最小 pipe 版本。
4. 实现 sentinel completion 和 maxTurnMs。
5. 实现 kill policy。
6. 接入 Claude settings policy。
7. 接入 `loom agent serve`。
8. 增加 Claude / Copilot 示例。
9. 根据真实 CLI 验证决定是否补 PTY。
