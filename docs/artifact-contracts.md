# Artifact Contracts

Cross-agent artifact JSON shapes for the joi runtime. Each artifact below
is published with `joi artifact publish` and referenced via `attaches_artifact`
on the `content.add` / `handoff.delivery` event that announces it. See
[`remove-dev-helper-migration-design.md`](./remove-dev-helper-migration-design.md)
§4.3 / §4.3.1 for the surrounding model. **These are stable contracts** —
producers may extend them with non-required fields (forward compatible),
but consumers must tolerate unknown fields and never rely on workspace
paths to discover them.

Conventions
-----------
- All artifacts are JSON unless explicitly noted. Names use kebab-case file
  stems (e.g. `clone-manifest.json`).
- Times are RFC 3339 UTC strings.
- `producer` is the AgentSpec `actor.id` of the publishing agent. Required
  on every contract — operators use it to trace provenance during incident
  review.
- `schema_version` is `"1"` for all contracts in this revision; bumped only
  on incompatible changes.
- An artifact MUST be byte-identical across reads. To "update" a contract,
  publish a new artifact and emit a new `content.add` event; do not mutate
  the previous artifact.

---

## 1. `task-goal.json`

Producer: `classmaster` / `router`. Consumer: `teacher` / `delivery` /
`feedback-fix-orchestrator`.

```json
{
  "schema_version": "1",
  "producer": "classmaster",
  "task_id": "task-2024-04-12-a1-bug-7421",
  "title": "Fix off-by-one in MR label sync",
  "narrative": "Free-form prose describing what the user wants and why.",
  "scope": {
    "channel_id": "a1-auto-dev",
    "thread_id": "thread-bug-7421"
  },
  "out_of_scope": [
    "do not touch the merge-strategy plugin"
  ],
  "links": [
    { "kind": "issue", "url": "https://.../issues/7421" }
  ],
  "created_at": "2024-04-12T03:14:15Z"
}
```

Required: `schema_version`, `producer`, `task_id`, `title`, `narrative`,
`scope.channel_id`, `created_at`.

---

## 2. `definition-of-done.json`

Producer: `classmaster` / `teacher`. Consumer: `delivery` / `teacher` /
`feedback-fix-orchestrator`.

A DoD is a *checklist of falsifiable criteria*. Each criterion has a
machine-checkable `verify` hint (kept as informal text) and a unique
`id` so downstream `validation-report` can reference it.

```json
{
  "schema_version": "1",
  "producer": "classmaster",
  "task_id": "task-2024-04-12-a1-bug-7421",
  "criteria": [
    {
      "id": "dod-1",
      "must": "all CI checks green on the MR head commit",
      "verify": "gh pr checks $MR --required"
    },
    {
      "id": "dod-2",
      "must": "no new TODO/FIXME introduced under crates/mr-sync/",
      "verify": "git diff --stat origin/main -- crates/mr-sync/ | grep -E 'TODO|FIXME' || true"
    }
  ],
  "out_of_scope": ["dod-2 is informational; CI is authoritative"],
  "created_at": "2024-04-12T03:15:00Z"
}
```

Required: `schema_version`, `producer`, `task_id`, `criteria` (≥1),
each criterion's `id` + `must`.

---

## 3. `clone-manifest.json`

Producer: `discovery` / `router`. Consumer:
`delivery` (and `joi thread create --bootstrap-artifact`).

This shape is *also consumed by* `joi thread create --bootstrap-artifact`
to populate `mounts[]` on the new thread's `scope.json`. See
`crates/cli/src/cmd/thread.rs::parse_bootstrap_mounts`.

```json
{
  "schema_version": "1",
  "producer": "discovery",
  "task_id": "task-2024-04-12-a1-bug-7421",
  "repos": [
    {
      "repo_id": "github.com/example/a1-auto-dev",
      "ref": "main",
      "from": "service://repo-cache/cache/github.com%2Fexample%2Fa1-auto-dev",
      "to": "repos/a1-auto-dev",
      "readonly": false,
      "purpose": "primary"
    },
    {
      "repo_id": "github.com/example/shared-libs",
      "ref": "v3.2.0",
      "readonly": true,
      "purpose": "reference"
    }
  ],
  "created_at": "2024-04-12T03:20:00Z"
}
```

Per-repo: only `repo_id` is strictly required. `from` defaults to
`service://repo-cache/cache/<urlencoded(repo_id)>`, `to` defaults to
`repos/<basename(repo_id)>`, `readonly` defaults to `false`. `ref` and
`purpose` are advisory metadata for delivery agents.

`thread create` will accept either this shape **or** a `{ "mounts": [...] }`
explicit override; explicit `mounts[]` skips the repo-rewrite step.

---

## 4. `lesson-plan.md`

Producer: `teacher` / `lesson-designer`. Consumer: `human` / downstream
agent (e.g. `delivery` for replay).

Markdown body with a YAML/TOML-style **JSON frontmatter** block as the
machine-readable header. The frontmatter is the contract; the markdown
body is the human-readable plan and is not parsed by other agents.

````md
```json
{
  "schema_version": "1",
  "producer": "teacher",
  "task_id": "task-2024-04-12-a1-bug-7421",
  "skills": ["a1.mr-sync.label-policy", "rust.test-ergonomics"],
  "estimated_blocks": 4,
  "prerequisites": ["definition-of-done.json#dod-1"],
  "created_at": "2024-04-12T04:00:00Z"
}
```

# Lesson 1 — diagnosing the off-by-one

(prose body, code samples, …)
````

Required (frontmatter): `schema_version`, `producer`, `task_id`, `skills`
(may be empty). `prerequisites[]` references criteria from the DoD by
`<file>#<id>`; the runtime does not enforce ordering, but downstream
tooling may.

A consumer that needs only the metadata MUST parse the first ```` ```json ````
fenced block at the top of the file. Producers MUST place exactly one such
block as the first non-blank content.

#### Optional `spec_apply` block (classroom 教学循环收口)

Lesson-plans destined for the `approval.spec_apply` action gate
(`docs/remove-dev-helper-migration-design.md` §4.4) MAY include a
machine-actionable `spec_apply` block in the frontmatter. `joi spec
apply --action <event_id>` reads it after the human approves and
applies the changes to the on-disk AgentSpec / ServiceSpec, then bumps
the reload-epoch marker so the running host re-spawns the worker.

```json
{
  "schema_version": "1",
  "producer": "teacher",
  "task_id": "task-2024-04-12-classroom-delivery-tweak",
  "skills": ["a1.delivery.handoff-template"],
  "spec_apply": {
    "target": { "kind": "agent", "id": "delivery" },
    "spec_patch": {
      "promptTemplate": "...new system prompt..."
    },
    "bundle_writes": [
      { "path": "snippets/handoff.md", "contents": "## Handoff\n..." }
    ]
  }
}
```

`spec_apply` semantics:

- `target.kind` ∈ `{"agent", "service"}`. `target.id` matches the
  on-disk `<id>/spec.json` (preferred) or `<id>.json` (legacy flat
  layout).
- `spec_patch` is JSON deep-merged onto the current spec. Object keys
  recurse; arrays / scalars replace.
- `bundle_writes[].path` is relative to the spec's `bundle/` sibling
  (rejects absolute paths and `..`).
- The previous `spec.json` and any overwritten bundle file are copied
  into `<spec-dir>/.backups/<utc-timestamp>/` before the new contents
  are written.
- After writing, the reload epoch for the target is bumped; a host
  running `joi {agent,service} serve` respawns the worker on the next
  poll cycle.
- A `runtime_receipt` artifact (JSON) and a `status.update` event with
  `responds_to` the action.response and `attaches_artifact` the
  receipt are appended to the original action.request scope so the
  decision is replayable from timeline alone.

---

## 5. `validation-report.json`

Producer: `teacher` / `delivery`. Consumer: `release-approval` /
`classmaster` / `feedback-fix-orchestrator`.

```json
{
  "schema_version": "1",
  "producer": "delivery",
  "task_id": "task-2024-04-12-a1-bug-7421",
  "dod_artifact": "art_d0d_xxxxx",
  "results": [
    {
      "criterion_id": "dod-1",
      "status": "pass",
      "evidence_uri": "art_run_99a8c",
      "checked_at": "2024-04-12T05:32:11Z"
    },
    {
      "criterion_id": "dod-2",
      "status": "fail",
      "evidence_uri": null,
      "note": "introduced 1 new TODO in crates/mr-sync/sync.rs",
      "checked_at": "2024-04-12T05:32:14Z"
    }
  ],
  "summary": "1/2 passed; release blocked on dod-2.",
  "completed_at": "2024-04-12T05:33:00Z"
}
```

Required: `schema_version`, `producer`, `task_id`, `dod_artifact`,
`results[]`, each result's `criterion_id` + `status` (`pass` | `fail` |
`skip`).

---

## 6. MR event artifacts (`mr-events`)

Producer: `mr-detector` (thread-bound) / channel-level `mr-watcher`.
Consumer: `delivery` / `release` / `classmaster`.

Each MR transition is published as its own artifact and announced via
`status.update` event. The artifact name is `mr-event-<event_kind>-<mr_id>.json`.

```json
{
  "schema_version": "1",
  "producer": "mr-detector",
  "event_kind": "merged",
  "mr_id": "https://github.com/example/a1-auto-dev/pull/4271",
  "title": "fix: off-by-one in label sync",
  "head_sha": "0a1b2c3d…",
  "base_sha": "9f8e7d6c…",
  "author": "octocat",
  "labels": ["bugfix", "release-blocker"],
  "ci_status": "success",
  "merged_at": "2024-04-12T06:14:01Z",
  "captured_at": "2024-04-12T06:14:02Z",
  "raw_event_fingerprint": "sha256:…"
}
```

`event_kind` ∈ `{ "opened", "labeled", "review_requested", "approved", "merged", "closed" }`.
`raw_event_fingerprint` is the SHA-256 over the canonicalised upstream
payload — used by the migration tool's verify pass to confirm
`mr-watcher/state.json::seen_note_keys` carried over without loss.

Required on every event: `schema_version`, `producer`, `event_kind`,
`mr_id`, `captured_at`, `raw_event_fingerprint`. Other fields are
event-kind dependent (e.g. `merged_at` only on `event_kind == "merged"`).

---

## Versioning

Add new optional fields at any time without bumping `schema_version`.
A `schema_version` bump means: at least one previously-required field
changed shape, became optional, was renamed, or removed. Producers
SHOULD continue publishing the old version alongside for one release
cycle and consumers SHOULD accept both.
