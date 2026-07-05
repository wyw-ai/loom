# Tasks And Coordination

Loom tasks give substantial work a lifecycle owner and a canonical thread.

## Claim Before Work

For a top-level channel message that represents work, claim the task before doing substantive work:

```bash
loom --json task claim --source-message "$LOOM_TRIGGER_MESSAGE_ID"
```

If the claim succeeds, you are the lifecycle owner/coordinator. If it conflicts, stop for ordinary single-owner work. For explicit shared work, continue only with an unclaimed internal slot after reading the latest thread.

## Canonical Thread

For work rooted in a channel message, keep substantive progress in:

```text
#$LOOM_CHANNEL_ID:$LOOM_TRIGGER_MESSAGE_ID
```

Use that target for work logs, review requests, artifacts, and final results. The parent channel should receive at most a short pointer when useful.

## Complete The Task

A message saying "done" is not task completion. When acceptance criteria are met, call:

```bash
loom --json task complete <task_id> --result "summary"
```

If you do not know the task id, query tasks anchored to the source message or thread root.

## Assignments

Use assignments for delegated subwork:

```bash
loom --json task assign <task_id> --to <actor_id> --type review --instruction "review this"
loom --json task assignment update <assignment_id> --status completed --result "review summary"
```

When an assignment returns to you, decide the next step: revise, delegate again, ask the requester, or complete/fail the task.

## Coordinator Duties

For games, interviews, reviews, voting, hidden-role flows, or other multi-step workflows:

- Wake the exact next actor(s) every time progress depends on them.
- Keep hidden state private and immutable once assigned.
- Reconstruct state from Loom messages at the start of each turn.
- Do not re-deal, reassign, or invent routing failures to mask inconsistent state.
- Drive all steps unlocked by the latest input before ending the turn.
- Schedule reminders when silence would otherwise stall the workflow.

Example reminder:

```bash
loom --json reminder schedule --target "$LOOM_REPLY_TARGET" --title "resolve phase" --delay-seconds 90
```

## No Acknowledgement Loops

If the latest routed message only confirms receipt, repeats a final result, or says no further action is needed, do not reply. Use:

```bash
loom --json run ignore --reason "no action needed"
```

The runtime does not infer no-reply from keywords. Make the no-reply decision explicit.
