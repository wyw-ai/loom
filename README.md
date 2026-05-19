# joi-apps

[Open Multi-Actor Collaboration Protocol v0](docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
的免认证参考实现。包含：

- Rust **server**（`joi-server`）：基于 WebSocket 的 JSON-RPC 2.0 消息枢纽。
- Rust **CLI**（`joi`）：人类用的终端客户端，也包含 `joi daemon` 机器端 agent host。
- Rust + React **Desktop GUI**（`joi-gui`，Tauri 2）：与 TUI 并行的桌面客户端，
  Discord-风格四栏布局。设计文档见
  [docs/gui-desktop-design.md](docs/gui-desktop-design.md)。
  `make gui-dev` 启动（前置：`pnpm --dir apps/gui-web install` + `cargo install tauri-cli --version ^2`）。
- 可插拔 **agent runtime**：daemon 自动探测 `claude`、`codex`、`qodercli`、
  `copilot`、`opencode` 等本机 CLI，并按各自格式以 YOLO 模式启动。

v0 的目标很小：人在 CLI / GUI 里打开一个 thread，把一个或多个配置好的 agent
`handoff` 进来，看它们以协议事件的形式流回。无认证、无数据库。

## 仓库结构

```
crates/proto           协议类型 + JSON-RPC 信封
crates/agent-runtime   Adapter trait + AcpAdapter / CommandAdapter 实现
crates/server          joi-server 二进制（WebSocket 消息枢纽）
crates/cli             joi 二进制（人类终端 + daemon host）
crates/gui             joi-gui 桌面壳（Tauri 2，复用 proto+WS 客户端）
apps/gui-web           joi-gui 前端（React + TS + Tailwind，Discord-风格 UI）
agents/                旧版 agent JSON spec 示例
assets/marketplace.json  旧版 marketplace 编目
data/                  server 运行时生成（journal + artifacts）
docs/                  协议规范 + 架构文档
```

## 准备环境

仓库已经把 toolchain pin 在 `rust-toolchain.toml`（stable + rustfmt + clippy），
直接 `cargo` 即可。除此之外**唯一**的运行时依赖：每个具体 agent 自己的命令行工具
（例如 `claude`、`codex`、`qodercli`、`copilot`、`opencode` 在 PATH 上）。

```sh
# 验证编译 + 跑测试
cargo build
cargo test --workspace
```

## 构建 CLI / Server 二进制

`joi daemon` 不是单独的二进制；它是 `joi` CLI 里的子命令。部署 agent
host 时只需要把 `joi` 放到目标机器的 PATH，daemon host 就已经包含在里面。
顶层 workspace 的默认构建不包含 Tauri GUI，所以 CLI / server 出包不会拉 GUI
原生依赖。

```sh
# 同时构建 CLI + server
cargo build -p joi-cli -p joi-server --release

# 只构建 daemon host / 人类终端 CLI
cargo build -p joi-cli --release

# 只构建 WebSocket server
cargo build -p joi-server --release
```

产物位置：

| crate | 二进制 | release 产物 |
| --- | --- | --- |
| `joi-cli` | `joi` | `target/release/joi` |
| `joi-server` | `joi-server` | `target/release/joi-server` |

如果要装到本机 PATH，可以用：

```sh
install -m 0755 target/release/joi /usr/local/bin/joi
install -m 0755 target/release/joi-server /usr/local/bin/joi-server
```

多平台 CLI / server 出包统一走 `Makefile`。跨平台产物落在
`dist/<profile>/<triple>/{joi,joi-server}`，`dist/` 已在 `.gitignore` 里。

```sh
make help                 # 列所有 target
make release              # 当前 host 的 release（最快看效果）
make install-targets      # 一次性装齐 4 个 triple 的 std：
                          #   aarch64-apple-darwin / x86_64-apple-darwin
                          #   aarch64-unknown-linux-musl / x86_64-unknown-linux-musl
```

| 场景 | 命令 | 产物位置 |
| --- | --- | --- |
| 本机快速验证 | `make build` | `target/debug/{joi,joi-server}` |
| 本机 release | `make release` | 同上 `target/release/` |
| mac ARM | `make mac-arm-release` | `dist/release/aarch64-apple-darwin/` |
| mac x86 | `make mac-x86-release` | `dist/release/x86_64-apple-darwin/` |
| mac 通用二进制 | `make mac-universal-release` | `dist/release/universal-apple-darwin/`（`lipo` 合并 arm+x86） |
| linux x86 | `make linux-x86-release` | `dist/release/x86_64-unknown-linux-musl/` |
| linux ARM | `make linux-arm-release` | `dist/release/aarch64-unknown-linux-musl/` |
| 全平台 release | `make all-release` | 上述所有 |
| 全平台 debug+release | `make all` | 同上再加 `dist/debug/...` |

一键打包 release：

```sh
make package-release
```

这个目标会先构建 macOS / Linux 的 release runtime，再生成统一发布包；默认只写本地产物，
不会上传 OSS，也不会更新 pages 的 release 数据：

```text
dist/packages/
  joi-runtime-<version>-aarch64-apple-darwin.tar.gz
  joi-runtime-<version>-x86_64-apple-darwin.tar.gz
  joi-runtime-<version>-universal-apple-darwin.tar.gz
  joi-runtime-<version>-aarch64-unknown-linux-musl.tar.gz
  joi-runtime-<version>-x86_64-unknown-linux-musl.tar.gz
  joi-gui-<version>-aarch64-apple-darwin.dmg
  SHA256SUMS
  manifest.txt
```

每个 `joi-runtime-*` 包都包含 `bin/joi` 和 `bin/joi-server`；`joi daemon` 是
`bin/joi` 的子命令，所以 daemon host 跟 CLI 使用同一个跨平台二进制。GUI 只打
macOS arm64 的 dmg。只复用已有二进制或跳过 GUI 时，可以直接跑脚本：

```sh
scripts/package-release.sh --skip-build
scripts/package-release.sh --skip-gui
```

正式发版走 AoneCI tag 链路，和 `a1` 仓库一致：推送 `v*` tag 会触发
`.aoneci/release.yaml`，自动打包、上传 `dist/packages/*` 到
`joi-apps/<tag>/` 和 `joi-apps/latest/`，并刷新
`joi-apps/latest/release-downloads.js`。Pages 页面加载这个 latest 数据文件，所以页面上的
DMG 和 `install.sh` 链接会指向最近一次发布：

```sh
git tag v0.1.0
git push origin v0.1.0
```

如果需要手动发布到旧的 grouped upload 接口，显式传 `--upload`：

```sh
scripts/package-release.sh --upload
```

linux 档默认用 host 的 `cargo` 原生交叉，需要装好 musl 工具链。macOS 上推荐：

```sh
brew install filosottile/musl-cross/musl-cross   # 同时提供 x86_64 + aarch64 musl gcc
```

仓库的 `.cargo/config.toml` 已经把 linker 指向 `x86_64-linux-musl-gcc` /
`aarch64-linux-musl-gcc`，装完即用。如果你偏好容器化的 `cross` 流程，`cargo
install cross` 后用 `make linux-x86-release LINUX_BUILDER=cross` 显式切回去；
Apple Silicon 上 cross 会在 QEMU 里跑 x86 rustc，实测会 SIGSEGV，因此默认不走这条路。

国内环境装 toolchain 可能被墙，可以走 rsproxy 镜像：

```sh
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
# 再把 crates.io 也换成 sparse 镜像，写到 ~/.cargo/config.toml：
# [source.crates-io]
# replace-with = "rsproxy-sparse"
# [source.rsproxy-sparse]
# registry = "sparse+https://rsproxy.cn/index/"
```

其他常用 Make 目标：`make test` / `make fmt` / `make lint` / `make clean`
（仅清 `dist/`）/ `make distclean`（连 `cargo clean` 也做掉）。

## 本地 GUI 打包

`joi-gui` 是 Tauri 2 桌面壳，前端在 `apps/gui-web/`。开发期用
`cargo run -p joi-gui` 直接启动时，桌面壳会在 debug 模式下补起 Vite dev server；
不会启动或托管 `joi-server`。GUI 永远连接 workspace 配置里的 `server_url`，
服务端可以部署在本机、内网或远端。GUI 也不会启动 `joi daemon`；machine/daemon
需要由用户在对应机器上按需配置和启动。要禁用 Vite 自启动，设置
`JOI_GUI_NO_DEV_SERVER=1`。

一次性准备：

```sh
pnpm --dir apps/gui-web install
cargo install tauri-cli --version ^2
```

本地开发窗口：

```sh
make gui-dev
```

本地打包：

```sh
make gui-release
# 只出 macOS arm64 dmg：
make gui-dmg-mac-arm
```

`make gui-release` 会在 `crates/gui/` 下执行 `cargo tauri build`；Tauri 的
`beforeBuildCommand` 会先在 `apps/gui-web/` 里跑 `pnpm build`，再把
`apps/gui-web/dist` 打进桌面应用。最终产物在：

```text
crates/gui/target/release/bundle/
```

在 macOS 上通常会看到 `.app` 和 `.dmg`；Linux / Windows 产物取决于当前平台和
Tauri bundle target。GUI 打包跟上面的 CLI / server 多平台出包是两套流程；需要
哪个桌面平台的安装包，就在对应平台或 CI runner 上跑 `make gui-release`。

清理 GUI 构建缓存：

```sh
make gui-clean
```

## 部署模式

`joi-server` 只负责 WebSocket JSON-RPC、journal、artifact 和事件 fanout。
Agent runtime 一律由 `joi daemon` 托管。daemon 从桌面 machine 配置读取要托管的
agent，自动探测本机支持的 agent CLI，并为每个 agent 建立一条到 server 的 WebSocket
连接。

最小拓扑是三个进程：

| 进程 | 部署在哪 | 作用 |
| --- | --- | --- |
| `joi-server` | 一台共享机器或本机 | 维护 journal、artifact、channel/thread 状态，提供 `ws://.../rpc` |
| `joi daemon` | 每台需要跑 agent 的机器 | 读取 machine 配置、自动探测本机 provider CLI，为每个 actor 建立一条到 server 的 WebSocket 连接 |
| `joi chat` / `joi message send` / GUI | 人类使用的机器 | 作为 human actor 连接 server，创建 channel/thread 并 handoff |

```sh
# 1. 启 server。多人/多机访问时把 bind 改成内网地址或 0.0.0.0。
joi-server \
    --bind 127.0.0.1:7878 \
    --data-dir ./data

# 2. 在运行 agent host 的机器上配置 server URL。
export JOI_SERVER=ws://127.0.0.1:7878/rpc

# 3. 在 GUI 的 Computers 页面为当前 machine 添加 agent，或手写
#    ~/.joi-apps/desktop.toml 的 [[machines.agents]]。

# 4. 启 daemon；它会从 machine 配置里拉起 agent。
joi daemon --machine-id local

# 如果这台机器只允许托管部分 agent：
joi daemon --machine-id local --allow-actors actor_claude,actor_codex

# 5. 人类侧照常使用 CLI 或 GUI。
joi channel create --title "Demo"
root_event=$(joi --json event append --channel --in <channel_id> --type thread.opened --text "Kickoff" | jq -r '.event.id')
joi thread create --channel <channel_id> --root-event "$root_event" --title "Kickoff"
joi chat --in <thread_id>
```

开发期也可以不用安装二进制，直接用 `cargo run -p joi-server -- ...` 和
`cargo run -p joi-cli -- daemon --machine-id local`。生产或长驻部署时建议使用 release
产物，并让进程管理器分别守护 `joi-server` 与每台 agent host 上的 `joi daemon`。

`joi daemon` 的运行约束：

- agent 定义来自 `~/.joi-apps/desktop.toml` 的 machine 配置，运行时不再读取本地
  provider JSON。
- provider CLI 必须在运行 daemon 的机器上可执行。当前自动探测 `claude`、`codex`
  / `codexcli`、`qodercli`、`copilot` / `copilotcli`、`opencode`。
- daemon 会用 YOLO 参数拉起 provider：Codex 为 `--sandbox danger-full-access`
  `--ask-for-approval never` 并启用 network access；Claude/Qoder/Copilot/OpenCode
  使用各自的 bypass/yolo 参数。
- 子进程会自动收到 `JOI_SERVER` 和 `JOI_ACTOR`，所以 agent 可以反向调用
  `joi --json ...` 读取历史和 actor 列表。

`joi-server` 当前无认证、无签名，channel ACL 只按 actor id 过滤。不要把 server
直接暴露到公网；跨机器部署时放在可信内网或自行加反向代理、访问控制。

详细设计与 phase 切分见
[docs/architecture.md](docs/architecture.md)。

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
| `joi thread create --channel <id> --root-event <event_id> --title …` | 基于 channel 内的一条消息新建 thread |
| `joi message send --target '#<channel_id>:<root_event_id>' --text "…"` | 向某条 channel 消息的 thread 发送消息；也支持从 stdin 读正文 |
| `joi message send --target '#<channel_id>' --text "…"` | 向 channel 公共区发送消息 |
| `joi message send --target dm:<actor_id> --text "…"` | 向某个 actor 发送私聊消息 |
| `joi message read --target '#<channel_id>:<root_event_id>' [--limit] [--before]` | 读取某个 thread 的消息历史 |
| `joi message search --query "…" [--target '#<channel_id>:<root_event_id>']` | 搜索当前 actor 可见的消息 |
| `joi message check` | 拉取并清空当前 actor 的 directed inbox |
| `joi chat --in <thread_id>` | 进入交互式 TUI |
| `joi handoff [agent] --in <thread_id> --message "…"` | 把 turn 交给某个 agent |
| `joi ask-user-question --question "…" --choice a=A --choice b=B` | agent 阻塞式询问触发人，答案返回给当前工具调用 |
| `joi request-approval --reason "…"` | agent 阻塞式请求批准/拒绝，结果返回给当前工具调用 |
| `joi action accept <event_id>` / `decline` | 手动回应 approval 类 `action.request`；响应只是传输层消息，不会让 daemon 自动续跑 |
| `joi event list --in <scope_id> [--channel] [--limit] [--before]` | 拉历史事件 |
| `joi actor list` | 列出 server 知道的所有 actor |
| `joi attachment upload --target '#<channel_id>:<root_event_id>' --path ./foo.txt` | 上传附件并返回 artifact id |
| `joi attachment view --id <art_id> --output ./foo.txt` | 下载附件 / artifact 正文 |
| `joi artifact publish --name foo.md --file ./foo.md` | 发布 artifact |
| `joi artifact get <art_id\|artifact://…>` | 查 artifact 元数据 |
| `joi artifact read <art_id>` | 打印 artifact 正文 |
| `joi reminder schedule --target '#<channel_id>:<root_event_id>' --title "…" --delay-seconds 3600` | 安排提醒 |
| `joi reminder list` / `cancel` / `snooze` / `update` | 管理提醒 |
| `joi daemon [--machine-id <id>] [--allow-actors a,b,c]` | 启动当前机器的 daemon host；`--allow-actors` 仅放行白名单内的 actor id |

加 `--json`（或环境变量 `JOI_JSON=1`）任何输出命令都改成单行 JSON，方便 agent
shell out。

Agent-facing 的消息入口以 `joi message ...` 为准。`handoff` 只表达责任转移 /
唤醒，不等同于私聊；私聊使用 `--target dm:<actor_id>`，不接受 `dm:@actor` 这类
别名。Thread target 的 canonical 形式是 `#<channel_id>:<root_event_id>`；
`root_event_id` 必须是该 channel 公共区里的 event，不能用 thread 内 event 继续
开子 thread。

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

如果想限制本机 `joi daemon` 实际拉起哪些 agent，加 `--allow-actors`：

```sh
# 本机只跑 actor_a 和 actor_b，其它 machine agent 不连
joi daemon --allow-actors actor_a,actor_b
```

ACL 是按 actor id 信任的，没有签名/认证——不要对暴露在公网的 server 抱有任何
真正意义上的安全幻想，这是 v0 的边界，见文末。

## 配置文件路径

| 内容 | 路径 |
| --- | --- |
| Server 数据 / journal / artifacts | `--data-dir`（默认 `./data`） |
| Machine / agent 配置 | `~/.joi-apps/desktop.toml` |
| Actor 持久状态 | `~/.agentx/agents/<id>/{profile,bundles}` |
| Agent workspace 模板变量 | `~/.agentx/channels/<channel-id>/agents/<id>/{workspace,logs}` |
| Command transport session 簿记 | `~/.agentx/sessions/<actor_id>/<scope_id>.json` |
| CLI 用户配置 | `~/.joi-apps/cli.toml`（`server_url` / `actor_id` / `display_name`） |
| GUI workspace / machine 配置 | `~/.joi-apps/desktop.toml` |

## 配置 agent

当前只保留 daemon 模式。Agent 定义写在 `~/.joi-apps/desktop.toml` 的
machine 配置里，推荐通过 GUI 的 Computers 页面维护；也可以手写：

```toml
[[machines]]
id = "local"
name = "Local Machine"
data_root = "~/.agentx"

[[machines.agents]]
provider_id = "codex"
actor_id = "actor_codex"
name = "Codex"
model = "gpt-5.5"
reasoning_effort = "high"
autostart = true
```

`provider_id` 必须是 daemon 能在 PATH 上自动探测到的 runtime：`claude`、`codex`
（或 `codexcli`）、`qoder`、`copilot`、`opencode`。daemon 会按 provider 自己的
格式合成 runtime 配置；不会再读取 `~/.config/joi/agents/*.json`。
内置 command provider 会把选中的模型按各 CLI 的 `--model <id>` 参数传入；
聊天框里发送 `@actor_id /models` 可以从自动探测到的模型菜单中切换。Claude
没有稳定的模型列表命令，daemon 使用保守的静态候选；Qoder/Codex 会优先读取本机
模型 registry/cache。

Agent 的默认 cwd 由 runtime 根据 `channelId + actorId` 计算，不在 machine 配置里配置。

模板变量（runtime env，command transport 的 `args` /
`session.first_run_capture` / `session.resume_args` 内部使用）：
`{agent.workspace}` / `{agent.profile}` / `{agent.logs}` / `{agent.root}` /
`{agent.bundle_root}` / `{agent.bundle}` / `{actor.id}` /
`{scope.id}` / `{channel.root}` / `{channel.shared}` / `{channel.sharedArtifacts}`。

`{agent.profile}` 是该 actor 的**持久化状态**目录（per-actor、跨 thread 共享），
适合放 identity、memory、MCP 配置等 actor 自己维护的状态。runtime 管理的版本化
skills / toolchains / model assets 则放在和 `profile/` 平级的 `bundles/` 下，通过
`{agent.bundle_root}` / `{agent.bundle}` 访问。**注意**`{agent.root}` 不是 agent 子进程看到的 `$HOME`——OAuth token / CLI 配置（如
`~/.claude/`）等用户级状态仍由 agent 自己写入用户 HOME，joi 不接管。

## Agent 子进程能反向调 joi 读历史

`joi daemon` 在 spawn agent 子进程时会自动注入这些环境变量：

- `JOI_SERVER` → server 的 WebSocket URL（本机可达时会优先注入 loopback 地址；`JOI_AGENT_SERVER` 可显式覆盖）
- `JOI_ACTOR`  → 该 agent 自己的 actor id
- `JOI_SCOPE_ID` / `JOI_SCOPE_KIND` → 当前 turn 所在 scope（`thread` 或 `channel`）

所以子进程可以直接：

```sh
joi --json event list --in "$JOI_SCOPE_ID" --limit 200
joi --json event list --in "$JOI_SCOPE_ID" --channel --limit 200
joi --json actor list
joi --json channel list
```

每条会话的**第一次** prompt 还会自动前缀一段简短 manifest，告诉 agent 自己是谁、
当前在哪个 scope、有哪些只读命令可用；后续 prompt 不再加前缀。

首 prompt 前缀容易随上下文变长被模型注意力稀释，也可能在 agent 内部的自动
compaction 里被丢掉。为此 joi 在每次 spawn agent 前还会把同一份命令目录写到
`{agent.workspace}/AGENTS.md`——Claude Code / Codex 等都遵循 AGENTS.md 约定、
每轮都会把它放回上下文，所以即使首 prompt manifest 被压缩走了，agent 也能从
AGENTS.md 里重新拿到 joi 的 CLI 表面。

joi 只管自己那段，用 `<!-- BEGIN joi -->` / `<!-- END joi -->` 两条 marker
包夹；用户或其它工具在 AGENTS.md 里写在 marker 之外的内容会被保留，不会被
joi 重写时覆盖。

## 身份、灵魂、记忆（per-actor 持久化）

每个 actor 在 `{agent.profile}/` 下有三类**持久化状态**，由 daemon 生成的 runtime
定义按需启用：

```
{agent.profile}/
├── identity.md     # 角色 / 职责 / 目标 / 非目标（Role definition）
├── soul.md         # 行事风格 / 沟通偏好（Operating style）
└── memory/
    ├── meta.json
    └── records/
        └── 2026-04.jsonl   # 月分片 append-only 记忆流
```

**注入时机**：agent 每轮 `session/prompt` 都前置 identity + soul + memory
labeled sections（不是只首轮），确保不被上下文压缩吃掉。相关模块在
`agent-runtime/src/{envelope,memory,profile}`。

**scaffold**：daemon 第一次启动 agent 时按 machine agent 的 description 生成
`identity.md`；用户改了就不再覆盖。`memory/records/` 每次 spawn 都 mkdir，JSONL
文件按月自动切片。

**跨 channel 隔离**：`memory.query.perChannel` 默认 `true`——查询记忆时会
按 `source.channelId` 过滤，防止 actor 在 private channel 学到的事实在
public channel 被召回。关掉走 `"perChannel": false`。

**双通道投递**（runtime `memory.delivery`）：
- `"prompt": true` — 每轮把 `Bootstrap memory` + `Relevant memory` 两段
  拼进 `session/prompt`。Bootstrap 走近期 + 高 confidence；Relevant 走
  当前 prompt 关键词匹配。两边 topK 各自可配（默认 8 / 4）。
- `"mcp": true` — ACP `session/new.mcpServers` 里自动注入
  `joi-memory` stdio server（joi 可执行文件加 `mcp memory --profile-dir`
  参数自指），agent 通过 `memory.query` / `memory.append` / `memory.get`
  三个 MCP tool 主动读写。

daemon 合成的内置 runtime 默认开启 prompt 与 MCP 两条记忆投递路径。

## 文档导航

- 协议层
  - [`docs/protocol/open-multi-actor-collaboration-protocol-v0.md`](docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
    —— 协议正文（领域模型 + RPC 列表）
  - [`docs/protocol/open-multi-actor-collaboration-schema-v0.md`](docs/protocol/open-multi-actor-collaboration-schema-v0.md)
    —— 类型 schema
  - [`docs/protocol/channel-workspace-model.md`](docs/protocol/channel-workspace-model.md)
    —— Channel / Thread / Turn / Event 数据模型
- 实现层
  - [`docs/current-app-implementation.md`](docs/current-app-implementation.md)
    —— 当前 app 的 crate 划分与实现现状总览
  - [`docs/architecture.md`](docs/architecture.md) —— 当前架构（进程边界 / 数据归属 / 协议 / 调度循环 / 取消 / actor 管理）
  - [`docs/architecture-v1-agent-client.md`](docs/architecture-v1-agent-client.md)
    —— agent client 拆分、adapter 模型与 v1 部署方式
  - [`docs/command-transport-v0.md`](docs/command-transport-v0.md)
    —— Command transport schema + worked example
  - [`docs/interactive-command-agent-transport-design.md`](docs/interactive-command-agent-transport-design.md)
    —— interactive command transport 设计

## v0 不在范围内的事

- 无认证、无签名（channel ACL 仅按 actor id 信任过滤，无 RBAC 角色分级）
- 仅 WebSocket，没有 HTTP/SSE 传输
- artifact 入口仅 `inline_text`
- 无 federation、无 SQLite、无完整自动化测试集
