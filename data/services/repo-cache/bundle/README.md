# repo-cache

Channel-level scheduler service that mirrors repos declared in
`<channel_ws>/.joi/repos/manifest.json` into the service host data dir
under `<service.data_dir>/cache/<urlencoded(repo_id)>/`.

- Spec: `spec.json` (kind=`scheduler`, one job `sync-all`, default cron
  `*/15 * * * *`, dedupe=`payload_hash`, cursor=`body_hash`).
- Bundle: `bundle/sync.sh` — refresh script. `--dry-run` is offline and
  does not touch network or disk.

Emitted body (one JSON line per repo, consumed as the scheduler's event
payload):

```json
{
  "schema_version": "1",
  "producer": "repo-cache",
  "repo_id": "github.com/example/a1-auto-dev",
  "status": "ok",
  "action": "fetched",
  "cache_path": "/.../services/repo-cache/cache/github.com%2Fexample%2Fa1-auto-dev"
}
```

Downstream consumers (e.g. `repo-provision`) reference cached repos via
`service://repo-cache/cache/<urlencoded(repo_id)>` per
`docs/artifact-contracts.md` §3 (`clone-manifest.from`).
