# Agent Turn Input 现状分析与改进建议

状态：分析 / 提议

日期：2026-07-07

范围：GUI（及 CLI）发出一条消息后，agent 实际收到什么；agent 在运行中持续收到
消息时 runtime 如何处理；以上行为与 Loom 核心理念的偏差；改进建议。

本文基于当前代码（`crates/cli/src/cmd/agent_serve.rs`、
`crates/agent-runtime/*`、`crates/server/src/store.rs`、
`apps/gui-web/src/App.tsx`）逐行核对，关键结论均附代码位置，见附录 A。

---

## 1. 结论摘要

1. 一条 GUI 消息到达 agent 时，会被渲染成一份**单一的、扁平的 user prompt**：
   本地时间上下文 + 最近 20 条消息回放 + `=== Latest Loom message ===` 头 +
   消息正文 + 每回合重复的"Response delivery reminder"操作提示，外加可选的
   memory / profile / assignment 段。静态契约在 workspace 的 `AGENTS.md`、
   默认 `loom` skill 与 `loom guide` 中（这部分符合
   `agent-runtime-awareness.md` 的设计）。
2. 运行中持续收到的消息按"会话族"（thread root / DM）排队，**一条消息 = 一次
   完整 turn**，严格串行重放；没有合并（coalescing）、没有中途注入、没有
   supersede，运行中的 turn 无法感知新消息，只能等它跑完。
3. 用户的判断成立：问题不是信息不够，而是**信息的形态不合理**——turn input 把
   "事实、投递元数据、操作手册、历史回放、任务上下文"五种职责压进一个
   `user_message`，且与 provider session、`AGENTS.md`、上一回合的回放多层重复；
   同时它把 Loom 已经拥有的结构化事实（Delivery/Receipt、intent、时间戳）降级
   成了自然语言近似。
4. 另有一个实现级缺陷：turn 并发键（`turn_key`，按 thread 族）与执行串行键
   （`scope.id`）不一致，同一 channel 内两条并发根消息会互相覆盖 active turn /
   争抢 provider session。

---

## 2. 现状：一条 GUI 消息的完整链路

### 2.1 GUI 侧（apps/gui-web）

`sendMessage` / `sendThreadMessage` / `sendDirectMessage`（`App.tsx`）组装
`message.send` RPC：

| 字段 | 来源 | 说明 |
| --- | --- | --- |
| `target` | 当前视图 | `#channel`、`#channel:rootMsg`（thread）、`dm:@actor` |
| `body` | 输入框原文 | 不做改写 |
| `parentMessageId` | 回复对象 | 仅频道回复 |
| `audience` | @mention 解析 + 回复对象 | `kind: actor` 列表 |
| `deliveryPolicy` | 计算 | audience 中含 agent → `wake_agent`，否则 `notify_only` |
| `intent` | 计算 | 唤醒 agent → `request_action`，否则 `chat` |

要点：**GUI 里没有"只聊天不唤醒"的对 agent 表达方式**——只要 @ 了 agent 或回复
agent，一律 `wake_agent + request_action`；DM 更是无条件
`wake_agent + request_action`（哪怕只是寒暄）。反过来，频道里不 @ 任何人的消息
不产生任何投递，agent 完全不会被唤醒（只能在未来某次 turn 的回放里"顺带"看到）。

### 2.2 Server 侧（crates/server）

`message.send` 追加不可变 message 事实，然后：

1. 向 scope 订阅连接广播 `MESSAGE_CREATED`；
2. 按 `message_delivery_recipients` 计算收件人（显式 audience actor、
   `@all`/`@agents`（仅 `wake_agent` 时包含 agent）、group、thread attention
   （presence following + task owner）、private-to），为每个收件人建立持久
   Delivery（durable inbox）；
3. `request_action + wake_agent` 但没有显式路由目标的消息直接拒绝。

Server 不启动任何 agent，只记录事实与投递——这一层与协议理念一致。

### 2.3 Agent worker 侧：唤醒判定与排队

每个 agent 由 `loom-daemon` 下的 worker 驱动（`agent_serve.rs`）：

- **两条接收路径**：WS 实时流 `notification_loop`（`MESSAGE_CREATED` /
  `EVENT_CREATED`）+ 每 15s 轮询 durable inbox（`drain_pending_inbox`，启动时
  也 drain 一次；超过 6h 的 pending 直接丢弃）。`seen_sources` 去重。
- **唤醒条件**（`is_message_for_us_with_delivery`）：DM；audience 命中自己且
  `wake_agent`（或走 inbox 投递且非 silent）；`@all`/`@agents` 且 `wake_agent`；
  thread 内产生给自己的 inbox 投递（主要是 task owner 通知）。自己发的、
  runtime failure 消息忽略。
- **排队键 `turn_key`**：`dm:@actor` / `thread-root:<channel>:<rootMsg>` /
  频道根消息各自成键。`begin_or_enqueue` 原子判断：该键空闲则立即 dispatch，
  忙则入 `pending_triggers` FIFO；人类或 channel-scope 触发插到 service 触发
  之前（`is_priority_trigger`）。
- **turn 生命周期**：`dispatch_trigger` → `run.open` → 组 prompt →
  `adapter.send_prompt`（每 turn 拉起一次 provider CLI 进程，按 scope 持久
  session `--resume`）→ `Finished` → 发布/丢弃文本 → `run.close` → **ack
  delivery** → `finish_and_next` 弹出下一条排队触发，开新 turn。

### 2.4 Agent 实际收到的 prompt（默认配置逐段还原）

对一个未配置 memory / prompt_template 的 agent，`{prompt.full}`（stdin 或
argv）形如：

```text
=== System: Local time context ===            ← 每回合注入（约 7 行）
Current local time: 2026-07-07T00:20:00+08:00
Current UTC time: ...（含时区换算说明）

=== Recent Loom conversation ===              ← 每回合注入（≤20 条，每条截 800 字）
These are prior messages in the same thread/channel. Continue from them;
do not repeat a number or answer another actor already supplied.
- canfuu (@actor_human_xxx): 上一条消息...
- 小助手 (@actor_agent_yyy): 自己上一轮的回复...

=== User message ===
=== Latest Loom message ===                   ← 触发消息头
Your actor: 小助手 (@actor_agent_yyy)
From: canfuu (@actor_human_xxx)
Scope: thread:th_zzz
Message id: msg_123
Delivery: explicit route to you
Route target(s): 小助手 (@actor_agent_yyy)
(Visibility: private to ...  — 私密消息时)
(Task id / status / owner / Assignment id / Expected output — 带任务元数据时)
Visible message:
route -> 小助手 (@actor_agent_yyy): <正文，@actor_id 被改写为 显示名 (@id)>

Response delivery reminder:                   ← 每回合重复（约 10 行固定文本）
If this message asks you to answer, ... make that answer visible by
executing a Loom CLI message command before ending the turn. ...
Closeout checklist: if another actor must act, wake them ...

(=== Loom assignment context === + 完整 assignment JSON + 生命周期规则 — 有 assignment 时)
```

在此之外，agent 还叠加以下输入面：

- **workspace `AGENTS.md`**（provider CLI 原生自动读取，每 turn 前由 Loom 刷新
  marker 块）：actor 身份、channel 信息、成员快照、操作规则、稳定 instructions；
- **默认 `loom` skill + `loom guide`**：场景路由与手册（按需读取）；
- **provider session**：per-scope `--resume`，provider 侧自己积累了完整对话
  历史（包括历史上每一回合被注入的回放与 reminder）；
- **环境变量**：`LOOM_REPLY_TARGET`、`LOOM_TRIGGER_MESSAGE_ID` 等；
- 可选段：`bootstrap_memory` / `turn_memory`（配置 memory 时，每回合注入）、
  `profile prompts/` 文件（每回合注入）、spec 级 `prompt_template`
  前后缀与 trigger prefix（first_turn / every_turn）。

一个值得注意的不对称：**频道根消息触发时，"Recent Loom conversation" 取的是
该消息锚定的 canonical thread（通常为空），而不是频道主干**——即 agent 被频道
根消息唤醒时，prompt 里没有任何频道上下文，需要它自己用 CLI 查询；而 thread
内触发则回放该 thread 最近 20 条。

### 2.5 回复路径

Assistant 最终文本默认**不**自动发布（`LOOM_AGENT_AUTO_PUBLISH_FINAL` 默认
关），它只是 run transcript；agent 必须显式执行
`loom --json message send --target "$LOOM_REPLY_TARGET" ...` 或
`loom --json run ignore` 结束回合——这正是每回合注入那段 reminder 的动机。

---

## 3. 现状：运行中持续收到消息的处理

以"agent 正在跑一个 5 分钟的 turn，期间同一 thread 连续来了 5 条消息"为例：

1. **全部排队**：5 条消息按到达顺序进入该 `turn_key` 的 FIFO（人类消息插到
   service 回调前）；同 id 去重；队列无长度上限。
2. **正在跑的 turn 全程不知道**这 5 条消息的存在。没有任何机制把新消息注入
   运行中的 provider 进程；也没有"新的人类消息到达 → 取消当前 turn 重跑"的
   策略。人类唯一的干预手段是在 GUI 手动 cancel run。
3. **turn 结束后逐条重放**：`finish_and_next` 每次只弹一条，开一个**完整的
   新 turn**（新 `run.open`、完整 envelope、一次 provider 进程调用）。5 条消息
   = 5 个串行 turn。
4. **重复回答风险**：第 2 个 turn 的"Recent conversation"里已经包含第 3、4、5
   条消息，模型很可能一并回应；随后第 3、4、5 条又各自触发一个 turn。系统对此
   的防御只有两句软约束：回放区的"do not repeat a number or answer another
   actor already supplied"，以及协作协议里的 send-time rebase（`--if-latest`）
   约定。
5. **时效性**：消息在队列里不会过期（6h 上限只作用于 durable inbox drain）。
   一个长 turn 之后，agent 可能花几个 turn 逐条"考古"早已被后续消息推翻的请求。
6. **跨会话族并发与冲突**：不同 `turn_key`（不同 thread root、不同 DM）允许
   并发 dispatch。但 `active_turns` 与 adapter 的 in-flight/session 都以
   `scope.id` 为键：
   - 同一 channel 的两条并发根消息（两个 turn_key，同一 channel scope）会：
     `set_turn` 互相覆盖 active turn 记录；command transport 下第二个
     `send_prompt` 直接报 "session already in flight" 失败并产生失败通告；
     interactive transport 下则并发争抢同一 session 文件与 pid 槽（cancel 可能
     杀错进程）。
   - 即**设计意图（按会话族并发）与执行模型（按 scope 串行）不一致**。
7. **崩溃恢复**：worker 重启后 drain durable inbox，把 6h 内未 ack 的触发逐条
   跑一遍；delivery 在 turn Finished（无论成败）时 ack。

---

## 4. 评审基准：Loom 的核心理念

从 README 与协议文档提炼，与本文相关的理念有六条：

1. **Actor 平等的通信图**：人、agent、脚本、服务统一为 Actor；Loom 是消息图
   与 runtime 桥接层，不是聊天机器人外壳（README）。
2. **对象原子、职责单一**；**Event 不可变**；**关系显式，不靠客户端推断**
   （protocol-v0 §1/§3）。
3. **Membership 与 Delivery 分离**：某 actor 是否属于上下文、某事实是否投递给
   它，是两个独立的、机器可读的事实（protocol-v0 §3.7）。
4. **运行规则外置**：不注入 Loom system prompt；`AGENTS.md` 是最小启动契约，
   skill 指路，guide 解释；**"不应该依赖一段很长的 user message"**
   （agent-runtime-awareness）。同文档明确：turn 动态信息的最终形态"由后续
   prompt 设计单独定义"——即当前 turn input 是一个未定稿的过渡态。
5. **静态/动态分层**：`AGENTS.md` 只放低频稳定事实；scope/trigger/task 等动态
   事实随 turn 进入 provider（agent-runtime-awareness"基础上下文与动态上下文
   边界"）。
6. **开工前 claim，发送前 rebase**：channel 主干只放短答与入口，工作在
   canonical thread 推进；可见回复必须显式发送；无事可做用 `run ignore`
   （agent-coordination-workflow）。

一句话：**agent 的自主性应当来自"可查询的事实图 + 显式投递 + 工具 + 稳定契约"，
而不是来自一份越堆越长的 prompt。**

---

## 5. 设计缺陷分析

### 5.1 职责混杂：一个 `user_message` 承担五种角色

当前 `=== User message ===` 段里同时存在：

| 成分 | 本质 | 应属层次 |
| --- | --- | --- |
| Latest message 头（from/scope/id/route） | 投递事实 | 结构化 turn header |
| 消息正文 | 不可变事实 | 引用/隔离块 |
| Response delivery reminder + closeout checklist | 操作手册 | AGENTS.md / skill（静态） |
| Recent conversation 回放 | 历史近似 | provider session / 按需查询 |
| assignment JSON + 生命周期规则 | 任务合同 + 手册 | 合同引用 + 静态规则 |

这直接违背协议自己的"对象原子、职责单一"，也违背 agent-runtime-awareness 的
"不依赖长 user message"。最典型的是那段每回合重复的 reminder：它是 Loom 通用
操作规则（该文档明确要求外置），却被放在整个 prompt 注意力最高的位置（user
message 末尾），且与 `AGENTS.md` 里的同主题规则（"Assistant text is an
internal run transcript..."）**逐字重复**。模型每一回合都要重新读一遍它已经
读过 n 遍的内容。

### 5.2 四层重复：同一信息最多被投递四次

以一条普通 thread 消息为例，模型在一次 turn 中可能同时从以下渠道看到同一事实：

1. provider session 历史（`--resume`，上回合的完整 envelope 都在里面）；
2. 本回合的 Recent conversation 回放（含 agent 自己上回合的发言）；
3. 本回合的 Latest message（上回合回放里可能已出现过它）；
4. `AGENTS.md` / reminder 的规则重复。

ISSUES #5 曾经修复过 instructions 级别的膨胀（47KB/turn → 5KB/turn），但
**turn 级别的重复模式原样保留**：session 每多一回合，就多沉淀一份 20 条回放 +
一份 reminder + 一份时间上下文。这不仅是 token 成本问题——重复呈现同一事实的
不同快照，要求模型自行对齐"哪份是最新的"，反而增加出错面（也与"Event 不可变、
事实只有一份"的理念相悖）。

### 5.3 视角错位：把"世界的变化"编码成"用户对我说话"

所有触发——人类消息、service 回调、reminder 到期、assignment 状态流转、event
——最终都被渲染成 `=== User message ===`。后果：

- **intent 丢失**。server 明明存了 `intent`（`chat` / `request_action` /
  `status_update`）与 `deliveryPolicy`，prompt 里却完全没有体现；模型无法区分
  "需要交付的请求"与"仅供知晓的状态"，只能靠正文猜。`run ignore` 的判断因此
  缺乏第一手依据。
- **多方对话被压成两人对话**。provider CLI 的世界观是 user/assistant 轮替，
  Loom 把频道里所有其他 actor 的发言都折叠进"user"一侧。于是需要每回合在头部
  重新声明 "Your actor: ..."、把正文里的 @id 改写成显示名——这些都是在为
  "把图状协作塞进线状聊天"这个错位打补丁。改写正文本身也与"事实不可变"的精神
  相抵触（虽然只发生在 prompt 层）。

### 5.4 唤醒粒度 = 单条消息，缺合并与打断

- 人类在聊天工具里的自然行为是分段连发；当前模型下每一段都是一次完整 turn
  （一次 provider 进程、一份 envelope、一次 run 记录），且串行执行。§3 描述的
  重复回答、考古式逐条重放，都源自"唤醒粒度=消息"这个选择。
- 反向缺口：turn 运行中无法注入、无法策略性打断。用户发"停一下/需求变了"，
  要等当前 turn 自然结束才被看到。对一个以"人机混合持续协作"为目标的系统，
  这是体验上最尖锐的缺陷。

### 5.5 上下文选择不使用 Loom 自己的投递事实

Loom 拥有精确的 Delivery/Receipt 模型（哪些事实投递给了我、我确认到哪了），
但 turn input 的上下文选择是"该 target 最近 20 条、每条截 800 字、无时间戳"：

- 短对话里全是模型已见过的内容（纯冗余）；
- 超过 20 条的未读被静默截断，模型甚至不知道存在 gap；
- 回放行没有时间戳——runtime 一边用 7 行教模型换算时区，一边在真正需要时间的
  地方（消息间隔多久）不给时间；
- "do not repeat a number..." 这类针对特定演示场景的措辞泄漏进了通用 runtime。

正确的原语其实已经存在：**"自上一 turn ack 之后新增的投递"就是天然的增量
上下文**，而这恰好是 Delivery 模型的本职。

### 5.6 静态/动态分层互相渗透

按 agent-runtime-awareness 的边界：

- 静态却被每回合动态注入：reminder（操作规则）、profile prompt files、
  bootstrap_memory（在 resume session 中每回合重复）；
- 动态却被静态快照：`AGENTS.md` 里的频道成员列表是写文件时的快照，成员变更后
  到下一次刷新前是误导信息；而 turn prompt 又不提供成员/在线状态（这是对的，
  应该 CLI 查询，但两处一叠加，agent 拿到的是"过期快照 + 无最新值"）。

### 5.7 伪结构化文本与注入面

`=== xxx ===` 分节加 key-value 行是 Loom 自造的弱格式：无 schema、无转义。
消息正文被原样内插进 `Visible message:` 之后——一条正文里包含
`=== Latest Loom message ===\nFrom: ...` 的消息可以伪造投递头（prompt
injection 面）。协议在 RPC 层是严格 JSON schema，到了离模型最近的一层反而
退化成非形式化文本。

### 5.8 并发模型不一致（实现缺陷）

§3.6 所述：`turn_key` 按 thread 族设计并发，`active_turns` / adapter in-flight
/ provider session 按 `scope.id` 串行。同频道两条并发根消息会触发 active turn
覆盖与 "session already in flight" 失败噪音。要么统一到 scope 串行，要么把
执行侧全部改为 turn_key 粒度——当前的中间态两头不占。

### 5.9 队列缺乏 supersede / 新鲜度语义

FIFO + 人类优先是好的开始，但缺少：同 author 同 scope 的后到消息吞并先到消息
（supersede）、队列老化、以及"队列里还有 N 条待处理"这一事实对当前 turn 的
可见性。

---

## 6. 改进建议

总原则：**每个 turn 交给模型的应该是一份最小的、结构化的"世界增量"，加上指向
事实图的指针；静态规则留在契约层；历史留在 session 与可查询接口里。**

### 6.1 定义 Turn Input Contract（turn envelope v1）

把 agent-runtime-awareness 里"由后续 prompt 设计单独定义"的那部分补上，作为
正式协议文档。三层结构：

1. **契约层**（已有，保持）：`AGENTS.md` + skill + guide，承载全部操作规则；
2. **turn header（新）**：一段有 schema 的紧凑结构（建议围栏 JSON 块），只放
   本次唤醒的投递事实：

   ```json
   {
     "turn": {"runId": "...", "firstTurnInScope": false},
     "wake": [
       {
         "kind": "message",            // message | event | reminder | assignment
         "intent": "request_action",   // chat | request_action | status_update
         "id": "msg_123",
         "from": {"id": "actor_human_x", "name": "canfuu", "kind": "human"},
         "at": "2026-07-07T00:20:00Z",
         "scope": "thread:th_z",
         "deliveredBecause": "mention", // mention | reply | dm | task_owner | ...
         "visibility": {"privateTo": ["actor_agent_y"]}
       }
     ],
     "unreadGap": {"count": 3, "hint": "loom message read --target ..."}
   }
   ```

3. **正文层**：每条唤醒消息的原文放独立围栏块（如
   ` ```loom-message id=msg_123 ... ``` `），**不做 mention 改写**（显示名映射
   放 header），围栏内容转义，杜绝伪造投递头。

结构化后，reminder、身份重申、时区教学都可以退出每回合注入（见 6.4）。

### 6.2 唤醒合并（coalescing）——最优先的行为修复

- `finish_and_next` 从"弹一条"改为"**弹出该 turn_key 当前积压的全部触发**"，
  合并为一个 turn：header 的 `wake[]` 按到达顺序列出全部消息，模型一次性回应；
  turn Finished 时 ack 全部对应 delivery。
- dispatch 前增加可配置 debounce 窗口（如 500–1000ms，spec 字段
  `wake.debounceMs`），吸收人类分段打字。
- 合并天然消除 §3.4 的重复回答问题，也让"回放里已包含队列后续消息"的怪象消失。
- 优先级语义保留：合并批次内人类消息排前；service 回调可限定每批最多 N 条。

### 6.3 用 delivery cursor 取代"最近 20 条"

- resume session 的 turn：不再整段回放，改为"自上一 turn ack 以来本 scope 的
  新消息"（即 `wake[]` + 同期非唤醒消息，各带时间戳）；有更早未读时在
  `unreadGap` 声明数量与查询命令，而不是静默截断。
- 仅在 session 新建/重置（first turn、signature 变更、provider session 丢失）
  时做一次有限的 bootstrap 回放（这时"最近 N 条"才是对的工具）。
- 频道根消息触发时，明确告知"频道主干上下文未注入，用
  `loom message read --target '#<channel>'` 查询"，消除 §2.4 的隐性空上下文。

### 6.4 reminder 降频外置

- 操作规则的唯一权威位置是 `AGENTS.md`（已有几乎全部内容）；
- turn header 保留一行指针（如 `"replyContract": "see AGENTS.md#loom-operating-rules"`）；
- 为弱模型兜底做成 spec 可配置：`reminder: firstTurn | everyTurn | off`，默认
  `firstTurn`——现有 trigger prefix 机制（first_turn/every_turn）可直接复用，
  实现成本低。

### 6.5 intent / deliveredBecause 显式化

把 server 已有的 `intent`、`deliveryPolicy`、投递原因带进 header（6.1 已含）。
这使得：`status_update` 类唤醒可以被模型（甚至被 runtime 在 dispatch 前）低成本
判定为 `run ignore`；"为什么是我被唤醒"不再需要从 Delivery: 一行英文里猜。

### 6.6 统一并发键（缺陷修复，可独立先行）

短期：`turn_key` 回退为 per-scope（channel 根消息共用 channel 键），与
`active_turns`/adapter/session 的粒度一致，消除覆盖与 in-flight 冲突。
长期：若确要 thread 族并发，则 `active_turns`、adapter in-flight、provider
session 目录全部改以 `turn_key` 为键，并为 channel scope 定义独立 session。

### 6.7 中途打断/注入策略

spec 增加 `onHumanMessageWhileBusy: queue | cancel_and_requeue | inject`：

- `queue`：现状（默认）；
- `cancel_and_requeue`：高优触发（人类新消息）到达时取消当前 turn，将其触发
  与新消息合并重新 dispatch（run trace 保留已完成部分）；适合 command
  transport；
- `inject`：对支持 stdin 持续输入的 interactive provider，把新消息作为后续
  输入注入当前进程（远期）。

### 6.8 预算与观测

- `prompt_breakdown`（已有遥测）增加"重复注入率"：本 turn 与该 scope session
  已注入内容的重叠字节数/占比，GUI Run 面板展示趋势；
- 给回放/上下文段设 token 预算上限（spec 可配），超限时收缩为 gap 声明。

### 6.9 落地顺序建议

| 阶段 | 内容 | 性质 | 状态 |
| --- | --- | --- | --- |
| P0 | 6.6 并发键统一；6.4 reminder 降频（复用 trigger prefix 机制） | 缺陷修复 / 低风险 | ✅ 已完成（2026-07-07） |
| P1 | 6.2 唤醒合并 + debounce；6.5 intent 显式化 | 行为改进，兼容现有 prompt 形态 | ✅ 已完成（2026-07-07） |
| P2 | 6.1 Turn Input Contract v1 + 6.3 delivery cursor（一起定稿、一起换） | 协议级变更，需要新文档 + provider 回归 | ⏳ 未开始 |
| P3 | 6.7 打断/注入；6.8 观测预算 | 增强 | ⏳ 未开始 |

---

## 7. 实施状态（2026-07-07，P0 + P1 已落地）

分支 `optimize-agent-runtime-awareness`，改动文件：
`crates/proto/src/methods.rs`、`crates/cli/src/cmd/agent_serve.rs`、
`crates/cli/src/cmd/daemon.rs`、`docs/ISSUES.md`（#7）。
测试：loom-cli 391/391、loom-server 123/123、proto 19/19 通过；
agent-runtime 有 4 个预先存在的 Windows 环境失败（测试依赖 `/bin/sh`，与本次改动无关）。

已实现内容：

1. **并发键统一（6.6）**：`turn_key_for_trigger` 统一为
   `scope:{kind}:{id}`（`turn_key_for_scope`），删除旧的 thread-root 族键与
   dm-target 键。消除同频道并发根消息的 ActiveTurn 覆盖 / "session already
   in flight" / busy 键永久泄漏（详见 ISSUES #7）；DM 也从"全部串行"修正为
   按对端（per DM channel scope）并行。
2. **唤醒合并（6.2）**：`WorkerState::finish_and_next_batch` +
   `pop_batch_locked` + `drain_coalescible_into`；`dispatch_trigger_batch`
   支持批量 turn。合并规则 `triggers_coalesce`：仅普通消息（无
   taskId/assignmentId/kind 元数据、无 triggerPromptPrefix）、同 reply
   target、同 privateTo 集合；上限 `WAKE_COALESCE_MAX = 10`。
   `ActiveTurn.trigger_source_ids` 记录整批来源，Finished/失败路径逐条 ack
   delivery；`run.open` metadata 带 `coalescedSourceIds`。批量渲染
   `render_batch_prompt_with_names`（`=== Latest Loom messages (N, oldest
   first) ===`，逐条 From/Message id/Delivery/Intent，共享一段 reminder）。
3. **spec 策略（proto）**：`AgentSpec.wake: Option<WakeSpec>` —
   `coalesce`（默认 true）、`debounceMs`（默认 0，上限 10s，仅对可合并消息
   生效）、`replyReminder`（`every-turn | first-turn | off`，默认
   `first-turn`）。
4. **reminder 降频（6.4）**：`ReminderRender::Full/Pointer/Skip`，由
   `reminder_render_for_turn(spec, first_turn)` 决定；默认首回合（per scope
   per worker 进程）注入完整 `Response delivery reminder`，后续回合只保留
   单行 `Reply contract: ...`（指向 AGENTS.md）。
5. **intent 显式化（6.5）**：消息触发的 prompt 头新增
   `Intent: chat|ask|request_action|assign_task|status_update|review|notify`
   行（单条与批量渲染均含）。

新增测试（crates/cli/src/cmd/agent_serve.rs tests 模块）：
`turn_key_matches_execution_scope`、
`turn_key_for_event_matches_message_key_in_same_scope`、
`turn_key_for_dm_message_follows_direct_channel_scope`、
`finish_and_next_batch_coalesces_consecutive_compatible_messages`、
`triggers_coalesce_requires_same_target_and_visibility`、
`reply_reminder_defaults_to_full_on_first_turn_then_pointer`、
`latest_prompt_reminder_modes_render_expected_text`、
`latest_prompt_includes_message_intent`、
`batch_prompt_lists_messages_oldest_first_with_one_reminder`。

后续待做（P2/P3，未开始）：

- **P2 / 6.1 Turn Input Contract v1**：结构化 turn header（围栏 JSON：
  turn/wake[]/unreadGap）、消息正文进转义围栏块防伪造 `=== ... ===` 头、
  取消正文 mention 改写（映射进 header）；需要新协议文档定稿。
- **P2 / 6.3 delivery cursor**：resume session 的回合用"自上次 ack 以来的
  新投递"替代固定最近 20 条回放；声明 unread gap；频道根消息触发时显式提示
  频道主干上下文需 CLI 查询。
- **P3 / 6.7**：`onHumanMessageWhileBusy: queue|cancel_and_requeue|inject`。
- **P3 / 6.8**：prompt 重复注入率遥测 + 回放段 token 预算上限。
- 收尾项：GUI Agent 编辑器暴露 `wake` 字段；`examples/agents` 文档补充
  `wake` 示例；`docs/protocol/agent-coordination-workflow.md` 中依赖
  "每条消息一个 turn"假设的措辞复查。

---

## 附录 A：关键代码索引（2026-07-07 时点）

| 行为 | 位置 |
| --- | --- |
| GUI 组装 message.send | `apps/gui-web/src/App.tsx` `sendMessage`（~1555）、`sendThreadMessage`、`sendDirectMessage` |
| 投递收件人计算 | `crates/server/src/store.rs` `message_delivery_recipients`（~4256）、`thread_attention_recipients`（~4317） |
| wake_agent 无目标拒绝 | `crates/server/src/store.rs` ~3591 |
| 实时接收循环 | `crates/cli/src/cmd/agent_serve.rs` `notification_loop`（~3233） |
| durable inbox 轮询（15s / 6h 上限） | 同上 `drain_pending_inbox`（~3905）、`pending_inbox_max_age`（~4041） |
| 唤醒判定 | 同上 `is_message_for_us_with_delivery`（~3596） |
| turn_key 规则 | 同上 `turn_key_for_message`（~2181） |
| 排队/优先级/串行门 | 同上 `begin_or_enqueue`（~2558）、`enqueue_locked`（~2526）、`is_priority_trigger`（~2815） |
| turn 完成 → 弹下一条 | 同上 `AdapterEvent::Finished` 分支（~6535）、`finish_and_next`（~2582） |
| turn 派发 | 同上 `dispatch_trigger`（~4341） |
| Latest message 头 + reminder | 同上 `render_trigger_prompt_with_names`（~5389） |
| 最近 20 条回放 | 同上 `recent_conversation_context`（~5115）、`format_recent_conversation_context`（~5295） |
| envelope 组装 | 同上 `compose_envelope_prompt`（~5830）；`crates/agent-runtime/src/envelope.rs` |
| assignment 上下文 | 同上 `assignment_context_for_prompt`（~5578） |
| AGENTS.md 生成 | `crates/agent-runtime/src/agents_md.rs` `loom_block`（~85） |
| per-scope session / in-flight | `crates/agent-runtime/src/command.rs` `RunSlotGuard`（~79）；`crates/agent-runtime/src/interactive.rs` `run_prompt_inner`（~322） |
| active turn 覆盖点 | `crates/cli/src/cmd/agent_serve.rs` `WorkerState::set_turn`（~2481），`active_turns` 注释（~2246） |
| 最终文本默认不自动发布 | 同上 `agent_text_auto_publish_enabled`（~6880） |

## 附录 B：关联文档

- `docs/protocol/agent-runtime-awareness.md` — 运行规则外置与 prompt 边界
  （本文 6.1 是它"另行设计"部分的提案）。
- `docs/protocol/agent-coordination-workflow.md` — claim / rebase / no-reply
  契约（6.2 合并与 6.5 intent 显式化可显著降低其对模型自觉性的依赖）。
- `docs/protocol/open-multi-actor-collaboration-protocol-v0.md` — Delivery /
  Receipt / Turn 模型（6.3 的事实基础）。
- `docs/ISSUES.md` #5 — instructions 注入膨胀的历史修复（本文 5.2 是其
  turn 级别的延续问题）。
