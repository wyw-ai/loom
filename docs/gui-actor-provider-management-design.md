# GUI Actor / Provider 管理设计

> 状态：Design proposal
>
> 目标读者：Loom GUI、daemon、agent-runtime、server 维护者。
>
> 本文回答一个具体问题：actor 和 provider 最终应该怎样在 GUI 中维护，才能符合 Loom 的项目理念，并且能落到当前 Rust / Tauri / React 实现上。

## 1. 结论

Loom 的核心理念不是“在 GUI 里配置一堆机器人”，而是：

- `server` 保存协作事实：actor、channel、message、task、delivery、run、artifact、machine command。
- `daemon` 代表一台真实机器：它发现本机 provider，保存本机 AgentSpec / ProviderManifest，启动 agent worker，并把 inventory 发布给 server。
- `GUI` 是 human actor 的工作台：它展示协作事实和 daemon inventory，发起维护意图，但不直接拥有 runtime 配置文件。

因此 actor / provider 的 GUI 管理应落在 `Actors` 工作区里。它不是一个单纯的 actor
directory，也不是一个 host / provider 配置后台，而是 Loom workspace 级的参与者管理面：
谁可以参与协作，哪些 agent / service actor 由哪些 registered hosts 承载，以及这些运行
依赖是否健康。

1. GUI 从 server 读取 daemon 发布的 machine inventory。
2. GUI 只通过 `machine/command` 向目标 daemon 提交 mutating 操作。
3. daemon 校验、写盘、刷新 inventory、重启或停止 worker。
4. server 只记录 command、转发给 daemon、保存结果，不解释 ProviderManifest，也不直接生成 AgentSpec。
5. GUI 把 actor 维护分成两层：
   - 协作身份层：Actor directory、channel membership、mention / delivery。
   - 运行配置层：AgentSpec、ProviderManifest、profile、memory、bundle、runtime health。

当前 GUI 已经走对了大方向：它不再直接维护本地 machine 配置，已经通过 daemon inventory 展示 hosts / agents，并通过 machine command 创建、更新、删除 agent。但它只覆盖了很窄的 AgentSpec 子集，provider 也只有只读 badge，没有可维护的 manifest / doctor / add / remove / mode / prompt / decoder 管理面。

## 2. 依据与项目理念

本节只总结当前仓库中已经形成共识的边界。

### 2.1 协作域和接入域分离

协议文档把 `Actor`、`Channel`、`Thread`、`Event/Message`、`Delivery` 放在协作域，把 connection / endpoint 放在接入域。核心含义是：Loom 关心“协作中发生了什么”，不把某个 WebSocket 连接或某个本地进程当成业务事实。

这影响 GUI 设计：

- GUI 不能把“某个 agent worker 在线”误当成“这个 actor 存在”的唯一依据。
- 删除 agent runtime 不应该破坏历史消息里的 actor 身份。
- channel membership 是协作权限和可见性问题，不能混在 provider runtime 配置里。

### 2.2 actor 平等，但运行职责不平等

协议层中 human、agent、service 都是 Actor。它们在 message / task / delivery 中是平等参与者。

但进程职责不是平等的：

- human client 只负责交互。
- agent worker 负责消费 directed delivery 并执行 run。
- service actor 可以发布 daemon inventory 或执行 machine command。

所以 GUI 的 actor 页面需要同时表达两件事：

- 这个 actor 作为协作者是谁。
- 如果它是 agent，它由哪台 daemon host、哪个 ProviderManifest、哪个 AgentSpec 驱动。

### 2.3 server 是纯消息枢纽

当前架构文档明确规定 `loom-server` 不读取 agent machine 配置，不安装 agent，不 spawn agent 子进程，也不链接 agent-runtime。任何 provider command、parser、session、workspace、profile、memory 都不应该进入 server。

这影响 GUI 设计：

- server 可以保存 machine command 和结果，方便审计和重试。
- server 可以保存 daemon inventory 快照，方便 GUI 查询。
- server 不应该验证 ProviderManifest 的 provider-specific 语义。
- server 不应该把 GUI 表单直接翻译成完整 runtime transport。

### 2.4 daemon 是 machine-scoped runtime supervisor

当前 daemon 负责：

- 读取 `LOOM_CONFIG_DIR/daemon.toml` 中的 machine 身份。
- 从 `LOOM_CONFIG_DIR/providers/<id>.json` 加载本机 provider manifest。
- 从 `LOOM_CONFIG_DIR/agents/<actor_id>/spec.json` 加载 AgentSpec。
- 探测 PATH 上的 provider CLI。
- 发布 machine inventory v2 到 server 的 service actor `_meta`。
- 处理 `agent.create`、`agent.update`、`agent.remove`、`provider.add`、`provider.remove` machine command。
- 根据 AgentSpec 解析 ProviderManifest，启动 worker，构造 scope-aware workspace/env/prompt。

所以 GUI 的维护操作必须以 daemon 为写入方。即使 GUI 和 daemon 在同一台机器，也不应该有“GUI 直接写文件”的本机捷径。

### 2.5 provider 是数据驱动 runtime recipe

当前 ProviderManifest 已经是数据驱动模型：

- `detect.candidates` 描述如何发现 CLI。
- `modes` 描述 transport、command、args、env、stdin、stdout/stderr decoder、session、interactive 配置。
- `models` 描述模型菜单和默认值。
- `extends` 支持 daemon-local provider variant。
- prompt delivery interface 描述这个产品接收 prompt 的形态，例如 full prompt、system/user
  split、stdin、argv、env 或 interactive template。

AgentSpec 只保存具体 agent：

- actor id / kind / displayName / metadata。
- `instructions`。
- `providerRef`，包含 provider id、mode、model、reasoningEffort。
- `autostart`。
- 可选 `models`、bundle、memory、announcement、trigger、Prompt Assembly。

这影响 GUI 设计：

- Provider Library 应维护 ProviderManifest，不应把 provider command/args 散落在 agent 表单里。
- agent detail 应维护 AgentSpec，不应复制 provider 的 runtime plan。
- provider manifest 不应管理 agent prompt 内容、profile prompt 文件或 workspace prompt 文件。
  Provider 只说明“这个产品怎么接收 Loom 组装好的 prompt”。
- Agent detail 才维护 system prompt / user prompt 怎么组装、引用哪些 profile / workspace
  文件、哪些 memory 和 runtime context 会进入 prompt。

需要固定一个硬边界：`LOOM_CONFIG_DIR/providers/<provider_id>.json` 只保存
ProviderManifest，绝不保存 actor 或 agent 列表。ProviderManifest 描述“某类外部
agent CLI 怎么接入 Loom”；AgentSpec 和 agent profile/workspace 才描述“某个具体 agent
actor 是谁、引用哪个 provider，以及它怎样组装 prompt”。

## 3. 当前实现盘点

### 3.1 agent-runtime / provider

当前代码中：

- `ProviderRegistry` 加载内置 provider，再加载 daemon-local provider manifest。
- provider detection 只读 PATH 和 manifest，不做写操作。
- `ProviderRuntimePlan` 是 resolved manifest + selected providerRef 后的执行计划，再转成 `AgentTransport`。
- decoder 会把 provider stdout/stderr 映射为 runtime event，之后再由 agent worker 映射为 AdapterEvent / message / run trace。
- `AdapterPrompt` 已经包含结构化 prompt parts、outputs、model、cwd、env、template vars。

这说明 provider 管理的正确抽象已经存在。GUI 缺的不是底层模型，而是维护面和诊断面。

### 3.2 daemon

daemon 的事实源如下：

| 数据 | 当前位置 | GUI 是否应直接写 |
| --- | --- | --- |
| daemon machine identity | `LOOM_CONFIG_DIR/daemon.toml` | 否 |
| AgentSpec | `LOOM_CONFIG_DIR/agents/<actor_id>/spec.json` | 否 |
| local ProviderManifest | `LOOM_CONFIG_DIR/providers/<id>.json` | 否 |
| profile / memory | `LOOM_AGENT_DATA_ROOT/agents/<actor_id>/profile` | 否 |
| channel workspace | `LOOM_AGENT_DATA_ROOT/channels/<channel_id>/agents/<actor_id>/workspace` | 否 |
| bundle install | `LOOM_AGENT_DATA_ROOT/agents/<actor_id>/bundles` | 否 |

这几个文件不能合并：

- `providers/<provider_id>.json`：ProviderManifest。保存 provider recipe，例如 detect、modes、
  command/args/env/stdin、prompt delivery interface、decoder、session、models。
- `agents/<actor_id>/spec.json`：AgentSpec。保存具体 agent actor 的 identity、
  instructions、providerRef、model/reasoning、memory、bundle、trigger、Prompt Assembly。
- server `Actor` directory：保存协作身份，供 message、channel membership、mention、
  delivery、history 引用。
- channel membership：保存某个 actor 是否属于某个 channel；不写进 ProviderManifest，也不写进
  AgentSpec。

daemon 发布的 machine inventory v2 包含：

- machine id / name / kind / dataRoot / configDir。
- `providers`：detected provider summary。
- `agentSpecs`：当前 daemon 加载的 AgentSpec。
- capabilities：当前包含 `inventory.read`、`connection.status`、`machine.command`、`agent.create`、`agent.remove`、`provider.add`、`provider.remove`。
- revision / observedAt。

daemon machine command 当前支持：

- `agent.create`
- `agent.update`
- `agent.remove`
- `provider.add`
- `provider.remove`

需要注意一个实现不一致：inventory capabilities 暴露了 `agent.create` / `agent.remove`，但没有单独暴露 `agent.update`；GUI 通过 `machine.command` 仍可执行 `agent.update`。后续设计应补齐 operation-level capability，避免前端只能靠 `machine.command` 推断。

### 3.3 server

server 的 machine command 链路已经具备落地基础：

1. GUI / CLI 调 `machine/command` 或 `machine/command.create`。
2. server 校验 machine actor 是否是 daemon inventory v2 service actor。
3. mutating operation 必须带 `ifInventoryRevision`。
4. server 把 command 持久化为 `MachineCommand`。
5. server 向 machine actor 推送 `machine/command.notify`。
6. daemon ack、执行、提交 `machine/command.result`。
7. server 保存结果。

当前短板是 `MachineCommandUpdated` 没有通过普通 scope stream 推给 GUI，GUI 现在主要使用旧的同步 `machine/command` 包装等待 30 秒。长期管理面需要 command history 和异步状态流。

### 3.4 GUI 后端

Tauri IPC 当前做了这些事：

- `machine_list` / `machine_check`：从 `actor/list` 找 daemon inventory service actor，组装 `MachineInfo`。
- `machine_agent_create`：构造 `agent.create` command。
- `agent_update`：构造 `agent.update` command。
- `machine_agent_remove`：构造 `agent.remove` command。
- `actor_list`：读取 server actor directory，并按当前 owner 的 daemon inventory 过滤 agent。

`MachineInfo.providers` 只暴露 provider summary：

- id / name / transportKind / command / args。
- actorCount。
- defaultModel / modelChoices。

它没有暴露：

- provider source：builtin / local / extends。
- detected health details。
- modes 列表和 default mode。
- prompt delivery interface。
- decoder / session / interactive 细节。
- provider doctor 结果。
- raw manifest 或安全 redacted manifest。

### 3.5 GUI 前端

React 当前 Hosts 页面已经支持：

- host 列表、连接状态、dataRoot/configDir/serveCommand/inventory revision。
- provider badge，只读展示 detected runtimes。
- agent 创建：provider、name、actor id、model、autostart、instructions。
- agent 编辑：displayName、avatar、instructions、provider、model、reasoningEffort、autostart。
- agent 删除。
- channel members 面板可邀请 agent / human，展示在线状态。
- message 中 agent avatar 可以跳到 agent settings。

但 provider 管理没有对应页面；agent 高级能力也没有表单或 JSON 编辑面。

### 3.6 AgentConfigVersion 的定位

server 协议里已经有 `agent_config.publish` / `agent_config.activate` 和
`AgentConfigVersion`。当前 agent worker 启动时会把 daemon-local AgentSpec 序列化进
`tools`，按 spec hash 生成 runtime version，并激活该 version。

这份 version 是运行时审计快照，不是 GUI 当前应直接编辑的事实源：

- AgentSpec / ProviderManifest 的写入仍属于 daemon。
- AgentConfigVersion 可用于 run 追溯、健康诊断、spec hash 对比和历史审计。
- GUI 可以展示 active AgentConfigVersion，但不应绕过 daemon 直接 publish 一个新
  version 来改变本机 runtime。

未来如果项目决定把 `AgentConfigVersion` 升级为 server-owned 不可变配置发布模型，需要先更新 provider/daemon 所有权设计；在当前实现下，它只是 daemon 执行后发布到 server 的快照。

## 4. GUI 已支持与未支持

### 4.1 已支持

| 领域 | 当前 GUI 支持 |
| --- | --- |
| host inventory | 展示 daemon 发布的 hosts、providers、agents、revision、observedAt |
| host connection | 展示 machine service actor 和 agent connection 是否在线 |
| provider visibility | 展示 detected provider badge、默认模型和模型菜单 |
| agent create | 选择 host/provider/model，填写 actor id/name/instructions/autostart |
| agent update | 修改 displayName、avatar、instructions、provider、model、reasoningEffort、autostart |
| agent remove | 删除 daemon-local AgentSpec |
| channel membership | 把 agent 作为 channel member 添加/移除 |
| actor filtering | GUI actor list 只展示当前 owner daemon inventory 中的 active agents |

### 4.2 未支持

| 领域 | 缺口 | 影响 |
| --- | --- | --- |
| provider add/remove | GUI 已有 Add ProviderManifest 的首版入口；remove/doctor/detail 仍需补齐 | 可以从 GUI 添加自定义 provider，但删除 local variant 和排障链路还不完整 |
| provider detail | 没有 mode、decoder、session、source、extends、doctor、prompt delivery interface | 使用者不知道 provider 为什么不可用，也无法审查 runtime 行为 |
| provider validation | 没有 manifest validate / doctor / command preview | 新 provider 只能靠 CLI 排障 |
| provider mode | Agent create/update 固定或隐式偏向 `print`，没有 mode 选择 | Claude `nonprint` 等模式无法通过 GUI 维护 |
| AgentSpec 高级字段 | memory、bundle、announcement、trigger、promptTemplate、models choices、capabilities 不可编辑 | GUI 创建的 agent 只是简单 agent，不能维护真实生产 actor |
| prompt ownership | 首版已迁移到 AgentSpec.promptAssembly；仍需补更完整的 builder / validation UI | provider 接入定义和 agent 个性化 prompt 已分层，细粒度编辑体验还需要打磨 |
| prompt file management | Agent detail 已支持受控 profile prompt files；scope workspace 文件 UI 仍需补齐 | 用户可以编辑长期 prompt 文件，但项目/线程级 prompt 文件管理还不完整 |
| instructions vs description | 首版已分开 description 和 instructions | 仍需在高级页补更清晰的 identity / metadata 审计视图 |
| autostart 语义 | GUI 可保存 autostart/manual，但当前 daemon reconcile 会启动所有加载的 AgentSpec | UI 状态和实际运行行为可能不一致 |
| profile/memory | 只展示 profilePath，没有 memory records、MCP 开关、per-channel 策略 | 无法从 GUI 管长期上下文和隐私边界 |
| bundle/tools | 无 bundle source/version/install mode 维护 | 无法管理技能包和工具包 |
| runtime health | 没有 spec resolve、provider detect、model allowed、last run、config version 诊断汇总 | agent 不工作时排障链路太长 |
| command history | 没有 machine command history、状态、重试、失败原因结构化展示 | 修改失败只能看一条 toast |
| async command | 主要使用同步 `machine/command` 等结果 | 长操作、离线 daemon、重试都不好表达 |
| operation capability | inventory capability 没有细到 `agent.update` / `provider.edit` / `provider.doctor` | 前端权限判断粗糙 |
| stale actor cleanup | 删除 AgentSpec 后历史 actor 应保留，但 channel membership / active directory 需要明确策略 | 容易出现 “channel 中有不可维护 agent” 的困惑 |
| service actor | GUI 标了 Coming Soon | actor 维护没有覆盖 service |

## 5. 目标信息架构

建议把当前 Settings 里的 Hosts 页面升级为 `Actors` 工作区。这个名字应该承接 Loom 的
核心抽象：workspace 中参与协作的是 actor，human / agent / service 在协议层是平等身份。

但 `Actors` 不能做成一个单薄的 actor directory。对 agent / service 来说，用户真正要维护
的是“这个参与者如何运行”：注册在哪台 host、依赖哪个 provider 或 service recipe、现在是否
健康、能否被邀请到 channel。host 是运行事实源，provider 是 agent 的运行依赖，它们都应该
放在 `Actors` 工作区里，但以用户任务组织，而不是按后端对象一股脑罗列。

```text
Actors
  Registered Hosts
    Register host
    Host detail
    Host-scoped agents / providers / commands
  Agents
    Agent roster
    Create agent
    Agent detail
    Provider library
  Services
    Service roster
    Create service
    Service detail
```

不建议在二级菜单中放 `Providers` 或 `Commands`：

- provider 不是 actor，它是 agent runtime recipe。用户需要创建或排障 agent 时才进入
  Provider Library。
- command history 不是独立业务对象。它应该出现在 host detail、agent detail、provider
  detail 的上下文里，让用户看到“这次操作影响了谁，为什么失败”。

第一版也不建议放一个泛化的 `Directory` 二级菜单。workspace 里的 human actor、channel
membership、mention picker 各自有更自然的协作上下文；`Actors` 工作区优先解决需要运行
承载的 actor：agent 和 service。如果未来需要 workspace 级全量 actor directory，应作为查找、
筛选和跳转入口，而不是把 host / provider / AgentSpec / channel membership 混成一个总表。

### 5.1 Registered Hosts

Registered Hosts 的核心问题是：哪些 daemon host 已经接入当前 workspace，它们是否可以
承载新的 agent / service。

默认列表不应该像机器配置表。每个 host 行或卡片重点展示：

- display name / machine id 短别名。
- online / offline / stale 状态和 last seen。
- readiness：可创建 agent、provider missing、capability missing、command pending。
- agent count / service count。
- provider readiness summary：例如 Claude detected、Codex missing、local variants count。
- 最近失败或正在执行的 command 摘要。

次要信息如 configDir、dataRoot、serveCommand、raw capabilities、inventory revision 放到
detail 或 advanced 区域，避免默认页面被路径和协议字段淹没。

注册 host 的流程应该像“把一台设备接入 workspace”：

1. 选择 local 或 remote host。
2. GUI 生成或展示 daemon 启动 / 配对命令。
3. daemon 上线并发布 inventory 后，GUI 显示待确认 host。
4. 用户命名 host，并确认归属 workspace / owner actor。
5. 进入 host detail，看到 readiness、detected providers 和下一步动作。

Host detail 的产品逻辑：

- Overview：这个 host 是否 ready，下一步建议是 add agent、add provider 还是修复 daemon。
- Agents：只列这个 host 承载的 agent runtime，可从这里创建 agent 并预选该 host。
- Providers：这个 host 的 Provider Library 摘要和 doctor 入口。
- Commands：最近 machine commands，按影响对象和失败原因展示。
- Advanced：paths、capabilities、inventory revision、raw daemon runtime inventory。

Host 页面不提供“删除 host”作为默认动作。daemon-owned host 不能由 GUI 删除。GUI 只能提示如何停止 daemon 或如何移除 daemon 配置。除非未来 server 支持显式 unregister machine，否则不应显示 destructive host delete。

### 5.2 Agents

Agents 的核心问题是：workspace 里有哪些 agent actor 可以参与协作，它们分别由哪台
registered host 运行，现在是否可用。

Agent roster 不应默认按 host 分成多个大区块。默认应是跨 host 的 agent 列表，方便用户先找
“我要用的 actor”。host、provider、model、health 是行内标签和筛选条件：

- avatar / display name / actor id 短别名。
- status：online、offline、manual、starting、failed、host offline。
- host tag。
- provider / model tag。
- channel count 或最近参与 channel。
- last run / last error 摘要。
- row actions：open detail、invite to channel、disable / enable、remove runtime。

从 host detail 进入 Agents 时，可以带 host filter；从 channel invite 进入时，可以带 channel
context。默认信息架构不要强迫用户先选 host。

Create Agent 应该是轻量向导，不要把 AgentSpec 的完整字段全部摊开。普通创建只解决四件事：

1. 这个 agent actor 是谁：display name、avatar、description，actor id 自动生成并允许高级修改。
2. 它注册到哪台 host：选择 Registered Host；如果没有 ready host，先引导注册 host。
3. 它用什么运行：选择该 host 上可用 provider 和 model；mode 用 provider default，放进 advanced。
4. 它如何参与：instructions、autostart、可选 invite 到 channel。

Create Agent 不默认展示 memory、bundle、Prompt Assembly、trigger、decoder、raw manifest、
raw spec。需要模板时可以提供 role preset，但 preset 生成的内容仍落到 AgentSpec 字段。

Edit Agent 则应该比创建详细，因为编辑发生在用户已经关心这个 actor 的运行行为时。Agent
detail 分 tabs：

- Overview：身份、host、provider/model、health、channel membership、最近错误和下一步建议。
- Identity：displayName、avatar、metadata description、actor id 只读或高级编辑。
- Instructions：`instructions`，与 UI description 分离。
- Run Settings：host、providerRef.id、mode、model、reasoningEffort、autostart；迁移 host
  或 provider 时展示 diff 和影响。
- Prompt Studio：system prompt / user prompt 组装规则、profile prompt files、scope workspace
  prompt files、rendered system/user/full preview、变量和缺失文件检查。
- Profile / Memory：profile overview、memory records、provider settings、MCP config、
  store/query/delivery/extraction/compaction、per-channel 状态。
- Bundle / Tools：bundle source/version/install mode/current path、skill body preview、MCP servers。
- Channels：当前 actor 是哪些 channel 的 member、可一键 invite/revoke。
- Diagnostics：machine online、agent connection、last run、last error、active AgentConfigVersion、
  spec hash、相关 machine commands。
- Raw Spec：redacted JSON editor，只给高级用户，必须 validate/diff 后 apply。

Provider Library 放在 Agents 页面内部，因为 provider 是创建和运行 agent 的依赖。默认视图按
host 聚合 provider readiness，而不是先展示 manifest taxonomy：

- host 是否有可用 provider。
- provider source：builtin、local、local extends builtin。
- detection：detected / missing / invalid。
- default model / default mode。
- used by agents count。
- primary actions：doctor、add local provider、duplicate as local variant、replace local provider。

Provider detail 才展开高级信息：modes、prompt delivery interface、argv/env/stdin binding、
decoder、session、detect candidates、redacted raw manifest。添加或替换 provider 必须先
validate，再 doctor；replace local provider 必须展示 impacted agents。

Provider detail 不展示 agent prompt parts，也不读取 agent profile/workspace 文件。它只回答：

- 这个产品如何被发现和启动。
- 支持哪些模式、模型、reasoning 参数。
- 接收 prompt 的接口是什么：full prompt、system/user split、stdin、argv、env 或 interactive
  template。
- stdout/stderr 如何被解码成 Loom runtime event。

Agent profile 内容应该能在 agent detail 中查看和编辑，但要做成受控的 Profile / Memory
编辑器，而不是任意文件浏览器。`{agent.profile}` 是 daemon host 上的 per-actor 持久状态
目录，可能包含 memory records、provider-specific settings、MCP 配置、本地模型选择等内容。
GUI 可以展示和修改这些内容，但只能通过 daemon machine command，由 daemon 做路径白名单、
redaction、schema 校验和写盘。

Profile / Memory 默认展示：

- Overview：profile path、profile scaffold 状态、memory store 类型、record count、最近更新。
- Memory records：按 accepted / pending / rejected / archived、type、tag、channel 过滤；
  支持 append、edit summary/detail/status/tags/confidence、archive，不默认硬删除。
- Provider settings：例如 `{agent.profile}/claude/settings.json` 这类 provider 声明过的
  actor_profile settings；只展示白名单文件，并按 provider 规则 redaction。
- MCP config：agent-local MCP 开关和配置摘要。
- Raw advanced：只读或白名单路径编辑，限制文件大小，禁止跳出 `{agent.profile}`。

这些内容不放到 Create Agent。新建 agent 只创建必要 scaffold；profile 内容在 Edit Agent
里维护，因为它属于长期上下文和运行调试。

Prompt Studio 的用户交互应该是：

1. 选择一个预览 scope，例如当前 channel 或最近一次 run。
2. 左侧是 prompt sources：
   - Instructions：AgentSpec.instructions。
   - Memory：bootstrap / turn memory 开关和过滤条件。
   - Profile prompt files：`{agent.profile}/prompts/*.md`，跨 channel 持久生效。
   - Workspace prompt files：当前 scope workspace 下 `.loom/*.md`，只影响该 channel/thread。
   - Runtime context：members、recent conversation、assignment/task context、latest message。
3. 中间是 System Prompt 和 User Prompt 的组装器。默认用 Loom preset；高级用户可调整顺序、
   include/exclude、template 和 join。
4. 右侧是 preview：`system`、`user`、`full` 三个输出，以及每个 part 的来源、大小、是否为空、
   是否缺失。
5. 保存时写 AgentSpec 的 Prompt Assembly 和受控 profile/workspace files；daemon 校验后生效。

Create Agent 不进入 Prompt Studio。创建时最多选择 role preset；preset 只生成
instructions、默认 Prompt Assembly 和可选 profile prompt file 模板。用户需要调细节时再进
Edit Agent。

### 5.3 Services

Services 的核心问题是：除了 LLM agent 外，workspace 里还有哪些 service actor 可以参与协作
或执行系统职责。

Services 先按产品能力预留，不要为了填页面而展示空技术表。当前可以支持空态和未来扩展：

- service roster：service actor、service type、host-backed 或 server-backed、status、channels。
- create service：选择 service type，再选择 Registered Host（如果该 service 需要 daemon
  承载），填写身份和最小配置。
- service detail：Overview、Config、Run Settings、Channels、Diagnostics。

新增 service 时和新增 agent 一样：先把它当作 actor 创建，再选择它的运行承载。channel invite
仍然是协作层操作，不写进 ServiceSpec。

### 5.4 Channel 成员面板

Channel Members 面板解决的核心问题不是 runtime 管理，而是频道协作：

- 这个 channel 里有哪些可协作对象。
- 谁可以被 `@mention`、directed delivery、task assignment 或 action request 指向。
- 谁当前在线、离线、忙碌或可唤醒。
- 我能不能把某个 actor 加进来或移出去。

因此这里应该保留当前“打平的成员列表”设计：human、agent、service 都是 channel
member 候选和成员行里的 actor。类型差异只通过名字旁边的标签、头像形状、状态点和
可用操作表达，不按 human / agent / service 拆成多个区块。拆分区块会把架构分类强加
给协作视图，增加扫描成本，也会让用户误以为 agent/service 不是普通 channel 成员。

推荐成员行信息：

- avatar / display name / actor id 短别名。
- kind tag：成员、智能体、服务。
- presence tag：在线、离线、运行中、不可用。
- secondary tag：provider/model 或 service kind，只在有帮助时显示。
- row actions：invite、revoke、open actor settings、open profile，按 actor kind 和权限显示。

在 channel 中移除 agent 或 service member，不等于删除 AgentSpec / ServiceSpec。删除
AgentSpec，也不应自动从历史消息中删除 actor。GUI 可以在删除 agent runtime 后提示：
“是否同时从当前 channel membership 中移除这个 inactive actor？”

## 6. 目标数据模型

### 6.1 GUI 聚合模型

GUI 内部应该维护三个分开的 store：

```text
ActorDirectory
  server actor/list
  channel membership

DaemonRuntimeInventory
  server actor/list 中 daemon machine service actor 发布的 runtime inventory 投影

MachineCommandStore
  server machine/command.list/get/create/result events
```

当前代码已经有 `actors` 和 `machines`，但 command store 还没有显式存在。

### 6.2 Daemon runtime inventory 终态模型

这里不应设计成 “Machine inventory v3 兼容 v2”。终态模型要表达事实源如何组合，而不是把
当前 `_meta` 形状扩大一圈。更准确的名字是 daemon runtime inventory：它是 daemon 向
server 发布的只读投影，不是 machine 配置文件，也不是 provider / agent 的落盘容器。

终态事实源应拆开理解：

| 事实源 | 唯一写入方 | 落盘 / 运行位置 | inventory 中如何出现 |
| --- | --- | --- | --- |
| Machine identity | daemon 启动 / daemon config | `LOOM_CONFIG_DIR/daemon.toml` | `machine` 摘要 |
| Provider registry | daemon provider command / 内置 manifests | builtins + `LOOM_CONFIG_DIR/providers/<id>.json` | `providerRegistry` 摘要 |
| Agent registry | daemon agent command | `LOOM_CONFIG_DIR/agents/<actor_id>/spec.json` | `agentRegistry` 摘要 |
| Runtime state | daemon supervisor | 内存、worker connection、run/health 观测 | `runtimeState` 摘要 |
| Command log | server | server store | 不进 inventory；通过 `machine/command.*` 查询 |

因此 provider 绝不维护在 `MachineConfig` 中。`provider.add` / `provider.replace` /
`provider.remove` 是发给 daemon 的 machine command，daemon 写
`LOOM_CONFIG_DIR/providers/<provider_id>.json`，随后重新加载 ProviderRegistry 并发布新的
providerRegistry 摘要。MachineConfig 只保存 daemon 上线和归属需要的宿主字段。

同理，actors/agents 也绝不维护在 provider manifest 中。创建 agent 时，GUI 发送
`agent.create` command，daemon 写 `LOOM_CONFIG_DIR/agents/<actor_id>/spec.json`；这个
AgentSpec 通过 `providerRef.id` 引用 provider。一个 provider 可以被多个 AgentSpec
引用，一个 AgentSpec 只能选择一个 providerRef/mode 作为当前 runtime 接入方式。

终态投影可以是这样的概念形状：

```json
{
  "schema": "loom.daemon_runtime_inventory",
  "revision": 42,
  "observedAt": "2026-06-02T05:00:00Z",
  "machine": {
    "id": "local",
    "name": "dev-mac",
    "kind": "local",
    "workspaceId": "ws_123",
    "ownerActorId": "actor_human_123",
    "dataRoot": "/Users/me/.local/share/loom/agents",
    "configDir": "/Users/me/.loom"
  },
  "capabilities": [
    "inventory.read",
    "machine.command",
    "agent.create",
    "agent.update",
    "agent.remove",
    "provider.add",
    "provider.replace",
    "provider.remove",
    "provider.doctor"
  ],
  "providerRegistry": {
    "revision": 7,
    "entries": [
      {
        "id": "claude",
        "displayName": "Claude Code",
        "source": { "kind": "builtin" },
        "detection": {
          "status": "detected",
          "command": "/opt/homebrew/bin/claude"
        },
        "defaultMode": "print",
        "modes": [
          { "id": "print", "transport": "command" },
          { "id": "nonprint", "transport": "interactive_command" }
        ],
        "models": { "default": "sonnet", "choices": [] },
        "agentCount": 2
      }
    ]
  },
  "agentRegistry": {
    "revision": 19,
    "entries": [
      {
        "actor": {
          "id": "actor_claude_reviewer",
          "kind": "agent",
          "displayName": "Claude Reviewer"
        },
        "providerRef": {
          "id": "claude",
          "mode": "print",
          "model": "sonnet"
        },
        "autostart": true,
        "specHash": "sha256:..."
      }
    ]
  },
  "runtimeState": {
    "machineConnection": "online",
    "workers": [
      {
        "actorId": "actor_claude_reviewer",
        "status": "online",
        "activeRunId": null,
        "lastError": null
      }
    ]
  }
}
```

原则：

- inventory 是组合投影，不是配置文件；不要把 provider / agent 写成 machine config 子字段。
- providerRegistry entry 是 ProviderManifest 的安全摘要，不是 raw manifest，也不是 runtime plan。
- agentRegistry entry 是 AgentSpec 的安全摘要。完整 spec 通过 `agent.show` / `agent.patch`
  类 machine command 读取和修改。
- provider manifest raw/detail 用 `provider.show` 读取，按需 redaction。
- runtimeState 只描述当前观测到的运行状态，不替代 run / trace / delivery 等 server 事实。
- command history 不冗余进 inventory，仍由 server 的 `machine/command.*` 负责。
- GUI 如果需要把这些信息放在一个页面，只是在视图层 join；不能因此改变事实源边界。

### 6.3 Provider detail command

新增 daemon machine command：

```json
{ "op": "provider.show", "providerId": "claude", "redact": true }
{ "op": "provider.doctor", "providerId": "claude" }
{ "op": "provider.validate", "manifest": {...} }
{ "op": "provider.replace", "manifest": {...}, "ifSource": "local" }
```

其中：

- `provider.show` 返回 resolved manifest detail、source、detected info、runtime plan preview。
- `provider.doctor` 返回 checks，不写盘。
- `provider.validate` 只校验 manifest，不写盘。
- `provider.replace` 只允许替换 local provider，不允许 shadow builtin。

当前已有 `provider.add/remove` 可以保留，但 GUI 应把 validate/doctor 作为 add 前置步骤。

### 6.4 Agent patch command

当前 `agent.update` 以 value presence 表达 patch，够用但不够可审计。建议未来改为：

```json
{
  "op": "agent.patch",
  "actorId": "actor_codex",
  "patch": {
    "actor.displayName": "Codex Reviewer",
    "instructions": "Review diffs before replying.",
    "providerRef.mode": "print",
    "providerRef.model": "gpt-5.5",
    "memory.delivery.prompt": true
  }
}
```

近期开销更小的做法是继续用 `agent.update`，但 GUI IPC 需要补齐字段：

- `mode`
- `instructions`
- `metadata.description`
- `models`
- `trigger`
- `promptAssembly`
- `memory`
- `bundle`
- `announcement`
- `capabilities`

并且 `description` 不应再覆盖 `instructions`。两者应是两个独立字段：

- description：给人看的 UI 描述，进入 `actor._meta.description`。
- instructions：给 agent 的静态 system-side 指令，进入 `AgentSpec.instructions`。

### 6.5 Profile / memory commands

Profile / memory 的事实源在 daemon host 上，不能进 server，也不能由 GUI 直接读写本地路径。
需要新增面向 agent actor 的 daemon machine command：

```json
{ "op": "profile.show", "actorId": "actor_codex", "redact": true }
{ "op": "profile.read", "actorId": "actor_codex", "path": "claude/settings.json", "redact": true }
{ "op": "profile.patch", "actorId": "actor_codex", "path": "claude/settings.json", "patch": {...} }
{ "op": "memory.list", "actorId": "actor_codex", "query": { "status": "accepted", "limit": 50 } }
{ "op": "memory.append", "actorId": "actor_codex", "record": {...} }
{ "op": "memory.update", "actorId": "actor_codex", "memoryId": "mem_123", "patch": {...} }
{ "op": "memory.archive", "actorId": "actor_codex", "memoryId": "mem_123" }
```

原则：

- `profile.show` 返回 profile overview、已知 sections、safe paths、record counts，不返回整棵目录。
- `profile.read/patch` 只能访问 daemon 白名单内的 profile files；path 必须规范化后仍在
  `{agent.profile}` 下。
- provider-specific settings 需要按 ProviderManifest 声明或内置规则 redaction。
- memory records 用结构化 API 管理，不让 GUI 直接编辑 JSONL 文件。
- destructive 操作默认 archive / reject；hard delete 只作为高级动作，并记录 command result。
- agent worker 正在运行时，daemon 应判断是否需要 hot reload、restart 或仅下次 run 生效。

### 6.6 Prompt / 参数替换终态模型

旧实现的问题不是某个字段放错，而是 prompt assembly、prompt file loading、provider
delivery、transport template expansion 分散在多层：

- `AgentSpec.promptTemplate` 包装 user input。
- daemon worker 组装 Loom prompt parts。
- `ProviderManifest.prompt.workspaceFiles` 读取 agent workspace `.loom` 文件。
- `ProviderManifest.prompt.outputs` 决定 system/user/full 如何拼接。
- command / interactive transport 再用 `{...}` 替换 argv/env/stdin/template。

终态应收敛成四个概念。

#### Provider Prompt Interface

ProviderManifest 只描述接入产品的 prompt 接收方式，不描述 agent 个性：

```json
{
  "modes": {
    "print": {
      "transport": "command",
      "command": "{bin}",
      "args": ["run", "--model", "{model}", "--system", "{prompt.system}"],
      "stdin": "{prompt.user}",
      "promptInterface": {
        "accepts": ["system", "user", "full"],
        "preferred": "system_user"
      }
    }
  }
}
```

说明：

- `args` / `env` / `stdin` 可以引用 `{prompt.system}`、`{prompt.user}`、`{prompt.full}`、
  `{model}`、`{reasoningEffort}`、`{session.id}` 和运行路径变量。
- `promptInterface` 只用于 GUI 和 validate 解释这个 provider 支持什么输入形态；真正传参仍由
  argv/env/stdin template 决定。
- Provider 不再声明 `workspaceFiles`，也不再决定 `actor_context`、`memory`、`latest_message`
  的 include 顺序。
- interactive provider 可以保留自己的 completion contract / session template，因为这是产品接入
  协议，不是 agent 个性化 prompt。

#### Agent Prompt Assembly

AgentSpec 拥有 prompt 组装规则。建议把现有 `promptTemplate` 升级为更完整的
`promptAssembly`：

```json
{
  "promptAssembly": {
    "vars": { "style": "concise" },
    "files": [
      {
        "key": "persona",
        "root": "profile",
        "path": "prompts/persona.md",
        "roleHint": "system",
        "optional": true,
        "maxBytes": 32768
      },
      {
        "key": "project_rules",
        "root": "scopeWorkspace",
        "path": ".loom/rules.md",
        "roleHint": "system",
        "optional": true,
        "maxBytes": 32768
      }
    ],
    "outputs": {
      "system": {
        "include": [
          "actor_context",
          "agent_instructions",
          "file.persona",
          "file.project_rules",
          "bootstrap_memory"
        ],
        "join": "\n\n"
      },
      "user": {
        "include": [
          "turn_memory",
          "runtime_context",
          "assignment_context",
          "latest_message"
        ],
        "join": "\n\n"
      },
      "full": {
        "template": "{prompt.system}\n\n{prompt.user}"
      }
    }
  }
}
```

`root` 的含义：

- `profile`：`{agent.profile}/...`，跨 channel 的长期 agent prompt 文件。
- `scopeWorkspace`：当前 channel/thread 的 `{agent.workspace}/...`，适合项目规则、channel
  约定、临时操作说明。
- `bundle`：可选，读取已安装 bundle 中的只读 prompt 文件。

内置 part 仍由 Loom daemon 生成：`actor_context`、`agent_instructions`、`bootstrap_memory`、
`turn_memory`、`runtime_context`、`assignment_context`、`latest_message`、`scope_bootstrap`。

#### Prompt Files

GUI 可以在 Agent detail 中管理两类文件：

- Profile prompt files：`{agent.profile}/prompts/*.md`，适合 persona、长期操作规范、审查口径。
- Workspace prompt files：`{agent.workspace}/.loom/*.md`，适合当前 channel/thread 的项目规则。

文件编辑必须通过 daemon command：

```json
{ "op": "agent.file.list", "actorId": "actor_codex", "root": "profile", "prefix": "prompts/" }
{ "op": "agent.file.read", "actorId": "actor_codex", "root": "scopeWorkspace", "path": ".loom/rules.md" }
{ "op": "agent.file.write", "actorId": "actor_codex", "root": "profile", "path": "prompts/persona.md", "content": "..." }
```

daemon 负责 root whitelist、path normalization、symlink escape 检查、maxBytes、UTF-8 校验和
redaction。GUI 不直接打开 host 文件系统。

#### Prompt Render Preview

Prompt 保存前和 agent detail 中都应该能 preview：

```json
{
  "op": "agent.prompt.preview",
  "actorId": "actor_codex",
  "scope": { "kind": "channel", "id": "ch_123" },
  "sampleMessage": "review this change"
}
```

返回：

- `parts`：每个 part 的 key、title、source、byte count、是否为空、是否缺失。
- `outputs.system` / `outputs.user` / `outputs.full`。
- `bindings`：当前 provider mode 会把哪些 outputs 绑定到 argv/env/stdin。
- `warnings`：缺失文件、过大文件、未知变量、provider 不支持 system/user split 等。

#### 统一模板替换

终态应只有一个模板渲染器，daemon 在启动/preview 时统一执行：

1. 构造 typed render context：actor、scope、paths、model、reasoning、session、prompt outputs。
2. 渲染 Agent Prompt Assembly，得到 `PromptBundle`。
3. 渲染 Provider transport templates，得到 argv/env/stdin。
4. 未知变量在 validate / preview 阶段报错；只有显式 escape 的文本可以保留 `{...}`。

这样 CLI、GUI、daemon 使用同一套 preview/validate 结果，不再出现“GUI 看到的 prompt”和
“实际传给 provider 的 prompt”不一致。

## 7. 关键流程

### 7.1 创建 agent

```text
GUI
  选择 host
  选择 provider + mode + model
  填 actor identity + instructions
  可选选择 channel membership
  提交 machine/command.create(agent.create, ifInventoryRevision)

server
  校验 machine actor / owner / revision
  持久化 command
  通知 daemon

daemon
  校验 provider 可用
  生成 AgentSpec
  写 LOOM_CONFIG_DIR/agents/<actor_id>/spec.json
  刷新 inventory revision
  reconcile worker
  upsert machine inventory actor

GUI
  观察 command result
  重新拉取 inventory
  可选调用 channel/invite
```

设计要点：

- actor id 可以自动生成，但必须可预览。
- provider mode 必须可选；默认值来自 provider manifest。
- 创建成功后，agent actor 可能尚未在线，这是正常状态。
- channel invite 是协作层操作，不应写入 AgentSpec。

### 7.2 更新 agent

更新必须带 `ifInventoryRevision`。如果 revision conflict：

- GUI 重新拉 inventory。
- 展示 “daemon inventory 已变化”。
- 给出 rebase 后的 diff。
- 用户确认后重试。

更新 providerRef 时：

- daemon 必须重新验证 provider availability。
- model 不在新 provider choices 中时，GUI 应提示保留 custom model 或切到 default。
- worker fingerprint 改变后 daemon 自动重启对应 worker。

### 7.3 删除 agent

删除应被命名为 “Remove runtime”，不要叫 “Delete actor”。

流程：

1. `agent.remove` 删除 daemon-local AgentSpec。
2. daemon 停止 worker，刷新 inventory。
3. server actor identity 默认保留，保证历史消息可读。
4. GUI 从 active agent list 隐藏该 actor。
5. GUI 可提供可选项：从当前 channel / all visible channels 移除 membership。

不建议默认调用 `actor/delete`，因为历史 message、task、delivery、run 都引用 actor id。

### 7.4 添加 provider

```text
GUI import JSON
  -> provider.validate
  -> provider.doctor(optional, if command exists)
  -> show diff/source/manifest summary
  -> provider.add(replace=false)
  -> reload inventory
```

规则：

- 不能 shadow builtin provider id。
- 本地变体应使用新 id + `extends`。
- manifest 中 env value 如可能含 secret，GUI 应提示改用 env var reference。
- 添加 provider 不自动创建 agent。

### 7.5 替换 provider

仅 local provider 可替换。替换前 GUI 必须展示 impacted agents：

- 使用该 provider 的 actor ids。
- 当前 mode/model 是否仍存在。
- runtime plan 是否能 resolve。

替换成功后 daemon 会因 inventory fingerprint 改变而重启受影响 agents。

### 7.6 provider doctor

doctor 返回结构化 checks。最小 checks：

- manifest parses。
- schemaVersion supported。
- id valid。
- default mode exists。
- command detected。
- runtime plan resolves for each mode。
- model arg substitution works。
- prompt delivery bindings reference supported outputs and runtime vars。
- decoder builtin name valid。

GUI 用 check list 展示，不只显示一条错误字符串。

## 8. 后端改造建议

### 8.1 server

保持 server 不理解 provider / agent spec 的原则，只补强 command 可见性：

- `machine/command.create` 作为 GUI 默认写入口，替代同步 `machine/command`。
- `machine/command.list/get/cancel` 暴露给 GUI command history。
- machine command 更新应通过 actor inbox 或专用 notification 推给 requester。
- 保留 `ifInventoryRevision` 校验。

不做：

- 不在 server 校验 ProviderManifest schema。
- 不在 server 保存 provider raw manifest。
- 不在 server 生成 AgentSpec。

### 8.2 daemon

daemon 需要补齐可维护操作：

- `provider.show`
- `provider.validate`
- `provider.doctor`
- `provider.replace`
- `agent.patch` 或扩展 `agent.update`
- `agent.clone`
- `agent.disable`，等价于 `autostart=false` 并停止当前 worker，可选
- `agent.health`
- `agent.prompt.preview`
- `agent.file.list`
- `agent.file.read`
- `agent.file.write`
- `profile.show`
- `profile.read`
- `profile.patch`
- `memory.list`
- `memory.append`
- `memory.update`
- `memory.archive`

daemon 还应补齐 capabilities：

- `agent.create`
- `agent.update`
- `agent.remove`
- `agent.clone`
- `agent.prompt.preview`
- `agent.file.read`
- `agent.file.write`
- `provider.add`
- `provider.replace`
- `provider.remove`
- `provider.show`
- `provider.validate`
- `provider.doctor`
- `profile.show`
- `profile.patch`
- `memory.list`
- `memory.update`

### 8.3 GUI IPC

新增 typed wrappers：

- `machineCommandCreate`
- `machineCommandList`
- `machineCommandGet`
- `providerShow`
- `providerValidate`
- `providerDoctor`
- `providerAdd`
- `providerReplace`
- `providerRemove`
- `agentPatch`
- `agentHealth`
- `agentPromptPreview`
- `agentFileList`
- `agentFileRead`
- `agentFileWrite`
- `profileShow`
- `profileRead`
- `profilePatch`
- `memoryList`
- `memoryAppend`
- `memoryUpdate`
- `memoryArchive`

旧的 `machineAgentCreate` / `agentUpdate` / `machineAgentRemove` 可以保留为便利封装，但底层应复用 command store。

### 8.4 GUI 前端

前端拆分建议：

```text
features/actors/
  ActorsView.tsx
  registered-hosts/
  agents/
  services/
  provider-library/
  prompt-studio/
  agent-files/
  actorWorkspaceStore.ts
```

当前 `App.tsx` 已经很大，继续把 registered hosts、agent editor、provider library、service
editor 加进去会不可维护。Actors 工作区应从 `App.tsx` 抽出。

状态管理建议：

- `machinesById`
- `providerRegistryByMachineId`
- `agentsByActorId`
- `servicesByActorId`
- `profileOverviewByActorId`
- `memoryRecordsByActorId`
- `promptPreviewByActorId`
- `agentFilesByActorIdAndScope`
- `commandsById`
- `selectedActorWorkspaceScope`

### 8.5 CLI

CLI 也应围绕同一套事实源工作，避免 GUI-only 配置：

```text
loom provider show <provider_id>
loom provider doctor <provider_id>
loom provider example

loom agent prompt show <actor_id>
loom agent prompt preview <actor_id> --channel <channel_id> --message <text>
loom agent prompt patch <actor_id> --file prompt-assembly.json

loom agent file list <actor_id> --root profile --prefix prompts/
loom agent file read <actor_id> --root scope-workspace --channel <channel_id> --path .loom/rules.md
loom agent file write <actor_id> --root profile --path prompts/persona.md --file persona.md
```

原则：

- provider CLI 继续维护 ProviderManifest，但示例模板应强调 prompt delivery interface，而不是
  prompt workspace files。
- agent prompt CLI 通过 daemon command 操作 AgentSpec Prompt Assembly 和受控文件。
- `prompt preview` 使用 daemon 的同一个 renderer，输出 system/user/full、part source 和
  provider binding，不在 CLI 里重新拼装。
- remote host 和 local host 使用同一条 `machine/command` 链路。

## 9. UI 设计原则

### 9.1 创建少，编辑细

创建 agent 的目标是快速得到一个能参与协作的 actor，不是完成整份 AgentSpec。普通创建只需：

- host
- provider
- model
- name
- instructions
- autostart
- channel invite

mode、memory、bundle、trigger、Prompt Assembly、reasoning、raw spec 默认隐藏，放在 advanced
或创建后的编辑页里。编辑 agent 时再提供详细 tabs，因为这时用户通常是在调行为、迁移
host/provider 或排障，需要看到更多运行细节。

### 9.2 provider 是接入定义，prompt 是 agent 行为

ProviderManifest 影响命令执行、认证、会话和输出解析，错误配置会导致 worker 不工作或泄露上下文。GUI 必须提供：

- validate before save。
- diff before replace。
- impacted agents。
- doctor after save。
- source / redaction 明示。

但 provider 不应作为 `Actors` 工作区的二级主菜单，也不应在创建 agent 时先展示 manifest
taxonomy。它应该出现在 Agents 的 Provider Library 和 agent detail 的 Run Settings /
Diagnostics 中。

Provider detail 只展示 prompt delivery interface：这个产品接收 full prompt、system/user split、
stdin、argv、env 还是 interactive template。system prompt / user prompt 的内容、文件来源、
include 顺序和 preview 都在 Agent detail 的 Prompt Studio 中维护。

### 9.3 actor 身份和 runtime 配置分离

UI 文案需要固定：

- `Actor`：协作身份。
- `Agent runtime`：由 daemon 管的运行配置。
- `Provider`：外部 agent CLI 的接入配方。
- `Host`：运行 daemon 的机器。
- `Channel member`：某个 actor 在 channel 里的可见和可投递身份。

这条原则不能反向伤害 channel 视图。Channel Members 面板的职责就是展示成员，所以应
继续叫 Members，并继续打平展示所有 actor。需要改名的是 Actors / Registered Hosts 页面里当前
把 daemon-managed agents 放在 `Members` 小节的区域；那里展示的是机器上的 agent
runtime，不是一个 channel 的成员列表。

### 9.4 不把架构分类强行变成主 UI 分类

本设计需要显式避免一个倾向：看到后端有 human / agent / service、server / daemon /
GUI、AgentSpec / ProviderManifest / AgentConfigVersion，就把这些分类直接搬到每个
用户视图里。架构分类是维护者理解系统的工具，不一定是用户完成任务的最佳路径。

判断原则：

- 如果视图目标是“协作”，优先按协作关系组织，类型做标签。
- 如果视图目标是“actor 运行维护”，再按 registered host / provider / agent runtime 组织。
- 如果视图目标是“排障”，优先按症状和影响组织，底层链路作为展开信息。
- 如果视图目标是“选择对象”，优先按可选、最近、在线、相关性排序，类型做辅助筛选。

类似需要举一反三的地方：

- Mention picker：不应先拆 human / agent / service 三个 tab；应展示当前 scope 中可提到的
  actor，按匹配度、membership、在线状态排序，并用标签标识类型。
- Assignment / handoff picker：核心是“谁能接这个任务”，不应只按 actor kind 分类；应把
  capability、channel membership、是否在线、最近参与度放在前面。
- Message timeline：不应把 human 消息和 agent 消息分成不同时间线；统一按时间和 reply
  关系展示，actor kind 只影响头像、标签和可用操作。
- Task view：owner / requester / reviewer 都是 actor；不要做“人工任务”和“智能体任务”
  两套列表，除非用户正在按资源负载排班。
- Provider detail：普通用户创建 agent 时不应先看到 manifest taxonomy；manifest、decoder、
  prompt delivery interface 应进入 Provider Library 的高级维护和 doctor 视图。prompt 内容、
  文件和 system/user 组装不在 Provider detail。
- Command history：用户首先关心“什么操作、影响哪个 agent/provider、是否成功”，不是
  queued / delivered / ack / result 的协议机械过程；协议状态应作为详情。

### 9.5 删除语义必须保守

删除 provider、agent、actor、channel member 是不同动作：

- Remove provider：删除 local ProviderManifest。
- Remove agent runtime：删除 daemon-local AgentSpec。
- Remove channel member：从 channel ACL 移除 actor。
- Delete actor：删除 server actor directory 记录，默认不提供。

## 10. 落地路线

### Phase 1：修正当前管理面

目标：不大改协议，先把 GUI 现有能力做清楚。

- Hosts 页面升级为 Actors 工作区，二级菜单为 Registered Hosts / Agents / Services。
- Actors / Registered Hosts 页面里承载 daemon-managed agents 的 `Members` 区域改名为 Agents；
  channel 的 Members 面板保持当前语义和打平展示。
- Agent create 改成轻量向导：identity、host、provider/model、instructions、autostart、
  optional channel invite。
- agent 表单区分 description 和 instructions。
- agent update IPC 增加 `instructions`，不再让 description 覆盖 instructions。
- daemon reconcile honor `AgentSpec.autostart`；GUI 的 manual 状态必须和实际 worker
  启停一致。
- provider badge 增加 source/detected/defaultMode/modes summary。
- inventory capabilities 补 `agent.update`。
- 删除 host action 从默认 UI 移除或改成只读提示。

验证：

- `cargo check -p loom-gui -p loom-cli -p agent-runtime -p loom-server`
- daemon `agent.create/update/remove` tests。
- GUI TS type check。

### Phase 2：Provider Library 最小闭环

目标：GUI 可以在 Agents 页面内维护 local provider。

- daemon 增加 `provider.show/validate/doctor/replace`。
- GUI IPC 增加 provider wrappers。
- Agents > Provider Library：按 host 展示 provider readiness、prompt delivery interface、
  detail、doctor、add/remove。
- Add provider 使用 validate -> add -> reload inventory。
- Remove provider 阻止 actorCount > 0 的 provider。

验证：

- provider manifest validate tests。
- provider.add/remove/replace command tests。
- GUI provider form tests。

### Phase 3：Agent Prompt Studio 与 Profile / Memory 管理

目标：GUI 可以维护生产 actor 需要的 AgentSpec 字段，并能查看/编辑该 actor 的受控
profile / workspace prompt 内容。

- Agent detail tabs：Overview / Identity / Instructions / Run Settings / Prompt Studio /
  Profile / Memory / Bundle / Channels / Diagnostics / Raw Spec。
- `agent.patch` 或扩展 `agent.update` 支持 mode、memory、bundle、announcement、trigger、
  Prompt Assembly。
- 新增 `agent.prompt.preview` 和 `agent.file.list/read/write` daemon command。
- Prompt Studio 支持 system/user/full preview、part source inspection、profile prompt files、
  workspace prompt files、missing-file/unknown-var warnings。
- 新增 `profile.show/read/patch` 和 `memory.list/append/update/archive` daemon command。
- Profile / Memory tab 支持查看 profile overview、编辑 memory records、查看/编辑白名单内
  provider settings 和 MCP config。
- Raw Spec editor 支持 validate/diff/apply。
- Agent health command 返回 provider resolve、memory scaffold、bundle current、last run。
- active AgentConfigVersion 作为只读审计信息展示，不作为 GUI 写入源。

验证：

- AgentSpec roundtrip tests。
- prompt assembly render / provider binding preview tests。
- agent file path traversal / symlink / maxBytes tests。
- profile path traversal / redaction tests。
- memory record append/update/archive tests。
- patch conflict/revision tests。
- health command tests。

### Phase 4：command history 与异步操作

目标：所有 runtime mutation 都可观察、可重试、可审计。

- GUI 使用 `machine/command.create`，不再依赖同步 `machine/command`。
- Host / agent / provider detail 中的 command history 显示 queued/delivered/running/succeeded/
  failed/cancelled/expired。
- server 推送 command update 给 requester，或 GUI polling。
- 失败展示 `structuredError`。

验证：

- server command create/list/get/ack/result/cancel tests。
- GUI command store reducer tests。

## 11. 验收标准

设计落地后，应满足这些不变量：

1. GUI 不直接写 `LOOM_CONFIG_DIR/agents`、`LOOM_CONFIG_DIR/providers`、daemon.toml、profile、memory、bundle。
2. 所有 runtime mutation 都经过 `machine/command`，并带 `ifInventoryRevision`。
3. server 不解析 ProviderManifest，不生成 runtime transport。
4. daemon 是 AgentSpec / ProviderManifest / profile / memory 的唯一写入方。
5. provider command/env/decoder/session 不复制到 AgentSpec。
6. AgentSpec 不复制 ProviderManifest runtime plan。
7. ProviderManifest 不读取 profile/workspace prompt files，不决定 `actor_context`、memory、
   latest message 等 prompt parts 的 include 顺序。
8. Agent Prompt Assembly 是 system/user/full prompt 组装的 source of truth。
9. 删除 AgentSpec 不删除历史 actor 身份。
10. channel membership 不写入 AgentSpec。
11. provider add/replace 必须先 validate，replace 必须展示 impacted agents。
12. GUI 可以说明一个 agent 不工作的原因：host offline、provider missing、spec invalid、prompt file missing、model invalid、worker offline、last run failed。
13. `autostart=false` 的 AgentSpec 不应被 daemon 自动启动；GUI 的 manual/autostart 文案必须可由 runtime 状态证明。
14. GUI 可以查看和编辑 agent profile 的受控内容，但只能通过 daemon command；不能直接浏览
    或写入 host 文件系统。
15. Prompt preview 必须和 daemon 实际启动 worker 时使用同一套 renderer / variable validation。

## 12. 当前代码映射

| 设计点 | 当前代码位置 |
| --- | --- |
| ProviderManifest schema | `crates/proto/src/methods.rs` |
| ProviderRegistry / runtime plan | `crates/agent-runtime/src/provider.rs` |
| provider detection | `crates/agent-runtime/src/discovery.rs` |
| current ProviderPromptSpec / workspaceFiles / outputs | `crates/proto/src/methods.rs`、`crates/agent-runtime/src/provider.rs`；终态应迁移到 Agent Prompt Assembly |
| current prompt composer / AgentSpec promptTemplate | `crates/cli/src/cmd/agent_serve.rs` |
| current command template expansion | `crates/agent-runtime/src/command.rs`、`crates/agent-runtime/src/interactive.rs` |
| profile scaffold | `crates/agent-runtime/src/profile.rs` |
| memory records / JSONL store | `crates/agent-runtime/src/memory/` |
| AgentSpec schema | `crates/proto/src/methods.rs` |
| daemon AgentSpec / provider command | `crates/cli/src/cmd/daemon.rs` |
| daemon machine host command loop | `crates/cli/src/cmd/agent_serve.rs` |
| server machine command | `crates/server/src/handlers/mod.rs` |
| GUI machine inventory IPC | `crates/gui/src/ipc.rs` |
| GUI Host / Agent settings UI | `apps/gui-web/src/App.tsx` |

## 13. 自审记录

### Round 1

从“Loom 项目理念是什么”重新检查：

- 文档是否保持 server 纯消息枢纽：是，server 只保存 command/inventory，不解释 provider。
- 文档是否保持 daemon runtime ownership：是，AgentSpec / ProviderManifest 写入都在 daemon。
- 文档是否区分 Actor 和 AgentSpec：是，删除 runtime 不删除 actor 身份。
- 文档是否对比当前 GUI 吃到什么、没吃到什么：是，第 4 节有现状和缺口。
- 文档是否能落地：基本可以，第 10 节拆了四期。

发现的不足：

- 初版容易把 `AgentConfigVersion` 忽略。当前 runtime 会 publish / activate config version，但这不是 GUI 的 source of truth。
- 初版没有明确 stale actor 与 channel membership 的处理。
- 初版没有明确 description 和 instructions 混用是当前具体问题。
- 初版没有识别当前 `autostart` 只被保存/显示、未被 daemon reconcile honor 的问题。

处理结果：

- 已在第 3、4、5、6、7、9、10、11 节补充这些边界。

### Round 2

再次从当前实现细节检查：

- `provider.add/remove` 已经在 daemon command 中存在，GUI 未接入：文档已明确。
- `agent.update` 已经存在，但 inventory capability 未列出：文档已明确。
- GUI 当前 `MachineAgentProviderInfo` 是 summary，不含 manifest detail：文档已明确。
- server `MachineCommandUpdated` 当前不进 scope stream：文档已明确，并提出 command history/notification 方案。
- AgentSpec 高级字段 memory/bundle/announcement/trigger/Prompt Assembly：文档已明确列为未支持和 Phase 3。

结论：

本文已经满足用户要求的五个部分：理解项目理念、梳理 actor/provider 实现、对比 GUI 现状、提出合理架构、给出可落地路线。

### Round 3

最终逐项验收：

- “理解 Loom 项目理念”：第 2 节从协作域/接入域、actor 平等、server/daemon/GUI 边界重新归纳。
- “看 actor/provider 实现架构、方案和细节”：第 3、6、7、12 节覆盖 AgentSpec、ProviderManifest、ProviderRegistry、daemon inventory、machine command、AgentConfigVersion。
- “对比 GUI 功能吃了什么、没支持什么”：第 4 节按能力表列出现有支持和缺口。
- “从项目设计理念出发设计得更好”：第 5、8、9 节坚持 daemon ownership、server 纯消息枢纽、GUI 只发维护意图。
- “可以落地、架构清晰”：第 10、11 节给出四阶段路线和验收不变量。

未发现需要继续重写的问题，可以交付。

### Round 4

用户反馈指出一个更根本的问题：我把架构分类误投射到了 channel 视图。Channel
Members 面板的目标是让人舒服地理解和操作频道协作对象，不是教育用户 human / agent /
service 的后端分类。当前 GUI 把成员打平展示，并用名字旁标签标识类型，这个方向是对的。

修正结果：

- 第 5.4 节改为明确保留 channel members 的扁平 actor 列表，并把 service 纳入成员体系。
- 第 9.3 节澄清：需要改名的是 Actors / Registered Hosts 页面里的 daemon agent 列表，不是 channel Members。
- 新增第 9.4 节，作为通用设计反思：不要把架构分类强行变成主 UI 分类，并列出 mention picker、assignment picker、message timeline、task view、provider detail、command history 等类似风险。
- Phase 1 修正为只改 Actors / Registered Hosts 页面里的 `Members` 区域，channel Members 面板保持现状。

结论：原文在 channel member 视角上过度架构化，已修正。

### Round 5

用户继续指出 `Machine inventory v3` 一节有两个明显问题：

- 终态设计不应该写成兼容 v2 的过渡方案。
- provider 不应被暗示为维护在 Machine 配置中；provider 是通过 daemon command 加入
  daemon-local ProviderRegistry 的。

这暴露的是同一类问题：我把当前实现投影中的字段当成了终态事实源，把一个“大而全
machine inventory blob”当作设计中心，导致 MachineConfig、ProviderRegistry、
AgentRegistry、RuntimeState、CommandLog 之间的边界变得冗余。

修正结果：

- 第 6.2 节从 `Machine inventory v3 建议` 重写为
  `Daemon runtime inventory 终态模型`。
- 明确 inventory 是 daemon 发布的只读组合投影，不是配置文件。
- 明确 MachineConfig 只保存 daemon 上线和归属需要的宿主字段。
- 明确 ProviderRegistry 来自内置 manifests + `LOOM_CONFIG_DIR/providers/<id>.json`，
  由 `provider.add/replace/remove` machine command 写入。
- 明确 AgentRegistry 来自 `LOOM_CONFIG_DIR/agents/<actor_id>/spec.json`，由 agent command 写入。
- command history 不冗余进 inventory，仍通过 server `machine/command.*` 查询。
- GUI store 命名从 `MachineInventory` / `providersByMachine` 调整为
  `DaemonRuntimeInventory` / `providerRegistryByMachineId`，避免继续暗示 provider 是
  machine config 子字段。

结论：终态模型应表达多个事实源的组合关系，而不是把兼容态字段扩成一个更大的冗余结构。

### Round 6

用户指出页面命名仍然有问题：`Runtime` 太隐晦，`Actors & Providers` 又把未来形态锁死。

这暴露的是另一类 UI 思维问题：我先拿内部对象或工程术语给页面命名，再解释用户为什么要
接受这个名字。更好的做法是从页面解决的问题出发：这里维护的是 Loom 的执行能力，包括
host、provider、agent runtime、service、tools / bundles、scheduler、policy 和 command
history。

中间结论是尝试用 `Execution` 命名主导航。但继续推敲后发现它仍然偏工程动作，不够
Loom native，也没有自然表达 actor 是协作核心。这个中间方案已被 Round 7 的 `Actors`
工作区取代。

### Round 7

用户进一步提出方向：主工作区可以叫 `Actors`，里面包含 `Registered Hosts`、`Agents`、
`Services`。Registered Hosts 管 host 注册和基础管理；Agents 中同时看 agent 和 provider；
新增 agent / service 时选择注册到哪个 host。

这次修正的关键不是简单把 Hosts 改名为 Actors，而是重新确认产品逻辑：

- Loom 的协作核心是 actor，所以主导航应该回到 `Actors`。
- agent / service 的运行事实源在 daemon host 上，所以 host 必须是 Actors 工作区里的
  first-class registration flow。
- provider 不是 actor，也不是顶层业务对象；它是 agent 创建、运行和排障时需要的依赖库。
- 页面不能罗列后端对象。Registered Hosts、Agents、Services 各自要围绕用户任务组织默认信息。

修正结果：

- 第 1、5 节将主工作区确定为 `Actors`。
- 二级菜单收敛为 `Registered Hosts`、`Agents`、`Services`。
- Registered Hosts 增加 host 注册流程、readiness 默认展示、host detail 的产品逻辑。
- Agents 改为跨 host 的 agent roster；Provider Library 放入 Agents 页面内部。
- Create Agent 改为轻量向导，只暴露 identity、host、provider/model、instructions、
  autostart、optional channel invite。
- Edit Agent 改为详细 tabs：Overview、Identity、Instructions、Run Settings、Prompt、
  Memory、Bundle、Channels、Diagnostics、Raw Spec。
- Services 增加 service actor 的未来管理模型，新建 service 也按需选择 Registered Host。
- 前端拆分从 `features/execution` 改为 `features/actors`。

结论：`Actors` 是 Loom native 的主工作区；Registered Hosts 是运行承载入口，Agents /
Services 是 actor 类型维护入口，Provider Library 是 agent runtime 依赖入口。

### Round 8

用户追问：Agent detail 是否可以看到每个 agent 自己 profile 的内容并编辑。

这暴露了文档里一个缺口：我把 Profile / Memory 只写成了 `profile path`、memory policy 和
record count，没有明确 profile 是 per-actor 长期状态，也没有说明 GUI 应如何安全维护它。

修正结果：

- 第 5.2 节将 Agent detail 的 `Memory` tab 改成 `Profile / Memory`，明确可以查看和编辑
  profile overview、memory records、provider settings、MCP config。
- 明确 Create Agent 不展示 profile 内容；profile 属于创建后的长期上下文和调试维护。
- 新增第 6.5 节 `Profile / memory commands`，通过 daemon machine command 提供
  `profile.show/read/patch` 和 `memory.list/append/update/archive`。
- 明确 GUI 不能直接浏览或写入 `{agent.profile}` 目录；daemon 必须做 path whitelist、
  redaction、schema 校验和写盘。
- Phase 3、daemon capability、GUI IPC、验收标准都补齐 profile / memory 管理能力。

结论：可以在 Agent detail 中看和编辑每个 agent 的 profile，但要以结构化、受控、daemon-owned
的方式做，而不是把 GUI 变成本机文件编辑器。

### Round 9

用户指出当前参数替换和 prompt 拼装过于复杂，并提出核心方向：provider 负责“基础接入源”
和产品接入定义，agent 负责在这套定义上实现具体 prompt、profile/workspace 文件和
system/user prompt 组装。

重新读代码后确认旧实现确实混层：

- `AgentSpec.promptTemplate` 在 daemon worker 中包装 user input。
- daemon worker 再组合 Loom prompt parts。
- `ProviderManifest.prompt.workspaceFiles` 读取 workspace `.loom` 文件。
- `ProviderManifest.prompt.outputs` 决定 system/user/full 拼装。
- command / interactive transport 又各自做 `{...}` 替换。

修正结果：

- 第 2.5 节明确 ProviderManifest 只拥有 prompt delivery interface，不拥有 agent prompt 内容。
- 第 4.2 节把 prompt ownership 作为当前 GUI/架构缺口列出。
- 第 5.2 节新增 Agent detail 的 Prompt Studio 交互：sources、system/user builder、preview、
  profile prompt files、workspace prompt files。
- 第 6.6 节新增 Prompt / 参数替换终态模型：Provider Prompt Interface、Agent Prompt Assembly、
  Prompt Files、Prompt Render Preview、统一模板替换。
- 第 8 节补充 daemon command、GUI IPC 和 CLI：`agent.prompt.preview`、`agent.file.*`、
  `loom agent prompt ...`。
- Phase 3 改为 Agent Prompt Studio 与 Profile / Memory 管理。
- 验收标准新增：ProviderManifest 不读取 profile/workspace prompt files；Agent Prompt
  Assembly 是 system/user/full prompt source of truth；preview 必须和 daemon 启动使用同一
  renderer。

结论：终态应把“provider 怎么接收 prompt”和“agent 怎么生成 prompt”分开。Provider 是接入
定义；Agent 是行为实现；daemon 是唯一 renderer；GUI 和 CLI 都只调用 daemon preview /
validate / write command。
