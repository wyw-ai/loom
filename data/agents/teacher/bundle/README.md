# teacher —— bundle

actor 训练工程师：把 classmaster 归档的真实 actor 缺陷转成候选 bundle、
回归作业、评分报告和训练记录。

- 消费：`actor-defect.v1`、`training-plan.v1`、`definition-of-done.json`。
- 产出：`homework.v1`、`grading-report.v1`、`lesson-plan.md`
  （含 `spec_apply`）、`training-record.v1`。
- Handoff 去向：训练完成或需要决策时回 `actor_classmaster`。

Provider：Codex CLI（`codex-joi` wrapper），走 `interactive_command`。
Envelope 同 `classmaster`（参考 `data/agents/README.md`）；`maxTurnMs = 1800000`，
因为验证轮可能通过工具收集证据。
