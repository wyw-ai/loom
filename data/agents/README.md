# `data/agents/` — AgentSpec library

Drop-in `AgentSpec` definitions registered with `joi agent register`.
Each subdirectory ships:

- `spec.json` — `AgentSpec` JSON. Parses cleanly via
  `serde_json::from_str::<AgentSpec>(...)` and round-trips through
  `joi agent register` (see `Verifying`, below).
- `bundle/SKILL.md` — the actor's skill body. Loaded by the agent on
  first turn via `firstTurnPrefix` (`Read your skill from
  {agent.bundle}/SKILL.md.`).
- `bundle/README.md` — operator notes (handoff contract, artifacts
  produced/consumed, prompt template variables).

Inventory shipped in Phase 3:

| Actor               | Provider | Slash (handoff prefix) | Scope    | Produces |
| --- | --- | --- | --- | --- |
| `classmaster`       | claude   | `/classmaster`         | channel  | `task-goal.json`, `definition-of-done.json` |
| `teacher`           | claude   | `/teacher`             | channel/thread | `lesson-plan.md`, `validation-report.json` |
| `lesson-designer`   | claude   | `/lesson-designer`     | channel  | `lesson-plan.md` |
| `router`            | copilot  | `/router`              | channel  | dispatches to discovery/teacher/delivery |
| `discovery`         | claude   | `/discovery`           | channel/thread | `clone-manifest.json`, repo notes |
| `delivery`          | claude   | `/delivery`            | thread   | code changes, MR, `validation-report.json` |
| `feedback-fix-orchestrator` | claude | `/feedback-fix-orchestrator` | channel/thread | dispatches feedback fixes |

All artifacts above conform to `docs/artifact-contracts.md`.

## Conventions

- `actor.id` uses the `actor_<kebab-with-underscore>` convention from
  the existing fleet; `display_name` is human-friendly.
- `transport.kind = "interactive_command"`. `provider.kind` selects
  `claude` or `copilot`; the runtime appends `--model=<id>` from the
  spec's `transport.model` if present.
- `prompt_template.everyTurnPrefix` carries the standard `[joi handoff
  v1]` envelope (`docs/remove-dev-helper-migration-design.md` §5).
  `firstTurnPrefix` carries the `[joi bootstrap]` instructions.
- `handoff.triggerPromptPrefix` injects the slash command on every
  inbox dispatch so the underlying provider activates the right skill
  (design §5.0).
- `bundle.source` is a workspace-relative path. The agent host copies
  the bundle into `{agent.root}/bundles/<version>/` on register.

## Verifying

Each spec must parse as a valid `AgentSpec`. Smoke check:

```sh
JOI_AGENT_SPECS=$(mktemp -d) cargo run -q --bin joi -- \
    agent register data/agents/classmaster/spec.json
```

A clean exit with `registered actor_classmaster at …` means the JSON
is structurally valid. The same `JOI_AGENT_SPECS` dir can be passed
to `joi agent serve --specs <dir>` for runtime smoke (Phase 5).
