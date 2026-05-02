# classmaster —— bundle

频道（channel）级编排 agent。负责每次会话里的 **任务定型** 阶段：

- 消费：用户的自由文本请求。
- 产出：`task-goal.json`、`definition-of-done.json` artifact
  （`docs/artifact-contracts.md` §1、§2）。
- Handoff 去向：`router` / `discovery` / `teacher` / `delivery` —— 被叫方的身份
  通过它们各自 `handoff.triggerPromptPrefix` 在 runtime 侧注入，所以 bundle 里
  不写死任何 callee 名字。

Provider：`claude`，走 `interactive_command` transport（见 `spec.json`）。
完成契约：`__JOI_DONE__` 哨兵。

每轮注入到 skill 的 prompt envelope 变量：

| 变量 | 含义 |
| --- | --- |
| `{scope.kind}` / `{scope.id}` | 触发事件所在的 scope |
| `{actor.id}` | 当前 actor（`actor_classmaster`） |
| `{trigger.actor_id}` / `{trigger.id}` | 上游 handoff 来源 |
| `{channel.id}` / `{thread.id}` | 解析后的 scope 地址 |
| `{workspace.dir}` | 可写工作区根目录 |
| `{scope.skills}` | scope 视角下投影出的 skill 文件 |
| `{agent.bundle}` | 当前 bundle 在 agent host 上的安装根 |
