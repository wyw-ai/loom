# ProviderManifest 样例

这里的 JSON 描述 provider 如何启动、如何组装 prompt、如何解析 stdout/stderr、如何处理
session/resume。它们不创建 actor；具体 agent 只在 AgentSpec 里通过 `providerRef`
引用 provider。

内置 provider 的权威样例可以直接由 CLI 生成：

```bash
loom provider example --claude --json
loom provider example --codex --json
loom provider example --copilot --json
loom provider example --kimi --json
loom provider example --opencode --json
loom provider example --qoder --json
loom provider example --zcode --json
```

不带 provider 参数时，`loom provider example` 会输出带字段解释的通用模板。添加到本机
daemon 配置前建议先校验：

```bash
loom provider validate examples/providers/claude.json
loom provider add examples/providers/claude.json
```

边界规则：

- `ProviderManifest` 保存 command/args/env、prompt outputs、parser、session 策略。
- `AgentSpec` 保存 actor metadata、instructions、model/reasoning 选择和 `providerRef`。
- 不再使用旧的 `provider + transport + actors[]` 文件形态。

Claude 示例里的一个 provider manifest 同时声明 `print` 和 `nonprint` 两个 mode。
对应的两个 agent 示例分别通过 `providerRef.mode` 选择具体接入方式。

所有内置 command provider 的 `print` mode 默认使用 `timeoutMs: -1` 和
`idleTimeoutMs: -1`，表示 Loom 不对单次运行施加总时长或空闲时长限制；运行仍可由用户
主动取消。需要限制时，用户可以定义一个继承内置 provider 的本地 variant，并用正整数
毫秒值覆盖任一字段：

```json
{
  "schemaVersion": 1,
  "id": "codex_timeout_2h",
  "displayName": "Codex CLI (2h timeout)",
  "extends": "codex",
  "modes": {
    "print": {
      "timeoutMs": 7200000,
      "idleTimeoutMs": -1
    }
  }
}
```
