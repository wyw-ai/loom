# feedback-fix-orchestrator —— bundle

频道级监督者。负责把最近的产品反馈拢成队列，按条派发给 `delivery`
（每条一个独立 thread）。本身不写代码。

- 消费：用户触发、频道里配置的反馈源，以及通过 handoff 总线回流的各 thread
  MR 事件。
- 产出：每条反馈一次 `delivery` handoff、频道级状态汇总，必要时一份
  `feedback-batch.json` artifact 用作统计。
- Handoff 去向：`delivery`（按条，独立 thread）；模糊条目退回 `router`。

Provider：`claude`，走 `interactive_command`，envelope 同其它 actor。
