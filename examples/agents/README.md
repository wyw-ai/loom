# Agent spec 样例

这里是几份可直接复用的 ACP agent spec，对应仓库 README「配置 agent」章节里
「手写 spec」的格式。把任意一份拷到 `~/.config/joi/agents/` 即可被
`joi agent serve` 加载（或运行 `joi agent serve --specs ./examples/agents`
直接读这个目录）。

| 文件 | 命令 | 备注 |
| --- | --- | --- |
| [`actor_claude_nonprint.json`](actor_claude_nonprint.json) | `claude` | Claude Code `interactive_command` 非 `--print` 模式；依赖 sentinel 判断完成并在完成后关闭 provider 进程 |
| [`actor_codex.json`](actor_codex.json) | `npx -y @zed-industries/codex-acp` | Zed 维护的 Codex ACP 包 |
| [`actor_opencode.json`](actor_opencode.json) | `opencode acp` | 需要本机已装 `opencode` CLI |
| [`actor_qoder.json`](actor_qoder.json) | `npx -y @qoder-ai/qodercli@0.1.48 --acp` | Qoder ACP 模式；与 Zed registry 当前版本对齐 |

Qoder 的 spec 需要跟 Zed registry 使用的 ACP 包版本保持一致。Zed 当前配置
为 `@qoder-ai/qodercli@0.1.48`；如果 Joi 仍使用旧版
`@qoder-ai/qodercli@0.1.36`，可能会出现 Zed ACP 可用但 Joi ACP 仍提示
`Authentication required` 并重新打开浏览器登录的情况。

Qoder 会根据客户端声明的 terminal auth 能力返回登录命令。Joi 会优先执行
Qoder 返回的 `_meta.terminal-auth` 命令；如果登录态失效，按日志提示重新
登录后重试。

每份 spec 的 `env` 都留空了。如果你的网络环境需要走代理，自己加
`http_proxy` / `https_proxy` / `all_proxy` 即可，例如：

```json
"env": {
  "http_proxy": "http://127.0.0.1:7897",
  "https_proxy": "http://127.0.0.1:7897",
  "all_proxy": "socks5://127.0.0.1:7897"
}
```

如果一个 agent runtime 支持在 ACP `session/new` 中指定模型，可以在 spec
里声明模型菜单。之后在聊天框发送 `@actor_id /models`，Joi 会弹出选择卡片，
并把选择结果保存到该 actor 的 profile，下次创建 ACP session 时带上选中的
`model`：

```json
"models": {
  "default": "provider/model-id",
  "choices": [
    { "id": "provider/model-id", "label": "Default model" },
    { "id": "provider/fast-model-id", "label": "Fast model" }
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
