# classmaster — bundle

Channel-level orchestrator agent. Owns the **task framing** stage of
each conversation:

- Consumes: the human's free-form request.
- Produces: `task-goal.json` and `definition-of-done.json` artifacts
  (`docs/artifact-contracts.md` §1, §2).
- Hands off to: `router` / `discovery` / `teacher` / `delivery` —
  callee identity is injected by the runtime from each callee's
  `handoff.triggerPromptPrefix`, so this bundle doesn't hardcode it.

Provider: `claude` via `interactive_command` transport (see
`spec.json`). Completion contract: `__JOI_DONE__` sentinel.

Prompt envelope variables surfaced to the skill (every turn):

| Variable | Meaning |
| --- | --- |
| `{scope.kind}` / `{scope.id}` | the scope the trigger event lives in |
| `{actor.id}` | this actor (`actor_classmaster`) |
| `{trigger.actor_id}` / `{trigger.id}` | the upstream handoff |
| `{channel.id}` / `{thread.id}` | resolved scope address |
| `{workspace.dir}` | mutable working state root |
| `{scope.skills}` | scope-projected skill files |
| `{agent.bundle}` | this bundle's installed root |
