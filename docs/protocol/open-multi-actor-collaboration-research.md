# 开放多参与者协作协议研究笔记

## 1. 文档目的

这份文档只做三件事：

1. 记录协议设计过程里真正影响模型边界的研究输入，方便后续查询和复盘。
2. 把 `channel / thread / session` 这些容易混淆的概念拆开，形成稳定术语。
3. 给协议草案设定一个明确的能力下限：后续正式协议至少要覆盖当前平台已经证明有价值的协作能力，但协议本体不依赖某个具体产品实现。

这不是协议正文。它保留研究过程、设计取舍和反例。

## 2. 研究问题

这轮研究集中回答五个问题：

1. 多参与者协作里的“主对象”到底是什么。
2. `channel` 和 `thread` 是否应该抽象成统一的作用域模型。
3. `session` 是否属于协作域，还是只属于运行时接入层。
4. 人和 Agent 是否需要两套不同协议。
5. 协议怎样既简洁，又能覆盖现有平台的关键能力。

## 3. 研究输入

### 3.1 内部输入：当前平台已经验证过的能力

从仓库实现看，当前平台已经稳定存在以下协作能力：

- 顶层共享空间中的消息时间线。
- 顶层空间下的局部线程。
- 显式 directed delivery 与 handoff。
- actor 被重新唤醒时可补看的持续上下文归属。
- 基于线程根消息的默认续接。
- 长运行 Agent 的逐步输出与完成事件。
- 权限请求、输入请求、选项请求。
- 显式 handoff。
- 显式 artifact 发布与稳定 URI 引用。
- 线程级摘要与结构化上下文文件。
- 面向查询的历史读取、搜索、窗口读取。

这些能力说明，协议至少不能退化成“只有一条消息流”的极简聊天协议。

### 3.2 外部输入：参考系统

这轮主要参考了五类系统：

- 邮件与 JMAP：
  [RFC 5322](https://www.rfc-editor.org/rfc/rfc5322.html)
  [RFC 8620](https://www.rfc-editor.org/rfc/rfc8620.html)
  [RFC 8621](https://www.rfc-editor.org/rfc/rfc8621.html)
- 社交分发模型：
  [ActivityStreams 2.0](https://www.w3.org/TR/activitystreams-core/)
  [ActivityPub](https://www.w3.org/TR/activitypub/)
- 实时协作系统：
  [Matrix Spec](https://spec.matrix.org/)
  [RFC 6120 XMPP Core](https://www.rfc-editor.org/rfc/rfc6120.html)
- 现代聊天产品：
  [Discord Threads](https://docs.discord.com/developers/topics/threads)

## 4. 关键观察

### 4.1 协作系统的主轴不是 Session，而是 Scope

邮件、社交和实时通信虽然形态不同，但有一个共同点：真正稳定的对象不是“当前连接”，而是“消息发生在哪个上下文里”。

- 邮件里是 message 和 thread。
- Matrix 里是 room 和 event。
- Discord 里是 channel 和 thread。
- ActivityPub 里是 actor、object 和 activity 关系。
- XMPP 里明确区分身份、连接和资源绑定。

这说明协作协议的主轴应该是 `scope + event`，不是 `session + prompt`。

### 4.2 Thread 不是 Session 的别名

`thread` 的本质是局部收敛作用域，不是客户端和服务器之间的一次在线会话。

如果把 `thread` 做成 `session`：

- 历史记录会和在线状态耦合。
- 离线重放会变得别扭。
- 多终端接入同一对话会缺乏稳定主键。
- Agent runtime 私有会话会污染协作模型。

所以 `thread` 应该归入作用域层。

### 4.3 Session 这个词在工程里经常同时指三件事

这类系统里最容易混淆的不是 `channel`，而是 `session`。它通常同时指：

- 登录态或鉴权态。
- 客户端和服务器之间的一次连接上下文。
- Agent runtime 内部的一次执行会话。

如果协议正文里继续直接使用裸词 `session`，后续讨论几乎一定会反复混淆。

结论很直接：

- 协作域不要使用裸词 `session` 作为核心对象。
- 协议正文里应拆成 `connection`、`auth session`、`runtime session`。
- `runtime session` 只属于接入适配层，不属于协作域模型。

### 4.4 人和 Agent 在领域模型里应平等

从 ActivityPub 这种模型看，`Actor` 是一个很成熟的抽象。它不关心参与者是人、机器人还是服务，只关心：

- 谁有身份。
- 谁能发出事件。
- 谁会收到投递。
- 谁可以确认、接力、回应。

如果协议把“人类客户端”和“Agent 客户端”当作两套不同的领域对象，后面所有 handoff、directed delivery、receipt、membership 都会被迫做两套语义。

因此更好的做法是：

- 领域层统一用 `Actor`。
- 接入层用 `Endpoint` 区分 GUI client、CLI client、Agent adapter。
- 能力差异通过 capability 表达，不通过地位表达。

### 4.5 现代聊天 UI 很重要，但不能反过来支配协议

Discord、Slack 一类产品证明了 `channel + thread + addressing` 的交互模型很好用，但这些产品的 UI 对象不应该直接变成协议对象。

协议需要比产品 UI 更稳定、更抽象：

- UI 里可以叫频道、房间、论坛、项目、工单。
- 协议里更适合统一抽象成 `channel`。
- UI 里可以叫线程、话题、子讨论、reply chain。
- 协议里更适合统一抽象成 `thread`。

这样协议可以承载 Discord 风格界面，也可以承载邮件视图、工单视图或 timeline 视图。

## 5. 从参考系统里借什么，不借什么

| 参考系统 | 借鉴点 | 不照搬的部分 |
| --- | --- | --- |
| 邮件 / RFC 5322 | `Message-ID`、`In-Reply-To`、`References` 这类显式关系语义 | 邮箱、文件夹、收件箱不是协作域主对象 |
| JMAP | 把 `session` 理解成能力和访问上下文，而不是会话主题 | 账户和邮箱组织方式不适合作为协作主模型 |
| ActivityStreams / ActivityPub | `Actor` 平等、对象间关系显式、收件与分发可建模 | 联邦发现、远程签名和分布式复杂度先不进入 v0 |
| Matrix | 房间作为主作用域、事件不可变、线程是关系或子上下文 | 状态事件解析和复杂 DAG 解决方案过重 |
| XMPP | 身份、连接、资源绑定分层清楚 | XML stanza 形状和历史包袱不必继承 |
| Discord | `channel + thread` 的交互直觉、局部讨论模型 | 产品命名和 UI 细节不该直接进入协议 |

## 6. 术语澄清

这轮研究建议把术语固定成下面这组：

- `Actor`：稳定参与者。人、Agent、服务都属于它。
- `Endpoint`：Actor 的一个接入端实现，比如 GUI、CLI、bot adapter。
- `Connection`：Endpoint 和服务器的一次在线连接。
- `Runtime Session`：Endpoint 内部和模型、工具运行时之间的一次私有会话。
- `Channel`：顶层共享作用域。
- `Thread`：`Channel` 下的子作用域，用于局部收敛。
- `Turn`：某个 Actor 在一个作用域中的一次处理回合。
- `Event`：不可变事实。
- `Relation`：事件到事件、事件到 Actor、事件到 Artifact 的显式边。
- `Artifact`：独立发布的共享产物。
- `Delivery`：某个事件对某个 Actor 的投递状态。
- `Receipt`：投递后的确认状态。

这里最重要的不是名词本身，而是边界：

- `Channel / Thread / Turn / Event` 属于协作域。
- `Endpoint / Connection / Runtime Session` 属于接入域。

## 7. 设计决策

### 7.1 采用两层模型，而不是一锅端

协议应明确分成两层：

- 协作层：定义 Actor、Scope、Event、Relation、Artifact、Delivery。
- 接入层：定义连接建立、能力协商、流式订阅、认证、适配器私有运行时。

这样可以避免把 ACP 一类 runtime 协议错误提升为协作协议主模型。

### 7.2 采用统一 Scope 模型

比起直接把 `channel` 和 `thread` 写死，协议更适合统一成：

- `Channel`：顶层作用域。
- `Thread`：`Channel` 的子作用域。

这样做的好处：

- `channel` 只是某类 UI 命名。
- `thread` 只是某类 UI 命名。
- 协议对产品形态更开放。

### 7.3 采用 Event + Relation，而不是把行为写死成一堆特殊消息

如果把 message、directed delivery、handoff、artifact、action request 都设计成互不相干的特殊消息类型，协议会越来越重。

更简洁的方式是：

- `Event` 负责承载事实。
- `Relation` 负责表达指向。
- `type + payload` 负责表达具体事件形态。

于是：

- `reply` 是 `replies_to` 关系。
- 面向机器的显式定向是 `targets` 或 `hands_off_to` 关系。
- 文本里的 `@handle` 属于 binding 层输入语法，不是核心协议关系。
- handoff 是 `hands_off_to` 关系。
- artifact 关联是 `attaches_artifact` 关系。

### 7.4 采用 Turn 作为执行单元

多参与者系统里，“一条消息”不足以表达一次执行过程。一个 Agent 的一次处理通常包含：

- 若干文本增量。
- 若干工具轨迹。
- 若干 action request。
- 一个完成或失败状态。

因此需要 `Turn` 作为聚合单元。`Event` 仍然保持不可变，但可以按 `turn_id + seq` 被组织成一个可回看的执行回合。

### 7.5 发布共享产物，而不是共享私有路径

路径不是协议对象，已发布产物才是协议对象。

这意味着：

- 协议关心 `artifact_id`、`artifact_uri`、`metadata`。
- 协议不应依赖某个参与者的本地绝对路径。

这是把“草稿”和“共享事实”分开的关键。

## 8. 明确拒绝的方案

### 8.1 拒绝把 Thread 设计成 Session

原因：

- 它混淆协作上下文和运行时连接。
- 它让历史与在线态耦合。
- 它不利于多端、多客户端和离线重放。

### 8.2 拒绝把人和 Agent 分成两套协作对象

原因：

- handoff、directed delivery、receipt、delivery 都会重复一遍。
- 人和 Agent 的混合协作会变得别扭。

### 8.3 拒绝把工作流 DAG 作为核心对象

原因：

- 它过早假设有一个中心调度者。
- 它不适合临时介入、局部讨论、半结构化协作。

工作流可以作为上层应用能力存在，但不是协议中心。

### 8.4 拒绝把可变消息作为最小事实单元

原因：

- 流式输出、工具轨迹、审批请求都更适合表示成不可变事件流。
- UI 可以做聚合，协议不必用“修改一条消息内容”作为主语义。

## 9. 能力下限清单

后续正式协议至少要覆盖以下能力。这份清单是验收底线。

| 能力 | 协议中至少需要的抽象 |
| --- | --- |
| 顶层共享时间线 | `Channel + Event` |
| 局部子讨论 | `Thread` |
| `reply` 关系 | `Relation(replies_to)` |
| actor 定向投递 | `Relation(targets or hands_off_to) + Delivery` |
| 显式 handoff | `Event + Relation(hands_off_to)` |
| 流式输出 | `Turn + ordered Event` |
| 工具轨迹 | turn 私有 trace 通道（owner-only），不进事件流 |
| 用户审批/输入/选择 | `action.request / action.response` |
| 共享文件与稳定引用 | `Artifact + artifact URI` |
| 历史检索与窗口读取 | `scope.read / event.list` 一类能力 |
| 可补看的上下文归属与实时更新 | `Membership + stream binding` |
| 参与者平等 | 统一的 `Actor` 模型 |

这份清单的意义不是要求兼容某个现有实现，而是要求新协议不能把这些已经证明必要的协作能力做丢。

## 10. 研究结论

可以把这轮研究压缩成六条结论：

1. 协作协议的主轴应该是 `scope + event`，不是 `session + prompt`。
2. `Channel` 和 `Thread` 属于协作域，`Connection` 和 `Runtime Session` 属于接入域。
3. `Actor` 必须统一建模，人和 Agent 在领域层平等。
4. `Turn` 必须存在，用来承载一次可回看的处理过程。
5. `Artifact` 必须是一等对象，不能退化成路径字符串。
6. 协议正文应尽量中立，不携带具体产品命名和具体运行时协议实现细节。

## 11. 下一步文档边界

基于这份研究笔记，后续应拆成两类文档：

- 协议草案：
  只定义对象、约束、事件和接口，不包含具体产品历史包袱。
- 独立实现文档：
  单独讨论某个平台怎样适配、怎样重构、怎样验收覆盖能力。
