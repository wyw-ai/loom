# Agent Coordination Workflow

本协议定义 Loom/Joi 多 agent 在 channel / thread / task 上协作时的最小硬约束。
目标不是把聊天系统改成 Git，而是借用 Git 的两个关键思想：

- 开工前先获得 work owner。
- 发送前确认自己基于最新上下文。

一句话版本：

> 开工前 claim，发送前 rebase。

## 1. 核心模型

| 协作概念 | Loom/Joi 原语 | 语义 |
| --- | --- | --- |
| 主干事实流 | channel | 只承载顶层请求、短答和必要索引 |
| 工作锚点 | 顶层 channel message | task 的唯一 source message |
| 工作上下文 | canonical thread | 计划、进度、证据、review、交付都回到这里 |
| 工作合同 | task | 记录状态、owner、artifact、assignment 和验收结果 |
| owner CAS | task claim | 决定谁可以执行该工作项 |
| 提交前同步 | send-time rebase | 发可见消息前确认上下文未过期 |

Thread 不是 owner。进入 thread、订阅 thread、或在 thread 里发言，都不能被解释为
“我拥有这个任务”。唯一 owner 来源是 task 的 claim / owner 字段。

## 2. 顶层工作请求

当 agent 收到顶层 channel / DM 消息时，先判断它是简单回复还是工作项。

可以直接回复的情况：

- 简单翻译、解释、澄清。
- 不需要工具、调研、写文件、artifact、长时间跟进。
- 不会与其他 agent 对同一请求并行产出。

必须 claim 的情况：

- 需要运行工具、查外部状态、改文件、产出 artifact。
- 需要多轮推进、review、验证或等待回调。
- 需要派发给其他 actor。
- 用户在 channel 里发的是一个可交付工作请求。

工作项流程：

```text
1. claim 顶层 source message。
2. claim 成功后，只在 canonical thread 推进实质工作。
3. claim 失败后，停止执行；最多在 owner thread 里做被请求的 review / 补充。
4. 完成后在 canonical thread 交付 summary / artifact。
5. 更新 task 状态为 waiting_review / done / failed / canceled。
```

## 3. Claim 语义

Claim 是 owner CAS，必须是原子的。

Claim 成功条件：

- source message 尚无 task：创建 task，owner = 当前 actor，status = claimed。
- task 已存在但没有 owner，且状态非终态：设置 owner = 当前 actor，status = claimed。
- task 已由当前 actor 拥有：幂等成功。

Claim 失败条件：

- task 已由其他 actor 拥有。
- task 已经 done / failed / canceled。
- source message 不是顶层 channel message。
- 当前 actor 无权访问该 channel。

Claim 失败后的规则：

- 不继续做同一工作。
- 不在 channel 里输出替代交付物。
- 不把 owner 改成自己。
- 如确有价值，只能在 owner 的 canonical thread 里发 review / 补充，且不能抢最终结论。

## 4. Canonical Thread

每个 task 都有一个 canonical thread，target 形式为：

```text
#<channel_id>:<source_message_id>
```

规则：

- channel 顶层只保留短答、任务入口和必要总结。
- 计划、进度、证据、artifact、review、最终结论都写入 canonical thread。
- 派发 assignment 时也回到 canonical thread。
- 不要为了“进入 thread”给自己 handoff。
- 不要为同一个 root message 创建替代 thread。

## 5. 发送前 Rebase

自然语言历史只能告诉 agent “曾经发生过什么”，不能保证 agent 准备发送时仍基于最新
上下文。因此所有可见消息在发送前都必须做 send-time rebase。

Rebase 流程：

```text
1. 读取目标 channel / thread 的最新消息。
2. 把最新 message id 作为 base。
3. 根据最新内容调整待发送内容。
4. 发送时携带 ifLatestMessageId = base。
5. 如果 server 拒绝，说明上下文已变；重新读取并回到第 3 步。
```

内容调整规则：

- 最新消息已经覆盖自己的内容：不发送，或只发送很短的补充。
- 最新消息部分覆盖：只发送 delta。
- 最新消息与自己的结论冲突：改为 review/comment，说明差异和依据。
- 最新消息来自用户且改变任务：放弃旧回复，按新指令重新执行。
- 自己不是 task owner：不能输出替代性交付物。

## 6. 子任务与并行

父任务 owner 可以拆 child task，但必须保留清晰归属：

- child task 引用 `parentTaskId` 或 `parentSourceMessageId`。
- 每个 child task 独立 claim。
- parent owner 负责整合 child 结果。
- child owner 不直接在 channel 主干争夺最终结论。
- 代码类 child task 必须明确 write set；必要时使用 workspace lease。

Task claim 只保证“同一工作项 owner 唯一”，不保证不同 task 不改同一资源。
共享资源写入仍依赖 assignment contract / workspace lease / git merge 检测。

## 7. Review 与非 Owner 发言

非 owner 可以发言的条件：

- 被 owner 明确 @mention。
- 收到 review / verify / investigate assignment。
- 人类明确要求其补充。
- 发现会导致明显错误的事实，需要在 thread 中指出。

非 owner 不应：

- 接管任务。
- 改写最终交付。
- 在 channel 输出另一个完整答案。
- 把 task owner 改成自己。

Review 结果必须回到 task timeline：thread 消息只是人类可读记录，assignment result、
artifact link、fact 或 task status 才是机器可读状态。

## 8. 超时与接管

Phase 0 可由人类或协调者手动处理：

- 询问 owner。
- cancel / reopen / reassign task。
- 创建后续 child task 接续。

未来可以把 claim 扩展为 lease：

```text
claim(owner, expires_at)
heartbeat(owner, task)
timeout -> reclaim allowed
```

没有 lease 前，其他 agent 不能因为 owner 沉默就自行抢占。

## 9. 工具契约

CLI/RPC 必须支持这两个硬约束：

```bash
# 开工前 claim：可直接按 source message claim
loom --json task claim --source-message "$LOOM_TRIGGER_MESSAGE_ID"

# 或 claim 已知 task
loom --json task claim <task_id>

# 发送前 rebase：先读最新，再携带 if-latest 条件发送
latest=$(loom --json message read --target "#$LOOM_CHANNEL_ID:$LOOM_TRIGGER_MESSAGE_ID" --limit 1)
loom --json message send \
  --target "#$LOOM_CHANNEL_ID:$LOOM_TRIGGER_MESSAGE_ID" \
  --if-latest "<latest_message_id>" \
  --text "..."
```

工具层必须保证：

- `task claim` 不覆盖其他 owner。
- `message send --if-latest` 在目标 scope 最新消息不匹配时拒绝写入。
- 拒绝结果必须是显式 conflict，调用方需要 re-read / rebase / retry 或 skip。

## 10. Prompt 契约

所有 agent prompt / AGENTS.md 必须表达同一规则：

```text
开工前 claim，发送前 rebase。

如果顶层消息是工作项，必须先 claim source message。claim 失败就停止，不重复做。
claim 成功后，只在 canonical thread 推进。

发送任何额外可见消息前，先读取目标最新消息；如果内容已经被覆盖，跳过或只补充
delta；如果仍需发送，携带 if-latest 条件。失败后重新读取并调整，不盲目重发。
```

Prompt 不能只建议“礼貌协作”，必须明确工具命令和失败后的停止条件。

## 11. 分阶段落地

Phase 0：

- 更新 prompt / AGENTS.md。
- 更新 CLI 文档和 examples。
- 要求 agent 按 source message claim。
- 要求显式 `--if-latest` 发送额外 thread/channel 消息。

Phase 1：

- server 对 `task claim` 做 owner CAS。
- server 对 `message send --if-latest` 做 latest-message CAS。
- agent runtime 在自动发布最终回复前尽量带上 base context 或进行 stale 检查。

Phase 2：

- 引入 claim lease / heartbeat / timeout reclaim。
- 引入更明确的 path/resource write intent。
- 对未 claim 的 channel 工作型回复做 soft enforcement。

Phase 3：

- 如需要审计、回放和跨机器恢复，再考虑 Git-like DAG substrate。
  这不是解决弱协商的第一步；弱协商首先靠 claim + rebase 解决。
