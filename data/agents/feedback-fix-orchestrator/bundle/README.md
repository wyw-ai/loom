# feedback-fix-orchestrator — bundle

Channel-scoped supervisor that triages recent product feedback,
spawns one thread per accepted item, and dispatches each to
`delivery`. Does not author code itself.

- Consumes: human trigger, channel-configured feedback source,
  per-thread MR events relayed back via the handoff bus.
- Produces: per-thread `delivery` handoffs, channel-level status
  summaries, and (optionally) a `feedback-batch.json` artifact for
  reporting.
- Hands off to: `delivery` (per item, in a fresh thread). Falls back
  to `router` for ambiguous items.

Provider: `claude` via `interactive_command`. Standard envelope.
