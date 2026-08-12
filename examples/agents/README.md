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
| [`actor_claude_stream.json`](actor_claude_stream.json) | `claude` / `print` | Claude Code `-p --output-format stream-json` 模式，AgentSpec 组装 system/user，provider manifest 只负责把它们映射到 CLI 参数 |
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
  "wake": {
    "coalesce": true,
    "debounceMs": 750,
    "replyReminder": "first-turn",
    "onHumanMessageWhileBusy": "queue",
    "contextTokenBudget": 900
  },
  "autostart": false
}
```

`instructions` 是这个 agent 自己的静态行为说明。Loom 会把它和 actor/channel 级基础
上下文写入当前 workspace 的 `AGENTS.md`。默认 provider 不再通过 system prompt 参数
注入 Loom 规则；每个 turn 只把当前动态输入按 `AgentSpec.promptAssembly` 渲染成
`prompt.full` 等输出交给 CLI。

每个 agent workspace 默认也会投影官方 `loom` skill。这个 skill 只负责场景指路：
需要更详细的运行规则时，agent 应通过 `loom guide` 读取官方 guide。

`providerRef.model` 和 `providerRef.reasoningEffort` 是具体 agent 的偏好。provider
manifest 负责声明这些值如何映射成 CLI 参数，例如 `--model {model}`。

`wake` 控制 turn intake 行为：

- `coalesce` 合并忙碌期间积压的兼容消息；
- `debounceMs` 在 dispatch 前等待短窗口，吸收连续分段输入；
- `replyReminder` 控制每回合 reply contract 的重复频率；
- `onHumanMessageWhileBusy` 可选 `queue`、`cancel_and_requeue`、`inject`；
- `contextTokenBudget` 限制 bootstrap / pending delivery 上下文预算，超出时转为
  `unreadGap` 提示；
- `turnInputStyle` 可选 `minimal`（默认，紧凑纯文本消息清单）或 `structured`
  （turn-input-contract v1 JSON header + fenced bodies）。

GUI 的 Wake Policy 提供三个预设（映射到上述字段）：排队+合并
（`coalesce=true, debounceMs=750, busy=queue`）、排队+逐条
（`coalesce=false, debounceMs=0, busy=queue`）、打断+追加
（`coalesce=true, debounceMs=250, busy=cancel_and_requeue`）。

如果 agent 配置了 `bundle.source`，Loom 会把 bundle 安装到该 agent home 下，并把当前
scope 可见的 agent bundles 以 symlink 方式挂到该 agent 自己的
`{loom_agent_home}/workspace/scopes/<kind>/<scope>/` 下。provider manifest 再通过
`--add-dir {agent.skillWorkspace}` 或 provider 专属配置把这个外挂路径接入，让 Codex、
Copilot、Claude Code、Qoder、OpenCode 按各自的原生规则发现同一批 scope skills。
