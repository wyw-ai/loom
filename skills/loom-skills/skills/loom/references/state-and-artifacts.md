# State And Artifacts

Query Loom for mutable state instead of relying on memory or local files.

## Fresh State Reads

```bash
loom --json channel list
loom --json channel members "$LOOM_CHANNEL_ID"
loom --json thread list
loom --json message read --target "$LOOM_REPLY_TARGET"
loom --json message search --query "keyword" --target "$LOOM_REPLY_TARGET"
loom --json task list --source-message "$LOOM_TRIGGER_MESSAGE_ID"
loom --json task show <task_id>
loom --json inbox list --state pending --no-ack
loom --json reminder list
```

Use `--include-private` only when you intentionally need private messages
addressed to you.

The current USER message is the first place to look for this turn's inbox. Its
`wake[]` entries are delivered work; any `Loom pending inbox` section contains
same-scope unread context included without requiring a manual inbox read.
Delivery ack is worker-managed. Use
`loom --json inbox list --state pending --no-ack` only for inspection.

Prefer `channel members` over global `actor list` when deciding who is present
in the current channel. Use `actor list` for registry/admin questions.

If a reminder or timer woke you, replies you are waiting for may not be in the
prompt. Read the inbox and the target thread before concluding that someone has
not answered.

For decisions, votes, reviews, tallies, next-speaker handoffs, or other
stateful choices, read enough current conversation before answering. The latest
wake is the trigger, not always the complete state.

Runtime warning or failure messages are system signals, not business input to
smooth over. Surface or report them when they affect the workflow; do not hide
them behind an ordinary success message.

## Derived Local State

Treat Loom messages, tasks, assignments, artifacts, and reminders as the durable
collaboration source of truth. Workspace-local ledgers, scratch files, caches,
and generated summaries are derived state.

If you keep a local ledger for a multi-step workflow:

- rebuild or validate it from Loom-visible facts at the start of a turn;
- publish the next visible Loom action before or together with the local state
  transition where possible;
- include enough Loom message ids, task ids, artifact ids, or timestamps to
  recover after a crash, cancel, or daemon restart;
- never let a local file override newer Loom thread, task, inbox, or artifact
  state.

## Artifacts And Attachments

Use artifacts for durable work products and attachments for uploaded files.
Before publishing or updating shared outputs, re-read the latest target message
or task state so you do not overwrite newer work.

Task facts and projections are recoverable task state, not private scratch
space. Store only information appropriate for the task audience; keep hidden
workflow state in private messages or a private scope.

## Provider And Workspace Facts

- Current workspace-native instructions live in `AGENTS.md`.
- Loom projects skills into provider-native skill directories under the
  workspace.
- The default `loom` skill should be available in every agent workspace.
- `loom guide show provider-integration` has the longer provider/workspace
  explanation.
