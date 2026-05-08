# a1-bug-fix-loop

Serial driver for `feedback-scanner` scan results.

Each tick (default `*/5 * * * *`):

1. Reads the newest `feedback-scan v1` event from the scanner thread.
2. For every tracked bug currently `in_progress`, checks whether its
   bugfix thread has published `mr-merged.v1` or a merged `mr-final.v1`.
   If yes, comments on the corresponding Aone feedback, updates its
   workitem status to `Fixed`, marks the local ledger `fixed`, and writes
   the closure note in the resident scanner thread.
3. If the concurrency budget allows (`params.concurrency`, default 1),
   promotes the next `pending` feedback from the scanner artifact to
   `in_progress`:
   - creates a `bugfix-<feedback_id>` thread under the same channel,
   - handoffs to `actor_a1_bug_triage` with the feedback context,
   - claims the feedback in `a1` (best-effort).
4. Persists the ledger to `<instance.data_dir>/state.json` and emits
   one `bug-fix-loop-status.v1` artifact per tick summarising
   `in_flight`, `fixed_total`, and the `tracking` map.

## Why a scheduler service

There is no event-bus subscription primitive yet. Polling every five
minutes is acceptable for the actual pacing of bug fixes, and means
we reuse the existing scheduler plugin without growing the runtime.

## Dev / offline behaviour

When `joi` or `a1` is missing on PATH (or `--dry-run` is set), the
tick is a structural no-op: the script still computes a status
artifact (with empty `tracking`) so subscribers can prove liveness
without side effects.

## Scope binding

`thread_bound`. The intended host is a `bug-fix-loop` thread under
`a1-dev-canfeng`. Pass `params.scanner_thread_id` to point at the
scanner thread (typically `feedback-scan`) and `params.channel_id` to
the parent channel. The loop creates per-bug bugfix threads via
`joi thread create --channel <chan>`.
