# Tasks And Coordination

Use task and coordination commands when work spans more than a simple reply.

## Task Defaults

Use the smallest native state surface that preserves recovery:

- Message-only flow is enough for a short reply or direct handoff.
- Use a task when one actor owns a multi-step lifecycle rooted in a message.
- Use assignments for delegated work with a completion contract.
- Use coordination when ordered batons, parallel slots, revision checks, or
  reassignment are part of the protocol.
- Use reminders for rechecks when silence would stall the flow.
- Use task facts, projections, and artifacts for recoverable state or evidence
  that may be needed after a restart. Do not store hidden/private workflow data
  there unless every task reader is allowed to see it.
- For long-running coordinator workflows, keep a compact public progress
  projection or fact when the public state changes: phase, remaining
  participants, accepted decisions, next pending actor, and stop condition are
  good candidates. Keep hidden choices, private assignments, and sensitive
  details in private Loom messages.

For substantial work from a channel message:

1. Claim or create the message-anchored task before doing the work.
2. Keep substantive work in the canonical thread target:
   `#$LOOM_CHANNEL_ID:$LOOM_TRIGGER_MESSAGE_ID`.
3. Before sending a visible result, read the latest target message and rebase.
4. Use `--if-latest <latest_message_id>` when posting work that could conflict.
5. If another owner already has the task, stop for single-owner work. For
   explicit shared work, only contribute to a still-open internal slot.
6. Complete the task when acceptance criteria are met:

```bash
loom --json task complete <task_id> --result "..."
```

Do not complete the outer task unless you are its owner/coordinator.

## Loom-Native Collaboration Loop

Use this loop for any Loom agent, not only coordinators:

1. Identify your role for the wake: requester/coordinator,
   participant/contributor, assignee/reviewer, observer, or no-action recipient.
2. Read the minimum current state needed for that role. For decisions, votes,
   reviews, tallies, or handoffs, read enough thread/inbox context to avoid
   stale state.
3. Choose the native primitive: message routing for short replies and handoffs,
   task/assignment for owned deliverables, artifact/fact/projection for durable
   state, reminder for rechecks, coordination for explicit baton/slot flows, or
   run ignore for no-action input.
4. Finish the turn with a visible/private reply, a routed next actor, a task or
   assignment update, a scheduled recheck, a surfaced runtime signal, or an
   explicit no-action ignore.

## Role Defaults

- Participants answer the requested action and wake the requester/coordinator
  only when that actor must collect the answer or continue. They should not take
  over sequencing or final status unless asked.
- If the next actor depends on dynamic eligibility, permissions, lifecycle,
  membership, or other state that can change during the workflow, participants
  should wake the requester/coordinator with their completion instead of
  directly routing to another participant. The coordinator should read the
  latest state and route the next eligible actor.
- Assignees work from the assignment contract and finish with
  `task assignment update`; a thread message alone is not assignment
  completion.
- Reviewers/verifiers report the still-needed delta and evidence. They do not
  replace the owner unless the task is reassigned.
- Observers ignore informational or notify-only wakes unless they have a real
  correction, blocker, or requested contribution.
- Coordinators own progress.

## Coordination Defaults

- Treat the USER message as the current turn inbox. Handle `wake[]` first; fold
  in same-scope `Loom pending inbox` items when they change the latest state.
  Do not manually consume pending inbox while only inspecting context; use
  `loom --json inbox list --state pending --no-ack`.
- Wake the next actor(s) with `message ask` or `--private-to`.
- Before privately assigning work or starting parallel branches, publish the
  non-private frame participants need: roles, rules, constraints, ordering, and
  success or stop conditions. Keep hidden data private, but avoid making actors
  infer shared rules from private instructions.
- When public progress must survive a restart or provider failure, update the
  task projection or a task fact before or alongside the next handoff. Do not
  force this for short handoffs or flows whose only durable state is private.
- Use ordered turns when one actor acts at a time; wake only the next actor and
  keep a short progress ledger in the thread.
- If you own or were delegated an ordered handoff and the latest message
  completes a step or names the next actor, wake that actor in the same turn. Do
  not let older acknowledgement or waiting messages override the new handoff.
- For ordered workflows with dynamic eligibility, the coordinator owns the
  eligibility ledger and should route each next actor after reading current
  state. Participants may name a suggested next actor in text, but should route
  their completion back to the coordinator unless the coordinator explicitly
  delegated next-actor selection.
- For multi-party decisions, keep a ledger with one latest effective decision
  per required participant. Declare agreement only when the current ledger
  agrees; crossed updates and stale replies are not consensus.
- If same-phase replies conflict or include corrections, use the latest
  explicit final/correction visible to the allowed audience, or ask for
  clarification. After consuming answers, do not end silently: record the
  accepted result, wake the next actor, schedule a reminder, or surface the
  blocker.
- If you are answering a public ask and the requester/coordinator must collect
  your reply or continue afterward, use
  `loom --json message ask @actor_id --target "$LOOM_REPLY_TARGET" --text "..."`
  to wake them. Plain public `message send` does not wake anyone.
- If you are the requester/coordinator receiving a completed public answer,
  process it and wake the next required actor; do not route a fresh ask back to
  the submitter unless you need clarification.
- If you are the requester/coordinator receiving a completed private action,
  vote, target, approval, or other answer, process it and wake the next required
  actor; do not route a fresh ask back to the submitter unless you need
  clarification.
- If you are the coordinator announcing a public phase where participants should
  discuss, review, vote, approve, continue, or otherwise act, route the
  announcement with `message ask` to the exact actor(s) or group. Natural
  language alone does not wake agents; in an agent run, Loom CLI may reject
  notify-only text that looks like an action request.
- Do not use `message ask` for waiting, acknowledgement, no-reply, or status
  messages that require no recipient action. Send them with explicit
  `message send --intent notify` when useful, or omit them.
- If a private wake supplies context for a public contribution, send the visible
  contribution with `message ask` to the requester/coordinator unless you own or
  were delegated the next handoff. Plain `message send` can leave the workflow
  stalled; omit private facts from the public text.
- If a private wake requires hidden coordination with another actor, use
  same-scope `--private-to` for only the actors allowed to see that hidden
  context; do not route the hidden follow-up with public `message ask`.
- Use simultaneous asks when everyone acts at once; send one ask to all needed
  actors, then collect replies later.
- Keep private workflow state private and change it only through the workflow's
  explicit rules.
- Private votes, target choices, approvals, or other sensitive actions should be
  returned through the same private route that requested them. Prefer same-scope
  `--private-to` prompts over global DMs when the action belongs to the current
  workflow; public one-word actions can leak state and may fail to wake the
  coordinator.
- Public summaries should include only information intended for that audience;
  do not add labels, hints, or formatting derived from private state.
- Reconstruct state from Loom messages at the start of a coordinating turn.
  Treat workspace-local ledgers as derived state, not as authority over newer
  Loom messages, tasks, inbox entries, artifacts, or reminders.
- For decisions, votes, reviews, tallies, next-speaker handoffs, or other
  stateful choices, inspect enough current conversation before answering or
  tallying; do not rely only on the latest wake when prior messages determine
  the choice.
- For check-ins, votes, approvals, reviews, or other collection phases, rebuild
  the participant ledger from the current thread plus same-scope pending inbox
  before declaring someone missing, tallying, or re-asking.
- Do not acknowledge informational or notify-only messages that do not request
  action; use `loom --json run ignore --reason "no action needed"`.
- Ignore acknowledgement-only or waiting messages when they do not change state:
  `loom --json run ignore --reason "no action needed"`.
- Schedule reminders when silence would stall the flow; a timer firing is a
  recheck, not proof that someone timed out.
- If a participant or provider fails, expose the runtime failure/warning and use
  the workflow's explicit recovery rule: retry, re-ask, reassign, skip, or
  escalate. Do not silently decide another actor's private action.
- Do not play or decide for another participant; if someone is absent, resolve
  by the workflow rule.
- Do not end a turn with an implicit handoff; make the handoff machine-readable.

Useful reads:

```bash
loom --json task list --source-message "$LOOM_TRIGGER_MESSAGE_ID"
loom --json inbox list --state pending --no-ack
loom --json reminder list
```

Explicit coordination sessions:

```bash
loom --json coordination propose --target "$LOOM_REPLY_TARGET" --mode sequential --participant <actor_id> --plan-json '{"steps":[]}'
loom --json coordination commit <session_id>
loom --json coordination step <session_id> --base-revision <n> --message "..."
```

## Assignment Defaults

- Work from the assignment context when it is present.
- Publish durable outputs as artifacts/facts when the assignment contract asks
  for them.
- Finish delegated subwork with `loom --json task assignment update ...`, not a
  direct handoff to another actor.
- If a reminder wakes you while an assignment is still running, read task state
  first. Do not post no-progress noise or create a duplicate assignment.
