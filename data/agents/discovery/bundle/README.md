# discovery — bundle

Channel/thread-bootstrap agent. Decides the repo set for a task and
publishes a `clone-manifest.json` (`docs/artifact-contracts.md` §3).

- Consumes: `task-goal.json`, `definition-of-done.json`, optional
  channel repo-cache mirrors.
- Produces: `clone-manifest.json`.
- Hands off to: `delivery` (typical) or back to `router`.

Provider: `claude` via `interactive_command`. Standard envelope.
