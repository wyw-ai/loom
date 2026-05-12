# lesson-designer —— bundle

单阶段 agent。响应 teacher 的请求，起草或修订 `lesson-plan.md` artifact
（`docs/artifact-contracts.md` §4）。

- 消费：`task-goal.json`、`definition-of-done.json`，以及可选的旧版
  `lesson-plan.md`、`validation-report.json`。
- 产出：新的 `lesson-plan.md` artifact。
- Handoff 去向：`teacher`（绝不直连执行者）。

Provider：`claude`，走 `interactive_command`，envelope 同其它 actor。
