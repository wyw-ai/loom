# delivery — bundle

Thread-scoped executor. Consumes the lesson plan + clone manifest,
makes code changes inside provisioned repos, opens an MR, and emits
validation evidence for the teacher.

- Consumes: `task-goal.json`, `definition-of-done.json`,
  `lesson-plan.md`, `clone-manifest.json`, streaming
  `mr-event-*.json` (from the thread-bound `mr-detector`).
- Produces: code changes, MR descriptor artifacts, validation evidence
  artifacts.
- Hands off to: `teacher` (validation phase). Falls back to `router`
  on hard blockers.

Provider: `claude` via `interactive_command`. Standard envelope with
`maxTurnMs = 3600000` to accommodate longer code-edit turns. The
`preFlight` gate described in design §5.1 (require
`.joi/state/scope.json`) is intentionally **not** declared here yet —
the `AgentSpec` field is a forward-compat hook that lands with p4c.
