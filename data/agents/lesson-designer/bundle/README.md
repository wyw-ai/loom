# lesson-designer — bundle

Single-phase agent. Drafts or revises a `lesson-plan.md` artifact
(`docs/artifact-contracts.md` §4) in response to a teacher request.

- Consumes: `task-goal.json`, `definition-of-done.json`, optional prior
  `lesson-plan.md` and `validation-report.json`.
- Produces: a new `lesson-plan.md` artifact.
- Hands off to: `teacher` (never directly to executors).

Provider: `claude` via `interactive_command`. Standard envelope.
