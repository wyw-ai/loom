# router —— bundle

频道级派发器。接到一次 trigger，决定下一个 actor，然后 handoff。本身基本无状态，
最多读一下频道上已有的 artifact。

- 消费：trigger event + 频道上已有的 artifact。
- 产出：单次 `joi event append --handoff <actor_id>`（或者对琐碎问题直接回复）。
- 不产出业务 artifact。

Provider：`claude`，走 `interactive_command`，envelope 同其它 actor。
