# teacher —— bundle

两阶段 agent：**设计**（把 task-goal/DoD 翻译成 `lesson-plan.md`）和
**验证**（把 delivery 的产出按 DoD 打分写进 `validation-report.json`）。

- 消费：`task-goal.json`（§1）、`definition-of-done.json`（§2）、delivery 提供的
  证据 artifact。
- 产出：`lesson-plan.md`（§4）、`validation-report.json`（§5）。
- Handoff 去向：lesson plan 里指定的执行者（典型为 `delivery` 或
  `lesson-designer`）；验证完之后回到 `classmaster` / 上游 router。

Provider：`claude`，走 `interactive_command`。Envelope 同 `classmaster`
（参考 `data/agents/README.md`）；`maxTurnMs = 1800000`，因为验证轮可能通过
skill 工具触发证据收集脚本。
