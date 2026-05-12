# tests/e2e/fixtures/

Static inputs for the local e2e harness under `scripts/e2e/`.

| Path | Producer | Consumer | What |
| --- | --- | --- | --- |
| `make-bare-repo.sh` | this dir | `scripts/e2e/local-up.sh` | Generates `repo.git/` (a bare repo with one seed commit on `main`). Re-runnable; idempotent. |
| `mr-fetch-merged.sh` | this dir | `mr-detector` (`MR_DETECTOR_FETCH_CMD`) | Prints a canonical "merged" payload to stdout. Stand-in for `gh pr view --json` / `glab mr view --json` so local poll can exercise the merged transition without a remote. |
| `mr-fetch-open.sh`   | this dir | `mr-detector` | Prints an "open, ci passing" payload. Use for the labeled-only / status.update half of the loop. |

Lifecycle: produced by `local-up.sh start` (which calls `make-bare-repo.sh` and exports the fetch script paths), wiped by `local-up.sh nuke`. Nothing here is consumed in `cargo test` — these fixtures intentionally stay out of unit-test reach.
