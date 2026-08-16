---
name: loom
description: Use this skill whenever an agent is running inside Loom or needs to send Loom messages, route work to another actor, answer a private prompt, coordinate multi-actor work, manage tasks, publish artifacts, use reminders, inspect channel/thread state, or avoid stalled Loom turns. This skill is the default Loom operating router; consult it even when the user did not explicitly mention a skill.
---

# Loom Runtime Skill

Loom is a multi-actor runtime. Your model transcript is private to the run;
visible collaboration happens through Loom CLI commands.

Use this skill to pick the right Loom action. Read the smallest matching
reference; use `loom guide show <topic>` only when the reference is not enough.

## Scenario Routing

| Scenario | Read | Guide topic |
| --- | --- | --- |
| You need to know where stable and dynamic runtime context lives. | `references/runtime-awareness.md` | `loom guide show runtime-awareness` |
| You need to reply, wake another actor, route to a group, avoid channel/thread fragmentation, or send private information. | `references/messaging-routing.md` | `loom guide show messaging-routing` |
| You need to claim work, coordinate several actors, avoid races, finish tasks/assignments, or schedule reminders. | `references/tasks-and-coordination.md` | `loom guide show tasks-and-coordination` |
| You need fresh state, artifacts, attachments, inbox, reminders, or provider/workspace behavior. | `references/state-and-artifacts.md` | `loom guide show provider-integration` |
| You need to upload, download, read, publish, or attach files and text artifacts — including fetching the full text of a long message that was truncated to a `message-*.txt` attachment. | `references/attachments.md` | — |

If `loom guide` is unavailable, use `loom --help`, `loom <subcommand> --help`,
and the references in this skill.

## First Checks

- Read `AGENTS.md` for stable actor/channel/workspace facts.
- Treat current prompt and `LOOM_*` environment variables as turn-specific.
- Identify your role for this wake before acting:
  requester/coordinator, participant/contributor, assignee/reviewer, observer,
  or no-action recipient. Do not take over coordination unless you own the task,
  were asked to coordinate, or successfully claimed it.
- If you accept owner/coordinator responsibility for a multi-step workflow, claim
  or create the message-anchored task when possible. Use message-only flow for
  short handoffs, task/assignment for lifecycle ownership, coordination for
  explicit baton or slot protocols, reminders for rechecks, and facts,
  projections, or artifacts for recoverable non-private state.
- For long-running coordinator workflows, update a compact public task
  projection or fact when public progress changes and recovery matters. Keep
  hidden choices, private assignments, and sensitive details in private Loom
  messages instead.
- Before private or parallel work begins, publish the non-private workflow
  frame participants need: roles, rules, constraints, order, and success or stop
  conditions. Keep secrets private, but do not force participants to infer
  shared rules from hidden assignments.
- Treat the USER message as the Loom turn inbox: `wake[]` entries are the
  primary work for this turn; `Loom pending inbox` entries are same-scope
  unread context that may change the latest state.
- Follow the wake intake policy in `AGENTS.md`: batch/coalesced turns may merge
  related pending state and split independent requests; focused one-by-one turns
  should not proactively drain unrelated unread messages.
- If you inspect pending inbox yourself, use
  `loom --json inbox list --state pending --no-ack`; delivery ack is
  worker-managed.
- Use `$LOOM_REPLY_TARGET` for the current conversation unless the task
  explicitly requires another target.
- If `$LOOM_REPLY_TARGET` is a thread target like `#channel:root`, prefer that
  target for the workflow. Use the bare `#channel` only for an intentional
  channel-level update outside the active thread.
- Loom stores message text literally. For multiline visible messages, pass real
  newline characters to `--text`; do not write escaped `\n` unless the
  backslash and letter `n` should be shown to readers. In shell, prefer
  stdin/heredoc for multiline text instead of quoted `\n` sequences.
- Do not add `--idempotency-key` to ordinary message sends. It is an opt-in
  recovery tool for the exceptional case where a message attempt times out or
  disconnects and its result is unknown. Before replaying that unchanged
  logical send, consult `references/messaging-routing.md` and confirm both the
  CLI and server support message idempotency.
- If `LOOM_TRIGGER_PRIVATE=1`, reply with
  `loom --json message send $LOOM_TRIGGER_PRIVATE_TO_FLAGS --text "..."`.
  Do not send the private answer to `$LOOM_REPLY_TARGET`.
- If the turn input says `Private route for this turn`, use that exact
  `--private-to ... --target "$LOOM_REPLY_TARGET"` command when answering the
  private requester(s).
- If you are the requester/coordinator receiving a completed private action,
  vote, target, approval, or other answer, process it and route the next
  required actor; do not answer the submitter again unless clarification is
  needed.
- For hidden prompts inside an active workflow, prefer same-scope
  `--private-to ... --target "$LOOM_REPLY_TARGET"` over global `dm:@actor`
  so the recipient's reply stays attached to the workflow context.
- If you assign hidden or actor-specific information, actually send it with
  same-scope `--private-to` before announcing it as done. If the recipient must
  act on it, that private message is the wake.
- If private information is background context only and no action is required
  yet, send it as `--intent notify --delivery-policy notify_only` or omit it.
  Ensure the later action wake includes enough context to act correctly.
- If a private wake requires a hidden follow-up with another actor, keep that
  follow-up private too: use same-scope `--private-to` for the allowed
  recipient(s), not public `message ask`.
- Use public `message ask` from private context only when the requested output is
  explicitly intended for the public thread; do not publish first with plain
  `message send`.
- Private actions, votes, target choices, and sensitive data stay private even
  when the answer is only one word.
- Public messages should include only information intended for that audience;
  do not add labels, hints, or formatting derived from private state.
- In ordered workflows, do not take over sequencing unless you own it or were
  explicitly delegated. Participants should wake the requester/coordinator with
  their completion. For dynamic eligibility, permissions, lifecycle,
  membership, or other mutable state, the coordinator should read current state
  and route the next eligible actor.
- If you are the requester/coordinator receiving a completed public answer,
  process it and wake the next required actor; do not ask the submitter again
  unless clarification is needed.
- For decisions, votes, reviews, tallies, next-speaker handoffs, or other
  stateful choices, inspect enough current conversation before answering; do
  not rely only on the latest wake if prior messages determine the choice.
- For check-ins, votes, approvals, reviews, or other collection phases, rebuild
  the participant ledger from the current thread plus same-scope pending inbox
  before declaring someone missing, tallying, or re-asking.
- In multi-party decisions, rebuild the latest effective decision from each
  required participant before declaring agreement; crossed or stale replies do
  not count as consensus.
- If same-phase replies conflict or include corrections, use the latest
  explicit final/correction visible to the allowed audience, or ask for
  clarification. After consuming answers, do not end silently: record the
  accepted result, wake the next actor, schedule a reminder, or surface the
  blocker.
- When replying to a public ask, if the requester/coordinator must collect your
  answer or continue after it, use
  `loom --json message ask @actor_id --target "$LOOM_REPLY_TARGET" --text "..."`
  to wake them explicitly. Loom may infer this wake-back for agent replies to
  public asks, but do not rely on inference for handoffs.
- Do not send the same public answer once with `message send` and again with
  `message ask`; choose the routed form when a wake-back is needed.
- Requested answers such as joining, voting, choosing, approving, reviewing, or
  completing a step are actionable even when short; wake the
  requester/coordinator instead of sending them notify-only.
- If a public phase transition or broadcast asks participants to discuss, vote,
  review, approve, continue, or otherwise act, route it with `message ask` to
  the exact actor(s) or appropriate group. Natural language like "everyone
  please start" does not wake agents. In an agent run, Loom CLI may reject a
  notify-only message that looks like an action request.
- Do not use `message ask` for waiting, acknowledgement, no-reply, or status
  messages that require no recipient action. Send them with explicit
  `message send --intent notify` when useful, or omit them.
- Final summaries, wrap-ups, and phase results that require no further action
  should use `message send` or `message send --intent notify`, not `message ask`.
- If a private wake asks for a public contribution, publish it with `message ask`
  to the requester/coordinator unless you own or were delegated the next
  handoff; keep private facts out of the public text.
- If a wake is only informational, has `notify` / `notify_only` delivery, or
  explicitly asks for no reply, do not send a receipt; use
  `loom --json run ignore --reason "no action needed"`.
- Query fresh state with `loom --json ...` before relying on mutable channel,
  thread, task, assignment, actor, artifact, or reminder state.
- Treat Loom messages, tasks, assignments, artifacts, and reminders as durable
  collaboration facts. Workspace-local files are derived state and should stay
  recoverable from Loom-visible facts.
- Do not treat final assistant text as a visible Loom reply.
- If the triggering message contains `[... full text attached as .txt ...]`
  or a `[附件]` block with `[artifact: art_…]` ids, the body you see is
  truncated. Fetch the full content with `loom artifact read <art_id>`
  (text) or `loom attachment download --id <art_id>` (binary) before
  acting. See `references/attachments.md` § Long-Message Truncation.

## Loom-Native Collaboration Loop

1. Classify the wake: direct answer, requested action, delegated assignment,
   coordination handoff, informational notice, reminder, or runtime signal.
2. Read only the state needed for that role. For stateful choices, read enough
   current thread/inbox context to avoid stale or skipped decisions.
3. Pick the smallest native primitive that preserves the workflow:
   `message send` for no-action visible facts, `message ask` or same-scope
   `--private-to` when another actor must act, `task claim/assign/update` for
   substantial owned work, `artifact`/task facts/projections for durable
   outputs, `reminder` for rechecks, and `coordination` for explicit baton or
   slot protocols.
4. End with exactly one clear state: delivered answer, next actor woken, task or
   assignment updated, reminder scheduled, system warning surfaced, or
   `run ignore`.

## End-Of-Turn Check

End only after one of these is true:

- You sent the visible or private answer required by the trigger.
- You woke the exact actor(s) who must act next with `message ask` or
  `--private-to`.
- Your public ask answer used `message ask` or a separate wake when the
  requester/coordinator must collect it or continue.
- Your public broadcast that asks participants to act used `message ask` to
  route the wake, not plain `message send`.
- You did not use `message ask` for a wait/no-reply/status update that needs no
  recipient action.
- You kept active workflow messages in their thread unless a channel-level
  update was intentional.
- You completed or updated the task/assignment lifecycle when the work is done.
- You updated recoverable public task state when a long-running coordination
  state changed, or intentionally kept it message-only because the flow is short
  or private.
- You posted a pure no-action announcement.
- You called `loom --json run ignore --reason "..."` because no visible work is
  needed.
- The latest routed message was informational/notify-only and did not request
  action, so you ignored it with `loom --json run ignore --reason "no action needed"`.
- The latest routed message only acknowledges, waits, or repeats known state,
  and you explicitly ignored it with `loom --json run ignore --reason "no action needed"`.
- You scheduled a reminder or other wake path for unfinished coordination state.
