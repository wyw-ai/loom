# a1-feedback-scanner

Pulls recent feedback from the `a1` CLI, classifies each item using the
`category` field (best-effort), and emits two artifacts per tick:

- `feedback-scan.bugs.v1` — existing-bug bucket; consumed by the
  `a1-bug-fix-loop` service to drive serial bug fixing.
- `feedback-scan.others.v1` — `new_request` / `unclear` / `duplicate` /
  `not_actionable` buckets plus `diff_vs_previous_scan`; consumed by
  human readers via the channel public chat (router announces).

The schema for both artifacts lives in
[`docs/artifact-contracts.md`](../../../docs/artifact-contracts.md) §8 / §9.

## Schedule

`0 * * * *` (top of every hour). Override per-instance through the
service host params. Each tick is a single-in-flight job and dedupes
on `body_hash`, so re-runs of the same window do not re-publish.

## Offline / dev mode

When the `a1` binary is not on PATH **or** `--dry-run` is passed, the
scan emits structurally-valid artifacts with empty `items` / empty
buckets. The scheduler still publishes them so consumers can confirm
the service is alive. This keeps unit tests and dev loops green.

## State

`<instance.data_dir>/state.json` carries:
- `last_scan_id` — used to populate `diff_vs_previous_scan.previous_scan_id`.
- `last_others_fp` — `{ bucket: [feedback_id...] }` snapshot used to
  compute `added` / `removed` per bucket.

Deleting `state.json` resets the diff baseline; the next scan emits
without `diff_vs_previous_scan`.

## Lifecycle

`thread_bound`. The scanner is intended to run inside a thread named
`feedback-scan` under `a1-dev-canfeng`. It auto-stops when the thread
closes or when the scheduler self-completes (manual `service stop`).
