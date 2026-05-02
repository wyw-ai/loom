# repo-provision

Channel-level command service that runs as the **thread bootstrap hook**
for `joi thread create --bootstrap-artifact <clone-manifest.json>`
(`docs/remove-dev-helper-migration-design.md` §4.7.2).

- Spec: `spec.json` (kind=`command`, autostart=false — invoked on demand
  by the thread create flow).
- Bundle: `bundle/provision.sh` — consumes a `clone-manifest.json`
  artifact and writes worktrees / `git clone --shared` mounts under
  `<thread_workspace>/repos/<basename>/`.

`--dry-run` is offline: no git operations, no FS mutations. The script
still emits a structurally valid `repo-provision-receipt.json` (with
`status="planned"` per repo) so downstream tooling can be tested.

Output (single JSON object on stdout):

```json
{
  "schema_version": "1",
  "producer": "repo-provision",
  "task_id": "task-…",
  "manifest": "/path/to/clone-manifest.json",
  "thread_workspace": "/path/to/.joi-workspaces/thread/…",
  "repos": [
    {
      "repo_id": "github.com/example/a1-auto-dev",
      "ref": "main",
      "from": "service://repo-cache/cache/github.com%2Fexample%2Fa1-auto-dev",
      "to": "repos/a1-auto-dev",
      "readonly": false,
      "purpose": "primary",
      "sha": "0a1b2c3d…",
      "status": "provisioned"
    }
  ],
  "captured_at": "2024-04-12T03:21:00Z"
}
```

The receipt is published as `repo-provision-receipt.json` and announced
via `thread.bootstrapped` per the spec's `emits` block.
