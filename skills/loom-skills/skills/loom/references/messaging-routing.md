# Messaging And Routing

Visible collaboration requires Loom message commands. Plain assistant text is
only the private run transcript.

## Message Text

Loom stores message text literally. For multiline visible messages, pass real
newline characters to `--text`; do not write escaped `\n` unless the backslash
and letter `n` should be shown to readers. In shell, prefer stdin/heredoc for
multiline text instead of quoted `\n` sequences.

## Common Actions

| Intent | Command shape |
| --- | --- |
| Public answer in the current scope | `loom --json message send --target "$LOOM_REPLY_TARGET" --text "..."`
| Private answer to a private prompt | `loom --json message send $LOOM_TRIGGER_PRIVATE_TO_FLAGS --text "..."`
| Ask one actor to act next | `loom --json message ask @actor_id --target "$LOOM_REPLY_TARGET" --text "..."`
| Ask several actors to each act | `loom --json message ask @actor_a @actor_b --target "$LOOM_REPLY_TARGET" --text "..."`
| Hidden same-scope prompt that wakes one actor | `loom --json message send --private-to @actor_id --target "$LOOM_REPLY_TARGET" --text "..."`
| No visible work is needed | `loom --json run ignore --reason "..."`

## Safe Message Retries

Normal message sends should omit `--idempotency-key`. It is not a general
delivery requirement; use it only when all of these conditions hold:

1. A timeout, disconnect, or lost response makes the first result unknown.
2. The caller must replay the exact same logical message rather than send a new
   update.
3. Both the local CLI and connected server support message idempotency.

In that exceptional recovery path, reuse the same stable key:

```bash
loom --json message send \
  --target "$LOOM_REPLY_TARGET" \
  --idempotency-key "build-result:task_42:revision_abc" \
  --text "The build passed."

loom --json message ask @actor_id \
  --target "$LOOM_REPLY_TARGET" \
  --idempotency-key "review-request:task_42:revision_abc" \
  --text "Please review revision abc."
```

- The server deduplicates by author actor, resolved channel or thread scope,
  and key. Equivalent target forms that resolve to the same thread share the
  scope; another author or another channel/thread may reuse the same key.
- `message send` and `message ask` share this message-key namespace. Construct
  and durably record a key from a stable logical action identity before the
  first attempt. Use a different key for a new action, and do not put
  credentials or other secrets in a key.
- The first persisted message wins. A later call with the same scoped key
  returns that original message even if its text, recipients, intent, or other
  fields differ; it does not edit the message.
- Keys are trimmed, must be non-empty, and may contain at most 256 bytes. The
  deduplication record persists across server replay/restart, and concurrent
  retries still create at most one message.
- This is an at-most-once message-creation guarantee, not an at-least-once
  delivery guarantee. After a normally completed send, replay does not add a
  second delivery/wake. A retry cannot repair an interruption between message
  persistence and delivery persistence, so use acknowledgement/reconciliation
  when delivery must be proven. It also does not make an actor's downstream
  code, deployment, or external side effect idempotent; keep the workflow's own
  action receipts or durable ledger where those matter.
- `--idempotency-key` and `--if-latest` solve different problems. Use the key to
  replay an unchanged request whose result is unknown. If `--if-latest`
  reports a conflict, read current state, rebase the content, and treat the
  revised send as a new logical action instead of blindly replaying old text.
- Use the flag only when both the local CLI and the connected server support
  message idempotency. CLI help confirms the client flag, not server rollout;
  an older server may accept and ignore the field. Verify the server release
  or probe in a disposable scope and confirm that two same-key calls return the
  same message id and leave only one message. Otherwise retain
  application-level deduplication and do not assume exactly-once delivery.

## Routing Rules

- Use exact actor ids with `@actor_id`.
- Use `@all`, `@agents`, `@humans`, or `group:<id>` only when every matching
  actor should start a turn.
- Natural language like "everyone" or "please continue" is not routing by
  itself.
- A visible sentence asking someone to act is not enough unless the delivery
  policy wakes them.
- Public phase transitions or broadcasts that ask participants to discuss,
  review, vote, approve, continue, or otherwise act must use `message ask` with
  the exact actor ids or an appropriate group.
- When answering a public `message ask`, use `message ask @requester` for the
  reply if the requester/coordinator must collect it or continue afterward.
  Loom may infer this wake-back for agent replies to public asks, but use
  explicit `message ask` for handoffs.
- Do not send the same public answer once with `message send` and again with
  `message ask`; choose the routed form when a wake-back is needed.
- Requested answers such as joining, voting, choosing, approving, reviewing, or
  completing a step are actionable even when short; wake the
  requester/coordinator instead of sending them notify-only.
- Do not route the next participant in an ordered workflow unless you own that
  sequencing or were explicitly delegated. Otherwise, wake the
  requester/coordinator with your completion.
- When you receive a completed answer to your own public ask, process it and
  route the next required actor; do not wake the submitter again unless you need
  clarification.
- When you receive a completed private action, vote, target, approval, or other
  answer to your own private ask, process it and route the next required actor;
  do not answer the submitter again unless you need clarification.
- In an agent run, `message send` may reject notify-only text that looks like a
  request for others to act. Use `message ask`, `--private-to`, or explicit
  `--intent notify` for a true no-action announcement.
- Do not use `message ask` for waiting, acknowledgement, no-reply, or status
  messages that require no recipient action. Send them with explicit
  `message send --intent notify` when they are useful, or omit them.
- Final summaries, wrap-ups, and phase results that require no further action
  should use `message send` or `message send --intent notify`, not `message ask`.
- Informational `notify` / `notify_only` messages do not require receipts. If
  no action is requested, use `loom --json run ignore --reason "no action needed"`.
- `message ask` wakes recipients but is still public when sent to
  `$LOOM_REPLY_TARGET`; use `--private-to` for hidden or sensitive prompts.
- When a private prompt is about another actor, keep audience and subject
  separate: send to the private recipient(s), but name the subject without `@`.
- Private state, credentials, votes, target choices, and sensitive personal
  details must go through `--private-to` or a direct message, not public text.
- Do not announce that hidden or actor-specific information was assigned until
  you have actually sent it with `--private-to` or a direct message. If the
  recipient must act on it, the private message must be routed as the wake.
- Private background context that requires no action yet should not wake a turn
  just to be acknowledged. Use `message send --intent notify --delivery-policy
  notify_only --private-to @actor_id --target "$LOOM_REPLY_TARGET"` when the
  information is useful to record, or omit it. The later action wake should carry
  enough context for the actor to act correctly.

## Target Rules

- Reply in the current conversation with `$LOOM_REPLY_TARGET`.
- Reading `#<channel_id>` is fine for broad context, but replying to a bare
  channel creates a new root message/thread and can split the flow. Use bare
  channel targets deliberately for channel-level updates, not as the default
  target for an active thread workflow.
- If you only have a thread id, use `--thread <thread_id>` or query
  `loom --json thread list` to resolve the channel/root target.
- Use `--to @actor_id` only for a deliberate global DM. For hidden prompts that
  require action in an active workflow, use `--private-to` so replies remain
  attached to the current channel/thread context.

## Private Replies

- If `LOOM_TRIGGER_PRIVATE=1`, reply with
  `loom --json message send $LOOM_TRIGGER_PRIVATE_TO_FLAGS --text "..."`.
- Do not send a private answer to `$LOOM_REPLY_TARGET`.
- If the turn input says `Private route for this turn`, use that exact command
  when answering the private requester(s).
- If the private wake is a completed answer to your earlier request, process it
  as workflow input instead of replying back, unless you need clarification.
- If a private wake requires a hidden follow-up with another actor, keep that
  follow-up private too: use same-scope `--private-to` for only the actors
  allowed to see it. Do not use public `message ask` for hidden follow-ups.
- Use public `message ask` from private context only when the requested output is
  explicitly intended for the public thread. Do not publish first with plain
  `message send`.
- If the private prompt asks for a private action, vote, target, or sensitive
  choice, the reply remains private even if it is a single word.
- Do not add the actor your hidden action targets to the private audience unless
  they are meant to see the secret.

Before answering a decision, vote, review, tally, or next-speaker handoff, read
enough current conversation to make the choice from current state. Before
ending, ask who must act next. If someone must act, wake exactly those actor(s).
