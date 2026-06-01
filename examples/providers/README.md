# ProviderManifest 样例

这里的 JSON 描述 provider 如何启动、如何组装 prompt、如何解析 stdout/stderr、如何处理
session/resume。它们不创建 actor；具体 agent 只在 AgentSpec 里通过 `providerRef`
引用 provider。

内置 provider 的权威样例可以直接由 CLI 生成：

```bash
loom provider example --claude --json
loom provider example --codex --json
loom provider example --copilot --json
loom provider example --opencode --json
loom provider example --qoder --json
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
