# mr-detector

Thread-bound scheduler service. One instance per delivery thread,
polling a single MR URL until it merges or closes, then self-completes
(`docs/remove-dev-helper-migration-design.md` §4.7.3).

- Spec: `spec.json` (kind=`scheduler`, `lifecycle=thread-bound`,
  `bind.scope=thread`, `auto_stop_on=[thread.closed, service.self_complete]`).
  Started via `joi service start --spec mr-detector --in <thread> --params '{"mr_url":"…"}'`.
- Bundle: `bundle/poll.sh` — single tick. Emits one `mr-event-*` JSON
  per *new* transition (matches `docs/artifact-contracts.md` §6).

Persisted state: `<instance.data_dir>/state.json` —
`{seen:{labels, ci_status, head_sha, state}, merged_emitted, closed_emitted}`.
On restart the file dedupes already-emitted terminal events.

Offline contract: `--dry-run` skips all network calls and prints a
single `op="poll"` line with `status="planned"`. Real fetch can be
swapped via `MR_DETECTOR_FETCH_CMD=<cmd>` (the command receives the MR
URL and must print canonical JSON to stdout); unset, the script uses a
stub payload that exercises the emit path without a remote.

The script writes `{"service.self_complete": true, "reason": "merged"|"closed"}`
as its **last** stdout line on terminal transitions; the service host
treats that as the auto-stop signal in §4.7.3.

`raw_event_fingerprint = "sha256:" + sha256(canonical_payload)` is
included on every emitted event for the migration tool's verify pass.
