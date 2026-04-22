# joi-apps

[Open Multi-Actor Collaboration Protocol v0](docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
的免认证参考实现。包含：

- Rust **server**（`joi-server`）：基于 WebSocket 的 JSON-RPC 2.0 消息枢纽。
- Rust **CLI**（`joi`）：人类用的终端客户端，也可作为 v1 模式下的 agent 客户端常驻进程。
- 可插拔 **agent runtime**：支持 ACP 协议子进程（如 `claude-acp`、`codex-acp`）以及
  一次性 CLI（`claude -p`、`codex` 等）两类 transport。

v0 的目标很小：人在 CLI 里打开一个 thread，把一个或多个配置好的 agent `handoff`
进来，看它们以协议事件的形式流回。无认证、无 GUI、无数据库。

## 仓库结构

```
crates/proto           协议类型 + JSON-RPC 信封
crates/agent-runtime   Adapter trait + AcpAdapter / CommandAdapter 实现
crates/server          joi-server 二进制（WebSocket + 嵌入式 supervisor）
crates/cli             joi 二进制（人类终端 + v1 agent 客户端）
agents/                示例 agent JSON spec
assets/marketplace.json  内置 marketplace 编目
data/                  运行时生成（journal + artifacts + agent workspace）
docs/                  协议规范 + 架构文档
```

## 准备环境

仓库已经把 toolchain pin 在 `rust-toolchain.toml`（stable + rustfmt + clippy），
直接 `cargo` 即可。除此之外**唯一**的运行时依赖：每个具体 agent 自己的命令行工具
（例如 `claude-acp` 走 `npx`，`claude` 走二进制 PATH）。

```sh
# 验证编译 + 跑测试
cargo build
cargo test --workspace
```

## 部署模式

joi-apps 当前支持两种拓扑，由 server 端的 `JOI_DISABLE_EMBEDDED_RUNTIME`
环境变量切换。两者**不可同时运行**——会在 `turn/open` 上互相抢占。

### 模式 A：v0 嵌入式（默认，最省事）

server 内嵌 agent supervisor，启动时扫描 `--agents-dir` 下的 `*.json` spec、
按需 spawn ACP 子进程。

```sh
# 终端 1：启 server
cargo run -p joi-server -- \
    --bind 127.0.0.1:7878 \
    --data-dir ./data \
    --agents-dir ./agents

# 终端 2：装一个 agent + 开 chat
cargo run -p joi-cli -- agent install claude-acp \
    --actor-id actor_claude --name "Claude"
cargo run -p joi-cli -- channel create --title "Demo"
cargo run -p joi-cli -- thread create --channel <channel_id> --title "Kickoff"
cargo run -p joi-cli -- chat --in <thread_id>
```

### 模式 B：v1 拆分式（外置 agent 客户端）

server 退化为纯消息枢纽；agent runtime 由独立的 `joi agent serve` 进程托管，
通过 WebSocket 跟 server 通信。每个被管理的 agent 在 server 上是一条独立连接。
适合：多机部署、异构 agent 接入（命令行 + ACP 混用）、想 ship 第三方 adapter。

```sh
# 终端 1：启 server，关掉嵌入 supervisor
JOI_DISABLE_EMBEDDED_RUNTIME=1 cargo run -p joi-server

# 终端 2：把 agent spec 放到 agent-client 配置目录
mkdir -p ~/.config/joi/agents
cp agents/*.json ~/.config/joi/agents/

# 终端 3：启 agent client；它会为每个 spec 起一条到 server 的连接
cargo run -p joi-cli -- agent serve

# 终端 4：照常用 chat
cargo run -p joi-cli -- chat --in <thread_id>
```

详细设计与 phase 切分见
[docs/architecture-v1-agent-client.md](docs/architecture-v1-agent-client.md)。

## CLI 速览

`joi --help` 列出所有子命令。常用流程：

| 命令 | 作用 |
| --- | --- |
| `joi who` | 显示当前 server URL / actor / 配置文件路径 |
| `joi channel create --title …` | 新建 channel（默认 **private**，仅 creator 可见） |
| `joi channel list` | 列出可见 channel（public + 自己是 member 的 private） |
| `joi channel invite <channel_id> <actor_id>` | 把 actor 加进 channel |
| `joi channel revoke <channel_id> <actor_id>` | 把 actor 移出 channel |
| `joi channel members <channel_id>` | 打印当前成员表 |
| `joi thread create --channel <id> --title …` | 在 channel 下新建 thread |
| `joi chat --in <thread_id>` | 进入交互式 TUI |
| `joi say <text> --in <thread_id>` | 一次性发一条消息（脚本用） |
| `joi handoff [agent] --in <thread_id> --message "…"` | 把 turn 交给某个 agent |
| `joi action accept <event_id>` / `decline` | 回应 ACP 提出的 `action.request` |
| `joi event list --in <scope_id> [--channel] [--limit] [--before]` | 拉历史事件 |
| `joi actor list` | 列出 server 知道的所有 actor |
| `joi artifact publish --name foo.md --file ./foo.md` | 发布 artifact |
| `joi artifact get <art_id\|artifact://…>` | 查 artifact 元数据 |
| `joi artifact read <art_id>` | 打印 artifact 正文 |
| `joi agent serve [--specs <dir>] [--allow-actors a,b,c]` | v1：启动 agent client；`--allow-actors` 仅放行白名单内的 actor id |

加 `--json`（或环境变量 `JOI_JSON=1`）任何输出命令都改成单行 JSON，方便 agent
shell out。

### Chat TUI 按键

| 按键 | 行为 |
| --- | --- |
| 普通文字 + `Enter` | 写一条 `content.add` 到当前 thread |
| `@<actor>` + `Enter` | handoff 给该 agent；若不在当前 channel 会先弹确认框邀请 |
| `/` 起头 | 弹出内联 slash-command 下拉 |
| `Tab` / `↑` / `↓` | 在下拉中导航 |
| `Enter`（下拉打开时） | 把命令模板填进输入框（不发送） |
| `/handoff` + `Enter` | 弹出目标选择器，选完发送 handoff |
| `/action` + `Enter` | 弹出待响应 `action.request` 列表 |
| `/agents` + `Enter` | 在历史区打印已注册 agent |
| `/invite` / `/members` | 操作当前 thread 所属 channel 的成员表 |
| `Ctrl-B` | 打开 / 关闭左侧 Channels / Threads / Members 三栏侧栏 |
| 侧栏 `Tab` | 在三栏间切焦点 |
| 侧栏 `n` / `r` / `d` | 在 Channels / Threads 栏新建 / 重命名 / 删除 |
| 侧栏 Members 栏 `i` / `I` / `x` | 选 actor 邀请 / 直接键入 actor id 邀请 / 移除选中成员 |
| `/quit` + `Enter` 或 `Ctrl-C` | 退出 |
| `PgUp` / `PgDn` / `End` | 翻历史 |

发出的消息显示 `⏳`，server echo 回来变 `✓`。

## Channel 隔离

每个 channel 现在有 **public / private** 两态：

- 旧 journal 里的 channel 反序列化为 `public`（任何 actor 可读写，全保后向兼容）。
- `joi channel create` 与 chat TUI 里新建的 channel 默认 **private**，creator 是
  唯一初始成员；`scope/read`、`scope/subscribe`、`event/append`、`channel/list`、
  `ws::fanout` 全部按 ACL gate，非成员看不到也写不进。
- 没有角色分级——只要是成员，都能 invite / revoke 别人；只有把自己 revoke 出
  最后一个成员的 channel 这种自废武功的操作会被拒。

如果想限制本机 `joi agent serve` 实际拉起哪些 agent，加 `--allow-actors`：

```sh
# 本机只跑 actor_a 和 actor_b，其它 spec 文件即便存在也不连
joi agent serve --allow-actors actor_a,actor_b
```

ACL 是按 actor id 信任的，没有签名/认证——不要对暴露在公网的 server 抱有任何
真正意义上的安全幻想，这是 v0 的边界，见文末。

## 配置文件路径

| 内容 | 模式 A（v0） | 模式 B（v1） |
| --- | --- | --- |
| Server 数据 / journal / artifacts | `--data-dir`（默认 `./data`） | 同左 |
| Agent spec | `--agents-dir`（默认 `./agents`） | `~/.config/joi/agents/`（`--specs <dir>` 可覆盖） |
| Agent workspace 模板变量 | `<data-dir>/agents/<id>/{workspace,cache,logs}` | `~/.local/share/joi/agent-client/agents/<id>/{workspace,cache,logs}` |
| Command transport session 簿记 | （仅 v1 用到） | `~/.local/share/joi/agent-client/sessions/<actor_id>/<scope_id>.json` |
| CLI 用户配置 | `~/.config/joi/config.toml`（`server` / `actor` / `display`） | 同左 |

## 配置 agent

三种添加方式——结果都是写一份 spec JSON 到 spec 目录（模式 A 是 `agents/`，
模式 B 是 `~/.config/joi/agents/`）。

### 1. 从内置 marketplace 装（推荐）

```sh
joi agent marketplace                         # 列出内置 6 个 ACP agent
joi agent install claude-acp \
    --actor-id actor_claude --name "Claude"
```

会按 `npx → uvx → 平台匹配的 binary` 顺序解析 PATH 上的可用项；不下载任何东西，
只写 spec 文件。

### 2. 交互式添加自定义命令

```sh
joi agent add
```

### 3. 手写 spec

**ACP transport**（长连接子进程）：

```json
{
  "actor": {
    "id": "actor_my_agent",
    "displayName": "My Agent",
    "kind": "agent",
    "capabilities": {}
  },
  "transport": {
    "kind": "acp_stdio",
    "command": "my-acp-binary",
    "args": [],
    "env": {},
    "cwd": "{agent.workspace}",
    "authMethod": null
  },
  "autostart": false
}
```

**Command transport**（一次性 CLI，例如 `claude -p`）：

完整 schema、`first_run_capture` 规则、`output_format` 翻译表与 worked example 见
[docs/command-transport-v0.md](docs/command-transport-v0.md)。

保存为 `agents/<actor-id>.json`，或运行时注册：`joi agent register <path>`。

模板变量（`cwd` / `env` 值里可用）：`{agent.workspace}` / `{agent.cache}` /
`{agent.logs}` / `{agent.root}` / `{actor.id}` / `{scope.id}`（command transport）。

## Agent 子进程能反向调 joi 读历史

server 或 agent client 在 spawn agent 子进程时会自动注入两个环境变量
（前提是 spec 自己没设）：

- `JOI_SERVER` → server 的 WebSocket URL
- `JOI_ACTOR`  → 该 agent 自己的 actor id

所以子进程可以直接：

```sh
joi --json event list --in <thread_id> --limit 200
joi --json actor list
joi --json channel list
```

每条会话的**第一次** prompt 还会自动前缀一段简短 manifest，告诉 agent 自己是谁、
当前在哪个 scope、有哪些只读命令可用；后续 prompt 不再加前缀。

## 文档导航

- 协议层
  - [`docs/protocol/open-multi-actor-collaboration-protocol-v0.md`](docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
    —— 协议正文（领域模型 + RPC 列表）
  - [`docs/protocol/open-multi-actor-collaboration-schema-v0.md`](docs/protocol/open-multi-actor-collaboration-schema-v0.md)
    —— 类型 schema
  - [`docs/protocol/channel-workspace-model.md`](docs/protocol/channel-workspace-model.md)
    —— Channel / Thread / Turn / Event 数据模型
- 实现层
  - [`docs/architecture.md`](docs/architecture.md) —— v0 当前架构（默认拓扑）
  - [`docs/current-app-implementation.md`](docs/current-app-implementation.md)
    —— v0 各 crate 实现现状
  - [`docs/architecture-v1-agent-client.md`](docs/architecture-v1-agent-client.md)
    —— v1 拆分设计与 phase 切分（已落 E1–E3）
  - [`docs/command-transport-v0.md`](docs/command-transport-v0.md)
    —— Command transport schema + worked example

## v0 不在范围内的事

- 无认证、无签名（channel ACL 仅按 actor id 信任过滤，无 RBAC 角色分级）
- 仅 WebSocket，没有 HTTP/SSE 传输
- artifact 入口仅 `inline_text`
- 传给 ACP 子进程的 `mcpServers` 永远为空——自带
- 无 GUI、无 federation、无 SQLite、无完整自动化测试集
