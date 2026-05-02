# repo-notes

Channel-level command service that mirrors a1 kbase repo-note records
into `<channel_ws>/.joi/repos/notes/<repo_id>.json` and pushes operator
edits back. Source of truth remains a1 kbase; the workspace mirror is
advisory.

Subcommands (forward-compat with the `command` plugin landing in p4a):

| Subcommand | Script           | Purpose |
| --- | --- | --- |
| `pull`     | `bundle/pull.sh`   | kbase → workspace mirror |
| `push`     | `bundle/push.sh`   | workspace edit → kbase |
| `verify`   | `bundle/verify.sh` | check kbase clone_url matches expected |

All three accept `--dry-run` and exit 0 without invoking `a1`, so the
spec is exercisable in CI / pre-deploy.

Replaces `joi_rpc.py repo-stg-*` from the legacy dev-helper bundle.
