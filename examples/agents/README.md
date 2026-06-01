# AgentSpec 样例

这里的 JSON 只描述“一个具体 agent 是谁，以及它选择哪个 provider”。它们不再
包含 `command`、`args`、`env`、stdout parser、session/resume 规则，也不再使用旧的
`provider + transport + actors[]` 混合结构。

运行适配规则属于 `ProviderManifest`，示例见
[`../providers`](../providers)。如果需要新增 CLI、改参数、改解析方式，先添加或覆盖
provider manifest，再让 agent 的 `providerRef.id` 指向它。

当前样例：

| 文件 | providerRef | 说明 |
| --- | --- | --- |
| [`actor_claude_stream.json`](actor_claude_stream.json) | `claude` / `print` | Claude Code `-p --output-format stream-json` 模式，system/user prompt 由 provider manifest 拆分注入 |
| [`actor_claude_nonprint.json`](actor_claude_nonprint.json) | `claude` / `nonprint` | Claude Code 普通交互模式，用 sentinel 判断完成 |
| [`codex.json`](codex.json) | `codex` / `print` | Codex CLI agent |
| [`copilot.json`](copilot.json) | `copilot` / `print` | GitHub Copilot CLI agent |
| [`opencode.json`](opencode.json) | `opencode` / `print` | OpenCode agent |
| [`qoder.json`](qoder.json) | `qoder` / `print` | Qoder CLI agent |

最小 AgentSpec 形态：

```json
{
  "actor": {
    "id": "actor_claude",
    "kind": "agent",
    "displayName": "Claude"
  },
  "instructions": "Agent-specific static instructions.",
  "providerRef": {
    "id": "claude",
    "mode": "print",
    "model": "sonnet"
  },
  "autostart": false
}
```

`instructions` 是这个 agent 自己的静态行为说明。Loom 会把它作为
`agent_instructions` prompt part 交给 provider manifest 的 prompt 组装规则；provider
决定它最终进入 system prompt、user prompt，还是完整 prompt。

`providerRef.model` 和 `providerRef.reasoningEffort` 是具体 agent 的偏好。provider
manifest 负责声明这些值如何映射成 CLI 参数，例如 `--model {model}`。
