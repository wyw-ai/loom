# `data/services/` — ServiceSpec library

Drop-in ServiceSpec definitions consumed by `joi service serve`.
Each subdirectory ships:

- `spec.json` — the ServiceSpec; passes `joi service validate <path>`.
- `bundle/` — operator scripts the spec invokes. Every script supports
  `--dry-run`, prints planned actions to stdout, and exits 0 with **no
  side effects** so the spec can be exercised offline (CI / pre-deploy).

Service inventory (Phase 3 of the dev-helper migration —
`docs/remove-dev-helper-migration-design.md` §4.5 / §4.7 / §7):

| Spec | Plugin kind | Lifecycle | Purpose |
| --- | --- | --- | --- |
| `repo-cache`     | `scheduler` | channel-level | mirror/refresh repo working copies under host data dir |
| `repo-notes`     | `command`   | channel-level | pull/push/verify a1 kbase repo notes (mirror to channel ws) |
| `repo-provision` | `command`   | channel-level | thread bootstrap hook: clone-manifest → thread workspace mounts |
| `mr-detector`    | `scheduler` | thread-bound  | per-thread MR poller; emits `mr-event-*` artifacts, self-completes |

`kind = "command"` is forward-compat for the once/command plugin landing
in p4a; `joi service validate` only checks JSON shape, so these specs
are usable today as deployment artifacts.

Artifact outputs follow `docs/artifact-contracts.md`:

- `repo-provision` consumes `clone-manifest.json`, emits
  `repo-provision-receipt.json` (per repo: `from`, `to`, `ref`, `sha`).
- `mr-detector` emits `mr-event-<event_kind>-<mr_id>.json` per
  transition, with `raw_event_fingerprint = sha256(canonical_payload)`.

Service state (cursors, dedupe, pid, log) lives under
`~/.local/share/joi/service-host/services/<service_id>/` (channel-level)
or `.../<service_id>/instances/<thread_id>/` (thread-bound). Workspace
files referenced from these specs are advisory mirrors only.
