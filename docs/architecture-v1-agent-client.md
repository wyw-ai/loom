# 架构 v1：Agent Client 拆分 + Adapter 模型

> **状态**：server 内嵌 runtime 已删除。`loom-server` 只负责消息枢纽职责；
> agent runtime 由 `loom agent serve` 托管，adapter 代码在 `crates/agent-runtime/`。
> 配套规范见 [docs/command-transport-v0.md](command-transport-v0.md)。
> v0 当前实现见 [docs/architecture.md](architecture.md) 与
> [docs/current-app-implementation.md](current-app-implementation.md)。

## TL;DR — v1 部署快速上手

```bash
# 终端 1：启动 server
cargo run -p loom-server

# 终端 2：安装或注册 provider spec
cargo run -p loom-cli -- agent install claude-acp --actor-id actor_claude

# 终端 3：跑 agent client；它会为每个 actor 起一条 ws 连接
cargo run -p loom-cli -- agent serve

# 终端 4：照常使用 chat
cargo run -p loom-cli -- chat --in <thread_id>
```

server 不再读取 provider spec，也不会 spawn agent 子进程。

---

## 1. 背景：为什么拆 agent runtime 出去

### 1.1 v0 的耦合点

拆分前 `loom-server` 同时承担两个角色：

1. **消息枢纽**：维护 `Channel` / `Thread` / `Event` / `Relation` 这套协议对象，做
   scope routing、subscription、journal 持久化。
2. **Agent supervisor**：内嵌一整套 ACP runtime，
   按需 spawn ACP 子进程、把 `directed_to` 事件翻译成 `session/prompt`、再把 ACP
   流回的 `session/update` 翻译成 `message` / `action.request` / trace 帧。

这两个职责共享同一个内存对象图：server 启动时直接读 `agents/*.json`，supervisor
直接订阅 store，adapter 调用直接走进程内 mpsc channel。

### 1.2 这套耦合带来的问题

- **异构 agent 接入只能往 `runtime/` 里塞**。新增一种 transport（比如 `claude -p`
  这种命令式 CLI）就要改 server 二进制。
- **部署形态被锁死**。即便用户只想把 `loom-server` 跑在云上、本地只跑 agent，也做
  不到——因为 agent 的 spec、子进程、stdio 全都长在 server 上。
- **协议层被拉低**。本来 `message.send` + `connection/open` + `message stream`
  已经足够描述一个 actor 的接入；现在却需要 `agent/register` / `agent/start` /
  `agent/stop` / `agent/log` 这一堆只为内嵌 runtime 服务的 RPC。

### 1.3 v1 的目标

把 agent runtime 从 server 里剥出来，独立成一个进程（`loom agent serve`），通过
**Adapter trait** 抽象不同的 agent 接入方式。Server 退化为纯消息枢纽，不再感知
agent 怎么被调度。

---

## 2. 目标拓扑

### 2.1 三节点示意

```mermaid
graph LR
    subgraph "loom-server (message hub)"
        S[event store<br/>scope routing<br/>journal]
    end

    subgraph "loom (human client)"
        H1[loom chat]
        H2[loom message read]
        H3[loom artifact ...]
    end

    subgraph "loom agent serve (agent client)"
        A[adapter dispatch]
        A --> A1[AcpAdapter<br/>opencode/claude-acp/...]
        A --> A2[CommandAdapter<br/>claude -p / shell scripts]
        A --> A3["future: McpAdapter / ..."]
    end

    H1 -- "WS / JSON-RPC" --> S
    H2 -- "WS / JSON-RPC" --> S
    H3 -- "WS / JSON-RPC" --> S
    A  -- "WS / JSON-RPC<br/>(one connection per managed actor)" --> S
```

三类客户端用同一套 wire 协议跟 server 对话：`connection/open` 上线、
`message stream` 听流、`message.send` 写消息、`run.open/append/close` 记录 agent
执行，`message.read` 拉历史。**Server 不区分对面是人还是 agent**。

### 2.2 节点职责

| 节点 | 职责 |
| --- | --- |
| `loom-server` | 持久化 + scope fanout + actor 注册 + journal 重放 |
| `loom <subcmd>`（人） | TUI / 一次性命令；每个进程持有人类 actor 的连接 |
| `loom agent serve`（agent client） | 读本地 agent 配置、按需 spawn agent runtime、为每个被管理的 actor 维护一条到 server 的连接、把 adapter 输出翻译成 message/run 记录 |

### 2.3 端到端 sequence（典型 wakeup）

```mermaid
sequenceDiagram
    actor Human as Human (loom chat)
    participant Server as loom-server
    participant Client as loom agent serve
    participant Adapter as AcpAdapter / CommandAdapter
    participant Agent as opencode / claude -p / ...

    Note over Client: 启动时已 connection/open<br/>+ message stream
    Human->>Server: message.send<br/>(message, directed_to=actor_X)
    Server-->>Client: message.created
    Client->>Adapter: dispatch(actor_X, message)
    Adapter->>Adapter: ensure_started(actor_X)
    Adapter->>Agent: send_prompt(text)
    Agent-->>Adapter: AdapterEvent::Text(partial)
    Adapter-->>Client: Text(partial)
    Client->>Server: run.append (text.delta)
    Agent-->>Adapter: AdapterEvent::Finished
    Adapter-->>Client: Finished
    Client->>Server: message.send (message)
    Client->>Server: run.close
    Server-->>Human: message.created
```

关键点：

- Server 既不感知 adapter 类型，也不感知 ACP/command 子进程是否存在。它只看到
  `actor_X` 上线了一条连接，写了一条消息。
- agent client 持有 `(actor_X) → adapter` 的内存映射，wakeup 完全在 client 进程
  内闭环。

---

## 3. 进程与二进制边界

### 3.1 一份产物，多个子命令

继续维持单 `loom` 二进制，子命令分发：

```
loom who                         # 当前 cli + 配置
loom chat --in <thread>          # 人类 TUI
loom message read --in <scope>     # 一次性命令
loom artifact publish --name ... # 一次性命令
loom agent serve                 # ★ 新增：agent client 常驻进程
loom agent install <id>          # （仍然存在，归属由 server 改成 client 本地）
loom agent list                  # 列出本地 agent client 管理的 agent
```

理由：

- agent client 与 human client 共享 `crates/cli/src/{client,config}.rs`——同一份
  连接管理、同一份 `~/.config/loom/config.toml` 解析。
- 部署只需要分发一个文件。
- 同一台机器上 `loom chat` 和 `loom agent serve` 可以共存，无端口冲突；它们各自向
  server 发起独立的 WebSocket 连接。

### 3.2 `loom agent serve` 的进程模型

```
loom agent serve
  ├─ tokio runtime
  ├─ AdapterRegistry
  │    ├─ actor_X ─ AcpAdapter ─ ACP child process
  │    ├─ actor_Y ─ CommandAdapter ─ (per-prompt: spawn `claude -p ...`)
  │    └─ ...
  ├─ ConnectionPool
  │    └─ for each managed actor: 1 WebSocket → loom-server
  └─ SignalHandler (SIGINT/SIGTERM → graceful shutdown 所有子进程)
```

每个被管理的 actor 在 server 上都有一条独立连接（`connection/open` 给出该 actor
id 与 `actor.kind = "agent"`）。这样 server 端 `subscribe::bind_actor` 的逻辑可
以直接复用，scope fanout 自动覆盖。

### 3.3 配置位置

| 内容 | v0 位置 | v1 位置（已实现） |
| --- | --- | --- |
| Agent spec（`*.json`） | `<server-data>/agents/` 由 server 扫描 | `~/.config/loom/agents/` 由 agent client 扫描；`--specs <dir>` 可覆盖 |
| Marketplace catalog | 无 server 归属 | 由 CLI 读取内置 catalog；`loom agent install` 直接写本地 spec |
| Actor 持久状态 | `<server-data>/agents/<id>/{profile,bundles,logs}` | `~/.agentx/agents/<id>/{profile,bundles}` |
| Agent workspace（`{agent.workspace}` 等模板变量） | `<server-data>/channels/<channel-id>/agents/<id>/{workspace,logs}` | `~/.agentx/channels/<channel-id>/agents/<id>/{workspace,logs}` |
| Command session 簿记 | （v0 没有 command transport） | `~/.agentx/sessions/<actor_id>/<scope_id>.json` |

> **迁移工具**：尚未提供专门的 `migrate-from-server` 命令；当前用法是手工
> `cp <server-data>/agents/*.json ~/.config/loom/agents/`，因为 spec 文件结构本身没变。
> phase E4 清理 server 时再考虑是否需要正式迁移工具。

---

## 4. Adapter 抽象

### 4.1 Trait 形态（草案）

```rust
// 所在 crate（建议）：crates/agent-client/src/adapter/mod.rs
#[async_trait]
pub trait Adapter: Send + Sync {
    /// 启动 agent 进程或建立连接。返回的 Sender 用于 send_prompt 的 sentinel；
    /// 真正的事件流通过传入的 mpsc::Sender 异步推回。
    async fn start(
        &self,
        ctx: AdapterContext,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AdapterStartInfo, AdapterError>;

    /// 把一段 prompt 文本送给 agent。command transport 下这一次调用就是一次完整
    /// 的子进程生命周期；ACP 下是往 long-lived 子进程发 session/prompt。
    async fn send_prompt(&self, scope: &ScopeRef, prompt: &str)
        -> Result<(), AdapterError>;

    /// 转发 action.response（ACP permission reply / command transport 下可能 no-op）。
    async fn respond_action(&self, request_id: &str, option_id: &str)
        -> Result<(), AdapterError>;

    /// 优雅停止。
    async fn stop(&self) -> Result<(), AdapterError>;
}

pub struct AdapterContext {
    pub actor_id: String,
    pub workspace: PathBuf,   // {agent.workspace}
    pub profile: PathBuf,     // {agent.profile}
    pub logs: PathBuf,        // {agent.logs}
    pub server_url: String,   // child-facing LOOM_SERVER, loopback-rewritten when possible
    /// agent 自己回头要 shell 出 loom 时，PATH 上带的 cli 目录
    pub cli_dir: Option<PathBuf>,
}
```

### 4.2 `AdapterEvent`：跨 transport 的统一事件

`AdapterEvent` 是跨 transport 的统一事件：

```rust
pub enum AdapterEvent {
    Text { content: String, is_partial: bool },
    ToolUse { tool_name: String, input: Value },
    ActionRequest {
        id: String,
        request_type: String,
        title: String,
        description: String,
        choices: Vec<ActionChoice>,
    },
    StatusChange { status: String },
    Finished { success: bool, summary: String },
    Error { message: String },
}
```

每个 adapter 实现自己内部到 `AdapterEvent` 的翻译；`AdapterRegistry` 收到
`AdapterEvent` 之后跑统一翻译层，把它们
变成 `message.send` / `run.append` / `run.close` RPC 调用发回 server。

不同 transport 的保真度差异通过填充 `AdapterEvent` 子集来表达：

| Transport | 通常会发出 |
| --- | --- |
| ACP | 全部七种（包括 ToolUse / StatusChange / partial Text） |
| Command (`text` 输出) | 仅 `Finished` 收尾时一次 `Text { is_partial: false }` |
| Command (`*_stream_json` 输出) | Text(partial) + ToolUse + Finished |
| MCP（未来） | Text + ToolUse + Finished |

### 4.3 类图

```mermaid
classDiagram
    class Adapter {
        <<trait>>
        +start(ctx, events) Result~AdapterStartInfo~
        +send_prompt(scope, prompt) Result
        +respond_action(req_id, option_id) Result
        +stop() Result
    }

    class AcpAdapter {
        -config: AcpConfig
        -inner: Mutex~AcpInner~
    }

    class CommandAdapter {
        -spec: CommandTransport
        -session_index: SessionIndex
    }

    class AdapterEvent {
        <<enum>>
        Text
        ToolUse
        ActionRequest
        StatusChange
        Finished
        Error
    }

    class AdapterRegistry {
        -adapters: Map~String, Box~Adapter~~
        -ctx_factory: Fn(actor_id) -> AdapterContext
        +ensure(actor_id) &Adapter
        +dispatch_event(actor_id, store_event)
    }

    Adapter <|.. AcpAdapter
    Adapter <|.. CommandAdapter
    AdapterRegistry o-- Adapter : owns
    Adapter ..> AdapterEvent : emits
```

为什么用 trait 而不是 enum：第三方也能 ship 自己的 adapter（比如某团队内部的
gRPC agent 服务），不需要修改 loom 的代码。

### 4.4 `transport.kind` 取值

[`AgentTransport.kind`](../crates/proto/src/methods.rs#L394-L406) 字段从
`"acp_stdio"` 单值扩展为：

| 值 | 对应 adapter | 说明 |
| --- | --- | --- |
| `"acp_stdio"` | `AcpAdapter` | 长驻 ACP 子进程；每个 dispatch 的 `session/new.cwd` 由 runtime 按 channel 计算 |
| `"command"` | `CommandAdapter` | v1 新增，详见 [docs/command-transport-v0.md](command-transport-v0.md) |

后续可继续追加（`"mcp_stdio"`、`"http_sse"` 等）。Agent client 启动时按
`transport.kind` 选择 adapter 实现并分发。

---

## 5. Agent Registry 归属

### 5.1 v0 的归属

拆分前 server boot 会扫描 `agents/`，每个 spec 写进内存 registry 并
`store.upsert_actor`。前端通过 `agent/*` RPC 访问这张表。

### 5.2 v1 的归属

Agent client **持有** registry。Server 不再维护 agent 列表。

```mermaid
flowchart TD
    A[~/.config/loom/agents/*.json] -->|boot scan| B[AdapterRegistry]
    B -->|"for each autostart=true"| C[connection/open<br/>actor.kind=agent]
    C --> D[server.actors 表]
    B -->|"on directed-message arrives"| E[adapter.start + send_prompt]
```

- Agent client 启动 → 扫 `~/.config/loom/agents/*.json` → 对每个 `autostart: true`
  的 spec 立刻 `connection/open(actor_id, kind=agent)`，把这个 actor 在 server 上
  上线。Server 端
  [`subscriptions.bind_actor`](../crates/server/src/handlers/mod.rs#L98-L135)
  自动建立 actor↔connection 映射。
- Server 端 `agent/*` runtime RPC 已删除。前端要列 agent，调 `actor/list` 过滤
  `kind == "agent"`；agent 配置管理走 CLI 本地 spec 文件。

### 5.3 Agent client 本地管理面

`loom agent serve` 进程之外，仍然要让用户能 `loom agent install foo` /
`loom agent stop bar`。两条选择：

- **a)** agent client 在 `~/.agentx/loom-agent.sock` 起 unix socket，
  `loom agent <op>` 命令通过它管理本地 registry。
- **b)** 不起 socket，`loom agent install` 直接写 `~/.config/loom/agents/foo.json`，
  agent client 监听文件目录变更（notify crate）做 hot reload。

倾向 **b)**：少一条进程间协议，部署面最小。`start`/`stop`/`log` 这种"对运行中
进程的实时操作"留到第二迭代再加 socket。

### 5.4 Marketplace

`assets/marketplace.json` 编目格式不变（6 个 ACP agent + 后续可加 command 类的
agent），但读取者从 server 改成 cli crate。`loom agent install foo --prefer command`
可以让用户显式装 command 版本（比如 `claude` 而不是 `claude-acp`）。

---

## 6. Wakeup 流（v1 详细）

### 6.1 启动阶段

```mermaid
sequenceDiagram
    participant Client as loom agent serve
    participant Server as loom-server

    Client->>Client: 扫 ~/.config/loom/agents/*.json
    loop for each spec where autostart=true
        Client->>Server: WS connect
        Client->>Server: connection/open<br/>(actor_id, kind=agent)
        Server-->>Client: connection.id
        Client->>Server: message stream<br/>(scope = actor's known scopes)
    end
```

注：`autostart=false` 的 agent 不在启动时连 server——只有当外部调用
`loom agent start <id>` 或 hot reload 触发时才上线。

### 6.2 收到 directed-message

```mermaid
sequenceDiagram
    participant Server as loom-server
    participant Client as loom agent serve
    participant Reg as AdapterRegistry
    participant A as Adapter (ACP or Command)

    Server-->>Client: message.created<br/>(message with audience=actor_X)
    Client->>Reg: dispatch_message(actor_X, message)
    Reg->>Reg: filter: audience contains actor_X<br/>and author != actor_X
    Reg->>A: ensure_started(ctx)
    A-->>Reg: started
    Reg->>Server: run.open(actor_X, scope, trigger=message.id)
    Server-->>Reg: run.id
    Reg->>A: send_prompt(scope, render_prompt(message))
    A-->>Reg: AdapterEvent stream
    loop translate_event
        Reg->>Server: run.append | message.send | run.close
    end
```

判断哪些消息触发哪个 actor，只看 message audience / delivery rows 是否指向该
client 管理的 actor 之一。

### 6.3 与 v0 的关键差别

| 步骤 | v0 | v1 |
| --- | --- | --- |
| 监听 store | `store.subscribe()`（进程内 broadcast） | `message stream`（跨进程 RPC） |
| 起 run | `store.open_run(...)`（直接调 store） | `run.open` RPC |
| 写 trace | `store.append_run_frame(...)` | `run.append` RPC |
| 写 message | `store.append_event(...)` | `message.send` RPC |
| 关闭 run | `store.close_run(...)` | `run.close` RPC |

注意 server 端这些 RPC 在 v0 已经存在（GUI 也用同一组），所以**协议侧零改动**。

---

## 7. 迁移路径

四个 phase，每个都可以独立验证、独立合并。

### Phase E1：抽 trait（同进程内重构） ✅ 已合

提交：`refactor: extract trait Adapter`。

- ~~定义 `trait Adapter` 与 `AdapterEvent`。~~
- ~~把现有 `AcpAdapter` 套上 trait。~~
- ~~server 侧 runtime 改成 `Arc<dyn Adapter>`。~~

实际落点：trait 与 `AdapterEvent` 现在在 `crates/agent-runtime` crate 中，
`AcpAdapter` 实现了它；server 不再依赖这个 crate。

### Phase E2：实现 `CommandAdapter` ✅ 已合

提交：`feat(server): add CommandAdapter`。

- `AgentTransport::kind` 增加 `"command"` 分支；
- 新增 [`CommandAdapter`](../crates/agent-runtime/src/adapter/command.rs)，
  实现 spec 见 [docs/command-transport-v0.md](command-transport-v0.md)。
- 端到端跑通"人发 directed-message → adapter spawn 子进程 → 输出 → 写回 message"。

### Phase E3：搬出去 ✅ 已合（分三步）

- **E3a**（`refactor: extract agent-runtime crate`）：把
  adapter/acp/command 提取到独立 crate `crates/agent-runtime/`。
- **E3b**：外部 client 通过 `connection/open(actor_id, kind=agent)` 接管 actor
  上线。
- **E3c**（`feat(cli): loom agent serve external runtime client`）：新增
  [`crates/cli/src/cmd/agent_serve.rs`](../crates/cli/src/cmd/agent_serve.rs)
  实现 `loom agent serve [--specs <dir>]`：扫 `~/.config/loom/agents/` 的
  per-actor AgentSpec；每个 actor 起一条 WS、用 `connection/open(actor_id, kind=agent)` 上线，监听通知、把
  message delivery 翻译成 `run.open` + `send_prompt` + 流式 run frame + `run.close`。

### Phase E4：清理 server ✅ 已合

- 删除 `crates/server/src/runtime/`，server 不再依赖 `agent-runtime`。
- 删除 server 侧 `agent/*` runtime RPC；AgentSpec / ProviderManifest 的安装、注册、删除
  改为 CLI/daemon 本地写 `{LOOM_CONFIG_DIR}/agents/` 与 `{LOOM_CONFIG_DIR}/providers/`。
- 后续可把 `AgentSpec` / `AgentTransport` 从 `methods.rs` 移到独立 mod，进一步
  表明它们不属于 server runtime 协议。

每个 phase 都满足"可灰度"：E1/E2 没有协议变更；E3 让 server 同时能跑两种部署模
式；E4 才是 breaking change。

---

## 8. 不动的部分

明确这些**不**在本设计的改动范围内，避免 review 时产生混淆：

- **Wire 协议**（`connection/open` / `message stream` / `message.send` /
  `run.open` / `run.close` / `run.append` / `message.read` /
  `artifact/*` / `delivery.ack` / `action/respond`）：字段、行为、语义都不变。
- **Trace 帧形状**：`run.append` 的 `kind`（text.delta / tool.start /
  tool.update / tool.end / status / error）与 payload 不变。变的只是发送方从
  server 内部变成 agent client。
- **Journal 格式**：server 端 [`journal.rs`](../crates/server/src/journal.rs) 不
  受影响。Agent client 不需要自己持久化事件流——它只是一个上游 producer。
- **Store 模型**：`Channel` / `Thread` / `Run` / `Message` / `Audience` /
  `Artifact` / `ActorPresence` / `Membership` / `Delivery` 表全部不动。

---

## 9. 开放问题

留给实现期决定，不在本文档强制：

1. **Agent client 与 server 的认证**。v0 默认同机无认证。v1 即便仍然同机，agent
   client 在 `connection/open` 时用什么凭证证明它有权代表 `actor_X`？
   - 候选：本地共享 secret（写在 `~/.config/loom/auth.toml`）；或 server 启动时
     生成 token 写到 `<data>/agent-client.token`，agent client 启动时读。
2. **多实例**。允许同一台 server + 多个 agent client 进程吗？同一个 actor id 同
   时被两个 client 持有时如何决定 routing？
   - 倾向：禁止；server 端 `subscriptions.bind_actor` 要求每个 actor 一个 owner，
     第二次 `connection/open` 抢占或拒绝。
3. **Hot reload**。`~/.config/loom/agents/` 下加新 spec 时是否自动起 connection？
   倾向：用 notify crate 做文件监听；新增/更新立即生效；删除等当前 in-flight
   prompt 完成后停。
4. **失败重连**。Agent client 与 server 的 WS 断开时的重连策略；ACP 子进程异常
   退出时是否自动重启。倾向：指数退避重连；ACP 子进程崩溃只在下一次 directed-message
   到来时重启（lazy）。
5. **多 server**。agent client 配置里允许多个 server URL，让同一台机器上的 agent
   同时驻留多个 loom 实例？v1 不优先支持，但 spec 字段留空间（spec 顶层加
   `target_server` 可选字段）。

---

## 10. 引用代码索引

| 概念 | 现有位置 | 在 v1 中的对应 |
| --- | --- | --- |
| `AdapterEvent` 七变体 | [crates/agent-runtime/src/adapter.rs](../crates/agent-runtime/src/adapter.rs) | 跨 transport 共用 |
| Agent 状态机 | [crates/cli/src/cmd/agent_serve.rs](../crates/cli/src/cmd/agent_serve.rs) | client 侧管理 adapter、turn 和 queue |
| `AcpAdapter` 实现 | [crates/agent-runtime/src/acp.rs](../crates/agent-runtime/src/acp.rs) | ACP stdio transport |
| `CommandAdapter` 实现 | [crates/agent-runtime/src/command.rs](../crates/agent-runtime/src/command.rs) | 一次性 CLI transport |
| `action.response` 路由 | [crates/cli/src/cmd/agent_serve.rs](../crates/cli/src/cmd/agent_serve.rs) | agent client 调 `adapter.respond_action` |
| `AgentSpec` / `AgentTransport` schema | [crates/proto/src/methods.rs](../crates/proto/src/methods.rs) | AgentSpec 只引用 providerRef；AgentTransport 是 provider resolve 后的 runtime plan |
| 现有 spec 范例 | [agents/opencode.json](../agents/opencode.json) | E3 阶段迁到 `~/.config/loom/agents/` |
| Marketplace 编目 | [assets/marketplace.json](../assets/marketplace.json) | 编目格式不变；`loom agent install` 改由 cli 写本地文件 |
| `connection/open` handler | [crates/server/src/handlers/mod.rs:98-135](../crates/server/src/handlers/mod.rs#L98-L135) | 不变；agent client 用同一接口上线每个被管理的 actor |

---

## 11. Review checklist

文档评审重点（写完之后给到 review 人对照看）：

- [ ] §2 拓扑图与 §3 进程边界是否一致？是否漏掉了任何"server 必须知道 agent
      细节"的隐性依赖？
- [ ] §4 trait 形态是否真的够覆盖 ACP 与 command 两类 transport？
- [ ] §6 wakeup 流相比 v0 的 `wakeup.rs` 是否丢了任何 corner case（特别是
      `take_seed_slot` / `set_active_turn` / `take_text_buffer` 这些跨调用状态）？
- [ ] §7 phase 切分是否真的可以独立合并、独立验证？
- [ ] §9 开放问题是否都标注了"倾向"或"留到 v2"，没有遗留"待讨论"？
