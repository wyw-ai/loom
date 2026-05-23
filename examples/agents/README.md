# Legacy Agent provider spec 样例

这些文件保留为旧版 provider spec schema 参考。当前运行路径只保留
`loom-daemon`：daemon 从 `~/.loom-apps/desktop.toml` 的 machine agent 配置自动合成
runtime spec，不再加载这个目录。

| 文件 | 命令 | 备注 |
| --- | --- | --- |
| [`actor_claude_nonprint.json`](actor_claude_nonprint.json) | `claude` | Claude Code `interactive_command` 非 `--print` 模式；依赖 sentinel 判断完成并在完成后关闭 provider 进程 |
| [`actor_codex.json`](actor_codex.json) | `npx -y @zed-industries/codex-acp` | Zed 维护的 Codex ACP 包 |
| [`actor_opencode.json`](actor_opencode.json) | `opencode acp` | 需要本机已装 `opencode` CLI |
| [`actor_qoder.json`](actor_qoder.json) | `npx -y @qoder-ai/qodercli@0.1.48 --acp` | Qoder ACP 模式；与 Zed registry 当前版本对齐 |

Qoder 的旧 provider spec 需要跟 Zed registry 使用的 ACP 包版本保持一致。Zed 当前配置
为 `@qoder-ai/qodercli@0.1.48`；如果 Loom 仍使用旧版
`@qoder-ai/qodercli@0.1.36`，可能会出现 Zed ACP 可用但 Loom ACP 仍提示
`Authentication required` 并重新打开浏览器登录的情况。

Qoder 会根据客户端声明的 terminal auth 能力返回登录命令。Loom 会优先执行
Qoder 返回的 `_meta.terminal-auth` 命令；如果登录态失效，按日志提示重新
登录后重试。

每份旧 provider spec 的 `env` 都留空了。如果你的网络环境需要走代理，自己加
`http_proxy` / `https_proxy` / `all_proxy` 即可，例如：

```json
"env": {
  "http_proxy": "http://127.0.0.1:7897",
  "https_proxy": "http://127.0.0.1:7897",
  "all_proxy": "socks5://127.0.0.1:7897"
}
```

如果一个 agent runtime 支持在 ACP `session/new` 中指定模型，可以在
`defaults.models` 或 actor 自己的 `models` 里声明模型菜单。之后在聊天框发送
`@actor_id /models`，Loom 会弹出选择卡片，
并把选择结果保存到该 actor 的 profile，下次创建 ACP session 时带上选中的
`model`：

```json
"defaults": {
  "models": {
    "default": "provider/model-id",
    "choices": [
      { "id": "provider/model-id", "label": "Default model" },
      { "id": "provider/fast-model-id", "label": "Fast model" }
    ]
  }
}
```

command / interactive_command runtime 可以通过 `transport.modelArgs` 声明 CLI
模型参数模板。内置 daemon provider 已经按各 CLI 填好 `["--model", "{model}"]`；
手写 spec 时可显式设置：

```json
"transport": {
  "kind": "command",
  "command": "codex",
  "args": ["exec", "--json"],
  "modelArgs": ["--model", "{model}"]
}
```

同一个 provider / CLI 可以在一份 JSON 里声明多个 actor，避免为同一套
`transport` 复制多份 spec。`defaults` 作为默认值，`actors[]` 里的 `identity`、
`model`、`models` 等字段按 actor 覆盖：

```jsonc
{
  "provider": { "id": "qoder", "displayName": "Qoder ACP" },
  "transport": {
    "kind": "acp_stdio",
    "command": "npx",
    "args": ["-y", "@qoder-ai/qodercli@0.1.48", "--acp"],
    "env": {}
  },
  "actors": [
    {
      "id": "actor_qoder_reviewer",
      "displayName": "Qoder Reviewer",
      "model": "provider/model-strong",
      "identity": {
        "description": "Code review actor",
        "scaffold": {
          "identity": "# Qoder Reviewer\n\n- Role: review changes and call out risks."
        }
      }
    },
    {
      "id": "actor_qoder_builder",
      "displayName": "Qoder Builder",
      "model": "provider/model-fast",
      "identity": {
        "description": "Implementation actor",
        "scaffold": {
          "identity": "# Qoder Builder\n\n- Role: implement scoped changes."
        }
      }
    }
  ]
}
```

`cwd` / `env` 里可以用的模板变量（`{agent.workspace}` 等）见根目录 README
「配置 agent」一节。Command transport（`claude -p` 这种一次性 CLI）的写法
见 [`docs/command-transport-v0.md`](../../docs/command-transport-v0.md)。

`actor_claude_nonprint.json` 使用 Claude Code 的普通交互模式，不带 `--print`。
如果你需要指定独立的 Claude settings 文件，可以把 `provider.settings` 改成：

```json
"settings": {
  "mode": "custom",
  "path": "/path/to/settings.json"
}
```
