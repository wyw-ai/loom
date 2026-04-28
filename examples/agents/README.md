# Agent spec 样例

这里是几份可直接复用的 ACP agent spec，对应仓库 README「配置 agent」章节里
「手写 spec」的格式。把任意一份拷到 `~/.config/joi/agents/` 即可被
`joi agent serve` 加载（或运行 `joi agent serve --specs ./examples/agents`
直接读这个目录）。

| 文件 | 命令 | 备注 |
| --- | --- | --- |
| [`actor_codex.json`](actor_codex.json) | `npx -y @zed-industries/codex-acp` | Zed 维护的 Codex ACP 包 |
| [`actor_opencode.json`](actor_opencode.json) | `opencode acp` | 需要本机已装 `opencode` CLI |
| [`actor_qoder.json`](actor_qoder.json) | `npx -y @qoder-ai/qodercli --acp` | Qoder ACP 模式 |

每份 spec 的 `env` 都留空了。如果你的网络环境需要走代理，自己加
`http_proxy` / `https_proxy` / `all_proxy` 即可，例如：

```json
"env": {
  "http_proxy": "http://127.0.0.1:7897",
  "https_proxy": "http://127.0.0.1:7897",
  "all_proxy": "socks5://127.0.0.1:7897"
}
```

`cwd` / `env` 里可以用的模板变量（`{agent.workspace}` 等）见根目录 README
「配置 agent」一节。Command transport（`claude -p` 这种一次性 CLI）的写法
见 [`docs/command-transport-v0.md`](../../docs/command-transport-v0.md)。
