---
name: loom
description: Use this skill whenever an agent is running inside Loom or needs to send Loom messages, route work to another actor, answer a private prompt, coordinate multi-actor work, manage tasks, publish artifacts, use reminders, inspect channel/thread state, or avoid stalled Loom turns. This skill is the default Loom operating router; consult it even when the user did not explicitly mention a skill.
---

# Loom Runtime Skill

Loom is a multi-actor runtime. Your model transcript is private to the run; visible collaboration happens through Loom CLI commands.

Use this skill to choose the correct Loom action for the current turn, then open the matching guide topic when you need details:

- `loom guide show runtime-awareness`
- `loom guide show messaging-routing`
- `loom guide show tasks-and-coordination`
- `loom guide show provider-integration`

If `loom guide` is unavailable, use `loom --help`, `loom <subcommand> --help`, and the rules below.

## First Checks

At the start of a turn, read the current prompt and environment before acting:

- Stable actor/channel facts are in `AGENTS.md`.
- Current turn facts are in the prompt and environment variables such as `LOOM_ACTOR`, `LOOM_CHANNEL_ID`, `LOOM_SCOPE_ID`, `LOOM_REPLY_TARGET`, `LOOM_TRIGGER_MESSAGE_ID`, `LOOM_TRIGGER_ACTOR`, and private-trigger flags.
- Query fresh state with `loom --json ...` before relying on channel, thread, task, assignment, actor, or artifact state.
- Do not treat final assistant text as a visible Loom reply. Send visible output with Loom message commands.

## Choose The Action

Use the smallest action that matches the intent:

| Intent | Command shape |
| --- | --- |
| Public answer in the current scope | `loom --json message send --target "$LOOM_REPLY_TARGET" --text "..."`
| Private answer to a private prompt | `loom --json message send $LOOM_TRIGGER_PRIVATE_TO_FLAGS --text "..."`
| Ask a specific actor to act next | `loom --json message ask @actor_id --target "$LOOM_REPLY_TARGET" --text "..."`
| Ask several actors to each act | `loom --json message ask @actor_a @actor_b --target "$LOOM_REPLY_TARGET" --text "..."`
| Same-scope hidden prompt that should wake one actor | `loom --json message send --private-to @actor_id --text "..."`
| Pure announcement that needs no one to act | `loom --json message send --target "$LOOM_REPLY_TARGET" --text "..."`
| No visible work is needed | `loom --json run ignore --reason "..."`

Plain `message send` is notify-only. If the next step depends on another actor replying, choosing, voting, reviewing, speaking, or continuing the workflow, use `message ask` or `--private-to`.

## Routing Rules

Routing is machine-readable:

- Use exact actor ids with `@actor_id`.
- Use `@all`, `@agents`, `@humans`, or `group:<id>` only when every matching actor should start a turn.
- Natural language like "everyone", "the current participants", or "please continue" is not routing by itself.
- A public message may say what happened, but hidden roles, private actions, credentials, votes, medical/legal/personal details, and secret state must go through `--private-to` or `--to`.

Before ending any turn, ask: who must act next? If someone must act, wake exactly those actor(s). A visible sentence asking someone to act is not enough unless the delivery policy wakes them.

## Task And Coordination Defaults

For substantial work from a channel message:

1. Claim or create the message-anchored task before doing the work.
2. Keep substantive work in the canonical thread target: `#$LOOM_CHANNEL_ID:$LOOM_TRIGGER_MESSAGE_ID`.
3. Before sending a visible result, read the latest target message and rebase your response.
4. Use `--if-latest <latest_message_id>` when posting work that could conflict.
5. Complete the task with `loom --json task complete <task_id> --result "..."` when acceptance criteria are met.

For multi-actor workflows, the coordinator owns progress. Wake the next actor(s), schedule reminders when silence would stall the flow, and keep hidden state private.

## Useful Reads

Common state queries:

```bash
loom --json channel list
loom --json actor list
loom --json message read --target "$LOOM_REPLY_TARGET"
loom --json thread list
loom --json task list --source-message "$LOOM_TRIGGER_MESSAGE_ID"
loom --json inbox list
loom --json reminder list
```

Use `--include-private` only when you intentionally need private messages addressed to you.

## End-Of-Turn Check

End only after one of these is true:

- You sent the visible answer or private answer required by the trigger.
- You woke the exact actor(s) who must act next.
- You posted a pure no-action announcement.
- You called `run ignore` because no visible work is needed.
- You scheduled a reminder or other wake path for any unfinished coordination state.
