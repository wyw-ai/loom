# Provider 扩展设计

本文描述 Loom agent runtime 的下一版 Provider 模型。目标是让新增 Provider
尽量数据驱动，而不是继续把 provider id、启动参数和输出解析逻辑硬编码在
Rust 里。

## 问题

当前一个 Provider 实际承担两类职责，但两者都部分硬编码：

- 启动方式：`discovery.rs` 根据 provider id 决定命令、固定 argv、模型参数、
  session 规则、prompt 传递方式和输出格式。
- 输出解析：`command.rs` 根据 `CommandOutputFormat` 枚举调用 Rust parser，
  例如 Claude stream JSON、Copilot JSON、Codex stream JSON 或纯文本。

这个方式在内置 Provider 很少时可行，但每加一个 Provider 都需要改多处
Rust 代码。即使新 Provider 只是另一种 CLI 包装，仍然要新增 hardcode。

Provider 的契约应该显式表达两个点：

1. Loom 如何启动 Provider，以及如何把 Loom 拼好的 prompt 映射到 Provider
   的命令、参数、stdin 或 env。
2. Loom 如何解析 Provider 的 stdout/stderr，并转换成内部 AdapterEvent。

这里的“prompt”不应该只是一个已经拼死的 `{loom.envelope}` 字符串。Loom 应该先
产出一组结构化 prompt parts，再由 Provider 决定这些 parts 如何组合、哪些进入
system prompt、哪些进入 user prompt、是否插入分隔符、顺序如何排列。

## 需要保留的现有行为

Loom 仍然拥有 prompt 内容生成逻辑。Provider spec 不负责自己生成 identity、
memory、handoff 或 assignment 内容。daemon 会先生成一组结构化 prompt parts，
例如 actor identity、profile identity/soul、Loom 规则、当前时间、最近消息、
最新 handoff/task message 等。Provider 负责决定这些 parts 的顺序、拼接方式，
以及它们映射到 Provider 的 system/user prompt、argv、stdin 或 env 中哪个位置。

Provider stdout 默认仍然只进入 run trace。Provider 结果是否成为 GUI 可见消息，
应继续由 Loom 策略决定：默认必须由 agent 显式调用 `loom message send`；只有
部署显式开启 auto-publish 时，最终文本才会自动发布成可见消息。

## 建议 Schema

引入 provider manifest。内置 Provider 可以作为 embedded manifest 随程序发布；
本地自定义 Provider 可以放在配置目录下，例如：

```text
{loom.configDir}/providers/<provider_id>.json
```

建议结构：

```json
{
  "schemaVersion": 1,
  "id": "claude",
  "displayName": "Claude Code",
  "detect": {
    "candidates": ["claude"]
  },
  "modes": {
    "print": {
      "transport": "command",
      "command": "{bin}",
      "prompt": {
        "outputs": {
          "system": {
            "preset": "loom_system"
          },
          "user": {
            "preset": "loom_turn"
          }
        }
      },
      "args": [
        "--add-dir", "{loom.configDir}",
        "--permission-mode", "bypassPermissions",
        "--output-format", "stream-json",
        "--verbose",
        "--session-id", "{session.id}",
        { "when": "model", "args": ["--model", "{model}"] },
        "--append-system-prompt", "{prompt.system}",
        "-p", "{prompt.user}"
      ],
      "stdout": {
        "format": "jsonl",
        "events": [
          {
            "when": { "path": "$.type", "equals": "assistant" },
            "emit": {
              "type": "text",
              "forEach": "$.message.content[*]",
              "when": { "path": "$.type", "equals": "text" },
              "text": "$.text",
              "partial": false
            }
          },
          {
            "when": { "path": "$.type", "equals": "assistant" },
            "emit": {
              "type": "tool_use",
              "forEach": "$.message.content[*]",
              "when": { "path": "$.type", "equals": "tool_use" },
              "name": "$.name",
              "input": "$.input"
            }
          }
        ]
      },
      "session": {
        "idSource": "loom_uuid",
        "scope": "actor_scope"
      }
    }
  },
  "models": {
    "default": "sonnet",
    "choices": [
      { "id": "sonnet", "label": "Sonnet" },
      { "id": "opus", "label": "Opus" }
    ]
  }
}
```

关键点是 `modes.print.args` 表达完整 argv 模板，同时 `modes.print.prompt.outputs`
表达 prompt parts 的组合方式。manifest 可以把 `{prompt.system}`、
`{prompt.user}`、`{prompt.full}` 或任意命名 prompt 输出放在任意位置，因此 Loom
不再需要对 `-p`、末尾追加 prompt、stdin 或 env prompt 做 provider 特殊逻辑，
只需要保留少量向后兼容能力。

`models` 只描述 UI 和默认模型选择；模型是否进入 CLI 参数，也由 mode 的
`args`/`env`/`stdin` 模板显式决定。也就是说，`--model {model}` 不应该藏在
另一个隐式 `modelArgs` 分支里。过渡期可以把现有 `transport.modelArgs` 编译进
manifest，但新 manifest 应以 argv 模板为准。

`detect.candidates` 是最小检测方式。内置 Provider 还应在测试里固定一组 help/version
样本，防止 CLI 升级后参数名变化但 manifest 仍然静默通过。自定义 Provider 可以先只
写 `candidates`；后续再扩展可选的 `versionCommand`、`helpCommand`、
`requiresHelpFlags` 做更严格校验。

如果某个 Provider 不支持 system prompt，可以在 prompt 配置中把所有 parts 合成
`prompt.full`，再传给一个普通 prompt 参数。

## Provider Mode

一个 Provider 可以声明多个 mode。Loom 为每个 agent runtime 选择一个 mode。

- `print`：每个 turn 启动一次命令。这是当前 Claude、Qoder、Copilot、Codex、
  OpenCode 的默认形态。它最简单，也最容易 supervise。
- `interactive`：使用 Loom 拥有的 session id 和 completion sentinel 的交互式命令。
  对应当前 `interactive_command` transport。
- `acp`：长生命周期 ACP stdio 进程。prompt 通过 `session/prompt` 发送，不走 argv。

mode 选择应该由 AgentSpec 或 Provider 默认值显式决定。GUI 创建的 agent 默认使用
`print`，除非 Provider 声明了更好的默认 mode。

## Prompt Parts 与拼接策略

Loom prompt composer 应输出结构化 parts，而不是只输出一个字符串。建议最小集合：

```text
trigger_prefix      兼容现有 AgentSpec triggerPromptPrefix
actor_context       Loom 注入的 actor id/display identity
identity            profile identity.md
soul                profile soul.md
bootstrap_memory    长期/启动 memory
scope_bootstrap     Loom CLI、协作规则、当前 scope 等首轮规则
turn_memory         本 turn 选中的 memory
runtime_context     本地时间、最近消息等运行时上下文
latest_message      Latest Loom message，含路由、scope、task 元信息
assignment_context  task assignment 的权威 JSON 上下文；非 task 场景为空
turn_input          兼容现有 user_text，包含 prompt_template 包装后的 latest_message
                    + assignment_context
user_message        turn_input 的兼容别名
```

每个 part 至少包含：

```json
{
  "key": "scope_bootstrap",
  "title": "System: Loom multi-actor context",
  "content": "...",
  "roleHint": "system"
}
```

`part.content` 应该是未包标题的正文，标题由 `part.title` 和输出的 `renderTitle`
决定，避免 Provider 模板里再手写一次标题导致重复。为了兼容旧实现，Loom 可以在
迁移期继续保留 legacy envelope 文本，但新 composer 应尽量输出 raw content +
metadata。

Provider manifest 可以把这些 parts 组合成任意命名 prompt 输出。输出支持三种写法：

- `preset`：使用 Loom 内置默认组合，减少每个 Provider 重复维护 part 列表。
- `include` / `join`：结构化拼接，适合按 part 顺序组合。
- `template`：自由模板，适合精确控制文案、分隔符和顺序。

```json
{
  "prompt": {
    "outputs": {
      "system": {
        "preset": "loom_system"
      },
      "user": {
        "preset": "loom_turn"
      },
      "full": {
        "preset": "loom_full"
      }
    }
  }
}
```

建议内置 preset：

```text
loom_system  trigger_prefix + actor_context + identity + soul
             + bootstrap_memory + scope_bootstrap + turn_memory + runtime_context
loom_turn    turn_input，保留现有 prompt_template 行为
loom_full    loom_system + loom_turn，等价于当前单字符串 envelope 的语义
```

preset 不是黑盒。Provider 可以改用 `include` 或 `template` 完全控制顺序；未来 Loom
新增 part 时，只需要更新 preset，旧 Provider 不会被迫修改 manifest。

如果 manifest 没有声明 `prompt.outputs`，Loom 应自动提供一个兼容输出：
`full = loom_full`。这保证旧 `AgentTransport` 或最小自定义 Provider 仍然能通过
`{loom.envelope}` / `{prompt.full}` 获得当前语义。只要 manifest 显式引用了不存在的
`{prompt.<name>}`，校验就应该失败。

`template` 中可以直接引用原子 part：

```json
{
  "prompt": {
    "outputs": {
      "claude_system": {
        "template": "{actor_context}\n\n{identity}\n\n{soul}\n\n{scope_bootstrap}"
      },
      "claude_user": {
        "join": "\n\n",
        "include": ["latest_message", "assignment_context"],
        "renderTitle": "always"
      }
    }
  }
}
```

组合规则：

- `include` 决定顺序，空 part 自动跳过。
- `join` 决定 part 之间的拼接字符串。
- `prefix` / `suffix` 可用于包装某个组合结果。
- `template` 精确控制输出文本。未出现或为空的 part 默认展开为空字符串；如果需要
  缺失时报错，应在输出或变量上声明 `required`。
- `renderTitle` 可以决定是否保留 part 标题，例如 `always`、`never`、`auto`。
- `required` 可声明某个 part 缺失时启动失败，默认缺失就跳过。
- `roleHint` 只是 Loom 默认建议，最终 system/user/assistant 映射由 Provider
  manifest 决定。
- `user_message` 是兼容别名，配置新 Provider 时优先使用 `latest_message`、
  `assignment_context` 或 preset，避免把一个已经合成过的 turn 输入再次包标题。
- 如果 actor 依赖 `prompt_template`，Provider 使用 `turn_input` 或 `loom_turn`
  能保留现有行为；直接使用 `latest_message`/`assignment_context` 则表示 Provider
  明确接管 turn 文本包装方式。

这样 Claude 这种支持 system prompt 的 Provider 可以把稳定规则放到 system，
把最新 handoff/task message 放到 user；而只支持一个 prompt 字符串的 Provider
可以直接使用 `{prompt.full}`。

## 模板变量

Provider 的 argv/env/stdin 模板应支持这些运行时变量：

```text
{bin}                  检测到的可执行文件路径
{prompt.<name>}        prompt.outputs 生成的命名 prompt，例如 system/user/full
{loom.envelope}        兼容变量，等价于 {prompt.full}
{loom.configDir}       当前 Loom 配置目录，可能是 per-machine/per-server 目录
{loom.server}          server websocket URL
{loom.actor}           actor id
{loom.scope.id}        当前 channel/thread scope id
{loom.scope.kind}      channel 或 thread
{loom.run.id}          当前 run id
{loom.trigger.id}      触发当前 turn 的 message/source id
{loom.trigger.actor}   触发当前 turn 的 actor id
{model}                当前选中的模型 id
{reasoningEffort}      当前选中的 reasoning effort，如果有
{session.id}           当前 Provider session id。可能是 Loom 生成的稳定 UUID，
                       也可能是从 Provider 输出捕获的 id
{paths.cwd}            本 turn 工作目录
```

模板展开必须 argv-safe：JSON 数组里的每个元素展开后仍然是一个 argv token。
Loom 不应该把 argv 拼成 shell 字符串再执行。

为了支持可选模型、可选 reasoning effort 或不同权限配置，`args` 除了字符串，也可以
支持条件片段：

```json
{
  "args": [
    "exec",
    "--skip-git-repo-check",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--effort", "{reasoningEffort}"] },
    "{prompt.full}"
  ]
}
```

`when` 只判断 Loom 运行时变量是否存在且非空。第一版不需要表达复杂逻辑；复杂逻辑
应该拆成不同 mode，或者由 AgentSpec 覆盖 mode 配置。

对于较大的 prompt，Provider 可以要求通过 stdin 或 env 传入任意命名 prompt：

```json
{
  "args": ["run", "--prompt-file", "-"],
  "stdin": "{prompt.full}"
}
```

或者：

```json
{
  "args": ["run"],
  "env": {
    "PROVIDER_SYSTEM_PROMPT": "{prompt.system}",
    "PROVIDER_USER_PROMPT": "{prompt.user}"
  }
}
```

这会替代当前 `PromptVia::{Args,Stdin,Env}` 的特殊分支，变成统一模板模型。

## Session 策略

Provider session 不应该默认依赖 stdout 解析。更稳的优先级是：

1. `loom_uuid`：Loom 为 `(provider, actor, scope)` 生成并持久化稳定 UUID，Provider
   每轮都通过 argv/env/stdin 收到同一个 `{session.id}`。如果 CLI 支持“指定 session
   id，不存在则创建、存在则继续”，优先使用这种方式。
2. `provider_capture`：Provider 自己生成 session id，Loom 从 stdout/stderr/file
   捕获后保存，后续通过 resume 参数传回。
3. `none`：每轮都是无状态新进程。

建议 schema：

```json
{
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

`scope` 建议先支持：

- `actor_scope`：同一个 actor 在同一个 channel/thread 内复用一个 Provider session。
  这是当前 command transport session map 的语义。
- `actor`：同一个 actor 全局复用一个 Provider session。
- `turn`：每个 turn 新 session，通常只用于调试。

如果 Provider 只能自己吐出 session id，则使用捕获模式：

```json
{
  "session": {
    "idSource": "provider_capture",
    "capture": "stdout_jsonl:last($.session_id)",
    "resumeArgs": ["--resume", "{session.id}", "-p", "{prompt.user}"]
  }
}
```

`capture` 的含义是“从输出里解析 session id”，不是输出解析的必需步骤。Claude Code
和 Copilot 这类支持指定 session id 的 Provider，不应该为了拿 session id 再解析
stdout；stdout parser 只负责 run trace、最终文本、工具调用、usage 和错误。

## 输出解析模型

将 `CommandOutputFormat` 从扩展点降级为兼容别名，引入 Provider 自己声明的
`stdout` / `stderr` parser spec。常见格式仍可保留 built-in alias，但自定义
Provider 应该能在不改 Rust 的情况下描述解析方式。

推荐 parser 类型：

- `text`：收集完整 stdout，进程结束后发一个最终 text event。
- `json`：把完整 stdout 解析成一个 JSON 文档，再提取字段。
- `jsonl`：逐行解析 stdout JSON，边读边发事件。
- `regex`：兜底文本提取方式。
- `builtin`：兼容逃生口，用于过于复杂或性能敏感、暂时无法 DSL 化的 Rust parser。

`builtin` 也必须写在 manifest 里，例如 `{ "format": "builtin", "name":
"codex_stream_json" }`。这样 runtime 不再通过 provider id 推断解析方式；只是某些
parser 的实现暂时仍在 Rust 里。

每个 parser 输出内部 AdapterEvent：

- `text`：候选文本，进入 run trace；是否发布成可见消息仍由 Loom 策略决定。
- `tool_use`：Provider 报告的工具调用 trace。
- `status`：Provider 进度或状态 trace。
- `error`：Provider 错误 trace。
- `usage`：token usage metadata。
- `session`：仅用于 `provider_capture` 模式下报告捕获到的 session id。
- `finish`：显式完成信号；没有显式完成时，进程退出仍会产生 Finished。

输出解析不能只靠“每条 JSON 映射一个事件”。Copilot/Codex 这类 CLI 会同时输出
中间解释、工具轨迹、子 agent 轨迹和最终回答，Provider 需要能声明 stateful reducer：

- `emit`：看到一条输入就发 AdapterEvent，适合 Claude assistant text/tool_use。
- `reduce.finalText`：在整个 stdout 结束后，从多条 JSONL 中选最终文本。
- `reduce.deltaText`：累积流式 delta，作为 finalText 的 fallback。
- `capture.session`：仅在 Provider 不能由 Loom 指定 session id 时，从 JSONL 或完整
  JSON 中捕获最后一个非空 session id。

Copilot 当前 `--output-format json --stream off` 实际是 JSONL，每行一个事件。现有
`CommandOutputFormat::CopilotJson` 的关键逻辑是：忽略 sub-agent 和 parent tool call
事件，忽略 thinking/reasoning phase，取最后一个 root `assistant.message`，如果没有
完整 message 再拼接 `assistant.message_delta`。这应被表达成 Provider parser，而不是
继续由 provider id 硬编码：

```json
{
  "stdout": {
    "format": "jsonl",
    "reduce": {
      "finalText": {
        "mode": "lastNonEmpty",
        "path": "$.data.content",
        "when": {
          "all": [
            { "path": "$.type", "equals": "assistant.message" },
            { "path": "$.agentId", "absentOrNull": true },
            { "path": "$.data.parentToolCallId", "absentOrNull": true },
            { "path": "$.data.phase", "notIn": ["thinking", "reasoning"] }
          ]
        },
        "fallback": {
          "mode": "concat",
          "path": "$.data.deltaContent",
          "when": {
            "all": [
              { "path": "$.type", "equals": "assistant.message_delta" },
              { "path": "$.agentId", "absentOrNull": true },
              { "path": "$.data.parentToolCallId", "absentOrNull": true }
            ]
          }
        }
      }
    }
  }
}
```

通用 JSONL 可以这样表达：

```json
{
  "stdout": {
    "format": "jsonl",
    "events": [
      {
        "when": { "path": "$.type", "equals": "text" },
        "emit": {
          "type": "text",
          "text": "$.text",
          "partial": true
        }
      },
      {
        "when": { "path": "$.type", "equals": "done" },
        "emit": { "type": "finish" }
      }
    ]
  }
}
```

第一版实现可以保持 selector 很小：

- JSONPath 子集：`$.field`、`$.a.b`、`$.arr[*]`、数字下标。
- 条件操作：`equals`、`notEquals`、`exists`、`absentOrNull`、`notEmpty`、`in`、`notIn`、
  `all`、`any`、`not`。
- 提取器：直接 path、`firstNonEmpty`、`lastNonEmpty`、`joinText`、`concat`。

这已经足够表达当前 Claude/Qoder stream JSON、Copilot JSONL final text 提取，以及
大多数 JSONL Provider 输出。不需要一开始就引入完整脚本引擎。Codex JSONL 的历史
形态较多，第一版可以继续使用 `builtin: codex_stream_json`，但这个别名也应该出现在
manifest 中，而不是通过 provider id 推断。

## 现有 Provider 接入结论

以下结论基于当前代码硬编码和本机 CLI help 抽查：

```text
Claude Code          2.1.92
Qoder CLI            1.0.6
GitHub Copilot CLI   1.0.54
Codex CLI            0.134.0
OpenCode             当前未安装，安装后再补调研
```

### Claude Code

Claude Code 支持非交互 `-p/--print`、`--output-format stream-json`、
`--append-system-prompt`、`--system-prompt`、`--resume`、`--add-dir`、`--plugin-dir`、
`--session-id`、`--model` 和 `--permission-mode bypassPermissions`。Loom 应优先使用
`--append-system-prompt`，因为 `--system-prompt` 可能替换 Claude Code 默认系统行为，
影响内置工具、默认规则或 CLI 自身约束。Session 建议用 Loom 生成的稳定 UUID，通过
`--session-id {session.id}` 传给 Claude Code，而不是从 stream-json 里捕获。

建议 print mode：

```json
{
  "args": [
    "--add-dir", "{loom.configDir}",
    "--permission-mode", "bypassPermissions",
    "--output-format", "stream-json",
    "--verbose",
    "--session-id", "{session.id}",
    { "when": "model", "args": ["--model", "{model}"] },
    "--append-system-prompt", "{prompt.system}",
    "-p", "{prompt.user}"
  ],
  "stdout": { "format": "builtin", "name": "claude_stream_json" },
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

### Qoder CLI

Qoder CLI 的 print 接口与 Claude Code 接近，支持 `-p/--print`、`--output-format`、
`--append-system-prompt`、`--system-prompt`、`--resume`、`--add-dir`、`--plugin-dir`、
`--model`、`--reasoning-effort` 和 `--permission-mode bypass_permissions`。
现有 Loom 代码把 Qoder 按 Claude stream JSON 解析，并使用 `--yolo`。新 manifest
应优先写文档化参数，并直接声明 session 策略：

```json
{
  "args": [
    "--add-dir", "{loom.configDir}",
    "--permission-mode", "bypass_permissions",
    "--output-format", "stream-json",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--reasoning-effort", "{reasoningEffort}"] },
    "--append-system-prompt", "{prompt.system}",
    "-p", "{prompt.user}"
  ],
  "stdout": { "format": "builtin", "name": "claude_stream_json" },
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "capture": "stdout_jsonl:last($.session_id)",
    "resumeArgs": [
      "--add-dir", "{loom.configDir}",
      "--permission-mode", "bypass_permissions",
      "--output-format", "stream-json",
      { "when": "model", "args": ["--model", "{model}"] },
      { "when": "reasoningEffort", "args": ["--reasoning-effort", "{reasoningEffort}"] },
      "--append-system-prompt", "{prompt.system}",
      "--resume", "{session.id}",
      "-p", "{prompt.user}"
    ]
  }
}
```

Qoder session 直接按 `provider_capture + --resume` 接入：首轮从 stream-json 输出捕获
Provider 生成的 session id，后续 turn 用 `--resume {session.id}`。

### GitHub Copilot CLI

Copilot CLI 支持 `-p/--prompt`、`--output-format json`、`--stream off`、`--yolo`、
`--add-dir`、`--model`、`--effort/--reasoning-effort` 和 `--resume`。help 中没有
system prompt 参数，因此应把 Loom system + turn 合成 `{prompt.full}` 传给 `-p`。
Session 建议用 Loom 生成的稳定 UUID，通过 `--resume {session.id}` 传给 Copilot；
如果该 id 不存在则创建 session，存在则 resume。

建议 print mode：

```json
{
  "prompt": {
    "outputs": {
      "full": { "preset": "loom_full" }
    }
  },
  "args": [
    "--add-dir", "{loom.configDir}",
    "--yolo",
    "--output-format", "json",
    "--stream", "off",
    "--resume", "{session.id}",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--effort", "{reasoningEffort}"] },
    "-p", "{prompt.full}"
  ],
  "stdout": { "format": "builtin", "name": "copilot_jsonl_final_text" },
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

这里的 parser 不能简单把所有 `assistant.message` 发成 text，否则会把中间状态和
最终回答一起暴露。`copilot_jsonl_final_text` 可以先作为 builtin parser 落地，
但语义必须等价于上一节的 finalText reducer；后续可直接展开成 manifest reducer。

### Codex CLI

Codex CLI 的非交互入口是 `codex exec [PROMPT]`。它支持 `--json` JSONL 输出、
`--sandbox danger-full-access`、`--add-dir`、`--model`、`-C/--cd`，也支持 prompt
参数为空时从 stdin 读取。Codex 没有专用 system prompt 参数，因此第一版应使用
`{prompt.full}`。现有 Loom 还为 Codex 设置 `LOOM_NO_DAEMON=1`，让 agent 内部调用
`loom` CLI 时走 websocket server，而不是访问 macOS sandbox 可能拦截的 daemon
Unix socket；这个行为必须进入 manifest/env。

建议 print mode：

```json
{
  "prompt": {
    "outputs": {
      "full": { "preset": "loom_full" }
    }
  },
  "env": {
    "LOOM_NO_DAEMON": "1"
  },
  "args": [
    "exec",
    "--skip-git-repo-check",
    "--json",
    "--sandbox", "danger-full-access",
    "-c", "sandbox_workspace_write.network_access=true",
    "--add-dir", "{loom.configDir}",
    { "when": "model", "args": ["--model", "{model}"] },
    "{prompt.full}"
  ],
  "stdout": { "format": "builtin", "name": "codex_stream_json" },
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "capture": "stdout_jsonl:last($.session_id)",
    "resumeArgs": [
      "exec",
      "resume",
      "{session.id}",
      "--skip-git-repo-check",
      "--json",
      "--sandbox", "danger-full-access",
      "-c", "sandbox_workspace_write.network_access=true",
      "--add-dir", "{loom.configDir}",
      { "when": "model", "args": ["--model", "{model}"] },
      "{prompt.full}"
    ]
  }
}
```

Codex 输出历史形态较多，短期用 builtin parser alias 更稳；中期可以把已知
`task_complete`、`agent_message`、`item.completed`、`agent_message_content_delta`
规则逐步迁移成 manifest reducer。Codex session 直接按 `provider_capture + resume`
接入：首轮从 JSONL 输出捕获 Provider 生成的 session id，后续 turn 用
`codex exec resume {session.id}`。

### OpenCode

OpenCode 当前本机未安装，不能把接入方式写成已验证事实。先保留现有行为假设：
`opencode run --dangerously-skip-permissions {prompt.full}`，输出按 `text` 处理。
安装后需要补齐 CLI help、模型参数、权限参数、是否支持 JSON/JSONL 输出、是否支持
system prompt 和 session resume，再更新内置 manifest。

## Runtime 架构

建议分层：

```text
ProviderManifest
  -> ProviderRuntimePlan
    -> CommandInvocation
    -> OutputDecoder
      -> AdapterEvent
```

`ProviderManifest` 是配置，应保持可序列化、稳定。

`ProviderRuntimePlan` 是检测、模型选择、scope 解析、模板展开之后的 resolved form。
它包含具体 argv/env/stdin 和编译后的 decoder。

`CommandInvocation` 只负责启动和 supervise 进程，不再知道 Claude、Copilot、
Codex 或 Qoder。

`OutputDecoder` 消费 stdout/stderr，产生 `AdapterEvent`。之后仍然复用现有
run trace 和可选 auto-publish pipeline。

## 向后兼容

不要马上移除 `AgentTransport`。建议分阶段迁移：

1. 为当前内置 Provider 增加 manifest。
2. discovery 加载内置 manifest 和本地 manifest。
3. 过渡期内，把选中的 provider mode 编译成现有 `AgentTransport`。
4. 将 command 输出解析移动到 manifest-driven decoder 后面。
5. 将现有 command session map 扩展成支持 `loom_uuid`，让 `{session.id}` 可以在首次
   启动前就存在；保留 `provider_capture` 兼容当前 stdout capture。
6. 保留 `CommandOutputFormat` 作为别名：
   - `text` -> manifest parser `format=text`
   - `claude_stream_json` -> built-in manifest parser alias
   - `copilot_json` -> manifest parser alias `copilot_jsonl_final_text`
   - `codex_stream_json` -> 暂时保留 built-in parser alias
   - `ndjson_lines` -> generic JSONL manifest
7. AgentSpec 和 GUI 迁移完成后，让 `providerRef` 成为主配置；raw transport 只保留给
   高级/手写 spec。

## AgentSpec 方向

长期看，GUI 创建的 agent 不应该复制一大段 transport object，而应该引用 Provider
和 mode：

```json
{
  "actor": {
    "id": "actor_joi",
    "displayName": "Joi"
  },
  "providerRef": {
    "id": "claude",
    "mode": "print",
    "model": "sonnet"
  },
  "identity": {
    "files": {
      "identity": "identity.md",
      "soul": "soul.md"
    }
  }
}
```

daemon 在启动时把 `providerRef` resolve 成具体 runtime plan。手写 spec 作者仍然
可以覆盖 command args、env、parser、timeout、session 规则。

## 安全与校验

Provider manifest 使用前必须校验：

- `id` 必须稳定、小写、唯一。
- `command` 必须来自 `detect.candidates` 的解析结果，或者是显式路径。
- 模板只能引用已知变量，除非开启显式 allow unknown。
- command mode 下至少一个 `{prompt.<name>}` 或兼容 `{loom.envelope}` 必须出现在
  args/stdin/env 中；如果同一个组合输出被多处引用，必须显式声明允许重复发送。
  ACP mode 例外。
- 对需要访问 Loom config 的 sandbox CLI，应建议或要求
  `--add-dir {loom.configDir}`。
- parser 每行/每个文档的提取工作必须有边界。
- regex parser 要限制输入大小，避免灾难性回溯。

对于本地自定义 Provider，GUI 应展示它是 custom provider，以及最终会执行哪个
命令。

## `loom provider` 管理命令

`loom provider` 应成为 Provider lifecycle 的 CLI 入口，而不只是一个校验工具。它
负责本地自定义 Provider 的校验、录入、查看、删除和诊断；GUI 可以复用同一套
backend API。

建议第一版命令：

```bash
loom provider validate <file>
```

校验一个 manifest 是否能被 Loom 正确加载。校验应覆盖 schema、模板变量、
prompt 输出引用、session 配置、parser 配置、命令检测和路径安全。

```bash
loom provider add <file>
```

把一个自定义 Provider 录入当前 `{loom.configDir}/providers/`。`add` 必须先跑
`validate`；如果 `id` 已存在，默认失败，除非显式传 `--replace`。内置 Provider
不能被删除，但是否允许本地 manifest 覆盖内置 Provider 需要单独设计。

```bash
loom provider list
```

列出当前可用 Provider，包括 built-in 和 local custom。输出至少包含 id、
displayName、source、detect 状态、默认 mode 和默认模型。

```bash
loom provider show <provider_id>
```

展示 Provider 的最终 resolved 配置：来源、检测到的二进制、可用 modes、默认 mode、
models、prompt outputs、argv/env/stdin 模板、stdout/stderr parser、session 策略。
这里展示的是 resolve 后的结果，方便配置者确认 Loom 实际会怎么启动 Provider。

```bash
loom provider remove <provider_id>
```

删除本地自定义 Provider。内置 Provider 不允许删除；后续可以支持 disable/enable，
但第一版可以先不做。

```bash
loom provider doctor <provider_id>
```

对已安装 Provider 做诊断：检查 CLI 是否存在、版本/help 是否符合 manifest 预期、
参数模板能否展开、`{session.id}` 策略是否完整、stdout parser 是否能用样本解析。
如果 manifest 提供 sample output，doctor 应直接跑 parser；如果没有 sample，可以
只做静态检查和可执行文件检测，避免误触发真实模型调用。

命令写入范围应遵守当前 `{loom.configDir}`。这意味着 GUI 对接多个 server/machine
时，不同 config dir 下的 local Provider 互不污染；`loom provider list` 默认只看
当前进程解析出的 `{loom.configDir}`。

## 迁移计划

1. 在 `proto` 或 `agent-runtime` 增加 manifest 类型和 parser 校验测试。
2. 把当前内置 Provider 编码为 Rust 常量或 JSON resource。
3. 将现有 prompt composer 调整为先输出结构化 parts，再由 Provider manifest
   组合成 `{prompt.system}`、`{prompt.user}`、`{prompt.full}` 等命名输出。
4. 实现 argv/env/stdin 模板展开，并支持上面列出的变量和条件 args。
5. 先让内置 Provider manifest 使用 `builtin` parser alias 跑通，确保行为与当前
   `CommandOutputFormat` 完全一致。
6. 实现 `text`、`json`、`jsonl` 的 manifest parser engine，并用它重新表达 Claude
   stream JSON 和 Copilot JSONL final-text reducer。它们能分别证明“逐条 emit”和
   “进程结束 reducer”两类解析能力。
7. 用 Claude manifest 验证 system/user prompt 拆分。Claude/Qoder 应优先使用
   `--append-system-prompt`，避免覆盖 CLI 自带系统行为；如果某版本参数名变化，
   manifest 只需调整 argv 模板，不影响 Loom prompt parts。
8. Qoder/Codex 初期可以继续使用 builtin parser alias；当 JSONL 提取模型足够后再迁移。
9. 更新 GUI machine inventory，展示 provider id、modes、默认 mode、models、
   provider 是 built-in 还是 local。
10. 增加 `loom provider validate/add/list/show/remove/doctor`，方便调试和管理
    自定义 Provider。

## 待定问题

- Provider manifest 应放在 `.loom/providers`，还是只放在 per-machine config 目录？
  per-machine 能避免之前 desktop config 的 server 冲突问题。
- auto-publish 是否继续只由全局 env 控制，还是允许 provider/actor spec 显式配置？
- OpenCode 安装后需要补齐 CLI help 和输出样本，再确认是否继续 text parser。
- 复杂 Provider 是否需要 JavaScript/WASM parser hook，还是小型 JSONPath/regex DSL
  足够？
- models 应该静态写在 manifest、通过 provider command 动态发现，还是两者都支持并
  使用 cache fallback？
- `interactive_command` 和 command `print` mode 是否现在就合并到同一 manifest
  schema，还是等 print mode 稳定后再处理 interactive？
