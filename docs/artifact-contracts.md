# Artifact Contracts

Cross-agent artifact JSON shapes for the loom runtime. Each artifact below
is published with `loom artifact publish` and referenced via `attaches_artifact`
on the `message` / `directed message.delivery` event that announces it. See
[`channel-topology-design.md`](./channel-topology-design.md) for the
surrounding actor / event model. **These are stable contracts** —
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
  publish a new artifact and emit a new `message` event; do not mutate
  the previous artifact.

---

## 1. `task-goal.json`

Producer: `classmaster` / `router`. Consumer: `teacher` / `delivery`.

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

Producer: `classmaster` / `teacher`. Consumer: `delivery` / `teacher`.

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
`delivery` (and `loom thread create --bootstrap-artifact`).

This shape is *also consumed by* `loom thread create --bootstrap-artifact`
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

Lesson-plans destined for the `approval.spec_apply` action gate MAY include a
machine-actionable `spec_apply` block in the frontmatter. `loom spec
apply --action <action_response_message_id>` reads it after the human approves and
applies the changes to the on-disk AgentSpec / ServiceSpec, then bumps
the reload-epoch marker so the running host re-spawns the worker.

```json
{
  "schema_version": "1",
  "producer": "teacher",
  "task_id": "task-2024-04-12-classroom-delivery-tweak",
  "skills": ["a1.delivery.directed message-template"],
  "spec_apply": {
    "target": { "kind": "agent", "id": "delivery" },
    "spec_patch": {
      "promptTemplate": "...new system prompt..."
    },
    "bundle_writes": [
      { "path": "snippets/directed message.md", "contents": "## Directed Message\n..." }
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
  running `loom {agent,service} serve` respawns the worker on the next
  poll cycle.
- A `runtime_outcome` artifact (JSON) and a `status.update` event with
  `responds_to` the action.response and `attaches_artifact` the
  delivery ack are appended to the original action.request scope so the
  decision is replayable from timeline alone.

---

## 5. `validation-report.json`

Producer: `teacher` / `delivery`. Consumer: `release-approval` /
`classmaster`.

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

---

## 7. `bug-triage.v1`

Producer: `a1-bug-triage`. Consumer: `router` (a1-dev-canfeng), and the
`a1-feedback-scanner` service when aggregating per-feedback verdicts.

Emitted once per inbound feedback item to record the triage verdict. The
contract is intentionally narrow: triage classifies, it does not plan.
Planning (rough plan / DoD / repos) is delegated downstream to discovery
via the existing `task-goal.json` + `definition-of-done.json` +
`clone-manifest.json` triplet.

```json
{
  "schema_version": "1",
  "producer": "a1-bug-triage",
  "feedback_id": "fbk-2024-04-15-9911",
  "category": "existing_bug",
  "severity": "major",
  "certainty": "high",
  "suspected_module": "router/directed message",
  "next_actor": "router",
  "summary": "Single-line restatement of the user's report.",
  "evidence_refs": [
    { "kind": "feedback", "id": "fbk-2024-04-15-9911" },
    { "kind": "thread", "id": "thread_2a6d3b569aa6" }
  ],
  "captured_at": "2024-04-15T08:21:33Z"
}
```

Allowed values:
- `category`: `"existing_bug"` | `"new_request"` | `"unclear"` | `"duplicate"` | `"not_actionable"`.
- `severity`: `"blocker"` | `"major"` | `"minor"` | `"trivial"`.
- `certainty`: `"high"` | `"medium"` | `"low"`.
- `next_actor`: any AgentSpec actor id (typically `"router"` for actionable
  items, omitted or `"none"` for `not_actionable` / `unclear`).

Required: `schema_version`, `producer`, `feedback_id`, `category`,
`severity`, `certainty`, `summary`, `captured_at`.

`evidence_refs` is required-but-may-be-empty. `suspected_module` is
optional and free-form (kebab-case path or symbol fragment).

The triage artifact is the **input** to the bug-fix loop: the loop reads
`category == "existing_bug"` items from the latest `feedback-scan.bugs.v1`
report and dispatches them serially through router → discovery → delivery.
Items in any other category are forwarded to the human-decision report
(`feedback-scan.others.v1`) instead.

---

## 8. `feedback-scan.bugs.v1`

Producer: `service_a1_feedback_scanner`. Consumer:
`service_a1_bug_fix_loop` (subscribes), human (read-only review).

Emitted once per scanner tick when there is at least one feedback item
classified as `category == "existing_bug"`. The artifact aggregates all
existing-bug verdicts since the previous scan into a single report so
the bug-fix loop can iterate them serially. Each entry quotes the
verbatim `bug-triage.v1` produced for that feedback.

```json
{
  "schema_version": "1",
  "producer": "service_a1_feedback_scanner",
  "scan_id": "scan-2024-04-15T08:00Z",
  "scanned_at": "2024-04-15T08:00:00Z",
  "window": { "since": "2024-04-14T08:00:00Z", "until": "2024-04-15T08:00:00Z" },
  "items": [
    {
      "feedback_id": "fbk-2024-04-15-9911",
      "feedback_url": "https://a1.example.com/feedback/9911",
      "title": "MR label sync 回退到上一次状态",
      "triage_artifact_uri": "loom://artifact/<sha256>",
      "severity": "major",
      "summary": "用户报告 MR label 在 sync 后又回到旧值。",
      "fix_status": "pending"
    }
  ]
}
```

`fix_status` ∈ `{ "pending", "in_progress", "fixed", "dropped" }`. The
scanner emits `pending` for newly-seen items and updates the same field
to `in_progress` / `fixed` / `dropped` on the next scan based on
existing thread / mr-merged signals (the loop annotates state by
publishing a follow-up `feedback-scan.bugs.v1` with adjusted statuses;
consumers always read the latest artifact in the thread).

Required: `schema_version`, `producer`, `scan_id`, `scanned_at`,
`window`, `items`. `items` may be empty (then the artifact is still
published so consumers can confirm the scan ran).

---

## 9. `feedback-scan.others.v1`

Producer: `service_a1_feedback_scanner`. Consumer: human (channel
public chat readers).

Companion to §8 covering everything that is **not** an existing-bug.
Designed to be human-skimmable and to support diff-against-previous-scan
display in the channel public chat.

```json
{
  "schema_version": "1",
  "producer": "service_a1_feedback_scanner",
  "scan_id": "scan-2024-04-15T08:00Z",
  "scanned_at": "2024-04-15T08:00:00Z",
  "window": { "since": "2024-04-14T08:00:00Z", "until": "2024-04-15T08:00:00Z" },
  "buckets": {
    "new_request": [ { "feedback_id": "fbk-…", "title": "…", "summary": "…" } ],
    "unclear":     [],
    "duplicate":   [],
    "not_actionable": []
  },
  "diff_vs_previous_scan": {
    "previous_scan_id": "scan-2024-04-14T08:00Z",
    "added":   [ { "feedback_id": "fbk-…", "bucket": "new_request" } ],
    "removed": [ { "feedback_id": "fbk-…", "bucket": "unclear" } ]
  }
}
```

When a scan has no diff and no items in any bucket the scanner still
publishes the artifact but skips the channel-public-chat announcement
(noise threshold). Required: `schema_version`, `producer`, `scan_id`,
`scanned_at`, `window`, `buckets`. `diff_vs_previous_scan` is omitted on
the very first scan.

---

## 10. `mr-status-diff.v1`

Producer: `svc_mr_detector` (replaces inline `status.update` payloads).
Consumer: `actor_delivery` (the bound delivery thread re-activates on
this artifact), `service_a1_bug_fix_loop`.

Emitted on each MR transition that is not "merged" / "closed". Replaces
the freeform `status.update` message previously used by mr-detector so
downstream loops can subscribe declaratively.

```json
{
  "schema_version": "1",
  "producer": "svc_mr_detector",
  "mr_id": "https://gitlab.alibaba-inc.com/aone/loom-apps/merge_requests/4271",
  "captured_at": "2024-04-15T09:14:02Z",
  "previous_state": {
    "ci_status": "running",
    "labels": ["wip"],
    "head_sha": "0a1b…",
    "open_comments": 0,
    "behind_target": false
  },
  "current_state": {
    "ci_status": "failed",
    "labels": ["wip", "ci-broken"],
    "head_sha": "0a1b…",
    "open_comments": 2,
    "behind_target": true
  },
  "delta": {
    "ci_status_changed": true,
    "ci_failed": true,
    "new_comments": 2,
    "behind_target_changed": true,
    "labels_added": ["ci-broken"],
    "labels_removed": []
  },
  "actionable_summary": "CI 红了；落后 origin/master，需要 rebase；2 条新评论",
  "raw_event_fingerprint": "sha256:…"
}
```

Required: `schema_version`, `producer`, `mr_id`, `captured_at`,
`current_state`, `delta`, `raw_event_fingerprint`. `previous_state` is
omitted on the first artifact for an MR. `actionable_summary` is a
short Chinese summary suitable for a `loom directed message` message body.

---

## 11. `mr-merged.v1`

Producer: `svc_mr_detector`. Consumer:
`service_a1_bug_fix_loop` (closes the loop iteration), `actor_router`
(announce to channel), human.

Terminal artifact: emitted exactly once per MR when the upstream state
transitions to `merged`. mr-detector also writes
`{"service.self_complete":true}` after this artifact so the
thread-bound service host stops the instance.

```json
{
  "schema_version": "1",
  "producer": "svc_mr_detector",
  "mr_id": "https://gitlab.alibaba-inc.com/aone/loom-apps/merge_requests/4271",
  "title": "fix: off-by-one in MR label sync",
  "head_sha": "0a1b…",
  "base_sha": "9f8e…",
  "author": "octocat",
  "labels": ["bugfix", "release-blocker"],
  "merged_at": "2024-04-15T09:31:15Z",
  "captured_at": "2024-04-15T09:31:16Z",
  "linked_feedback_ids": ["fbk-2024-04-15-9911"],
  "raw_event_fingerprint": "sha256:…"
}
```

Required: `schema_version`, `producer`, `mr_id`, `merged_at`,
`captured_at`, `raw_event_fingerprint`. `linked_feedback_ids` is
populated when the bound thread carried a `bug-triage.v1` referencing
specific feedback ids; otherwise omitted.

---

## 12. `training-plan.v1`

Producer: `actor_classmaster`. Consumer: `actor_teacher`.

Emitted in classroom when the human and classmaster have agreed on
which actor to train and what the target behavior is. The plan names
the actor under training, the success criteria (delegated to a regular
`definition-of-done.json`), and the expected lesson sequence.

```json
{
  "schema_version": "1",
  "producer": "actor_classmaster",
  "training_id": "training-2024-04-15-router-intake-v2",
  "target_actor": "actor_router",
  "target_bundle_version": "v2-draft",
  "summary": "router 入流分类对中英混合输入鲁棒性",
  "lesson_sequence": [
    "review 当前 SKILL.md",
    "出 5 道入流分类作业（中文/英文/中英混合各 2/2/1）",
    "teacher 评分；不通过则修订 SKILL.md 再出一轮",
    "通过后发布新 bundle"
  ],
  "dod_artifact_uri": "loom://artifact/<sha256>",
  "captured_at": "2024-04-15T10:00:00Z"
}
```

Required: `schema_version`, `producer`, `training_id`, `target_actor`,
`target_bundle_version`, `summary`, `lesson_sequence`, `captured_at`.
`dod_artifact_uri` is required-to-link if a separate
`definition-of-done.json` exists for the training; if the DoD is
inline-only it may be omitted.

---

## 13. `homework.v1`

Producer: `actor_teacher`. Consumer: `actor_teacher` (self, for
grading), human reviewer (read-only).

A homework set is one batch of test prompts that exercise the actor
under training. Each prompt is run against the candidate bundle and
the actor's reply is captured for grading. Homework is the input to
`grading-report.v1`.

```json
{
  "schema_version": "1",
  "producer": "actor_teacher",
  "homework_id": "hw-training-2024-04-15-router-intake-v2-r1",
  "training_id": "training-2024-04-15-router-intake-v2",
  "target_actor": "actor_router",
  "candidate_bundle_uri": "loom://bundle/actor_router/v2-draft",
  "items": [
    {
      "prompt_id": "p1",
      "input": "我要做一个新功能：把 router 拆成…（用户原文）",
      "expected_classification": "new_task",
      "actor_reply": "（teacher 在 dry-run 时填）",
      "actor_directed_target": "actor_discovery"
    }
  ],
  "captured_at": "2024-04-15T10:30:00Z"
}
```

Required: `schema_version`, `producer`, `homework_id`, `training_id`,
`target_actor`, `candidate_bundle_uri`, `items`, `captured_at`.
`actor_reply` and `actor_directed_target` are filled in by the teacher
after running the candidate bundle against each prompt.

---

## 14. `grading-report.v1`

Producer: `actor_teacher`. Consumer: `actor_classmaster` (decides
publish-or-revise), human.

Emitted once per homework run. Each item is graded `pass` / `fail` /
`partial` against `definition-of-done.json`. The report carries an
overall verdict that classmaster uses to gate `approval.spec_apply`.

```json
{
  "schema_version": "1",
  "producer": "actor_teacher",
  "report_id": "grade-hw-training-…-r1",
  "homework_id": "hw-training-2024-04-15-router-intake-v2-r1",
  "training_id": "training-2024-04-15-router-intake-v2",
  "verdict": "pass",
  "score": { "pass": 4, "partial": 1, "fail": 0, "total": 5 },
  "items": [
    {
      "prompt_id": "p1",
      "result": "pass",
      "reason": "正确分类为 new_task 并 directed message 给 actor_discovery"
    }
  ],
  "recommendation": "publish",
  "captured_at": "2024-04-15T11:00:00Z"
}
```

Allowed values:
- `verdict`: `"pass"` | `"fail"` | `"needs_revision"`.
- `result` (per item): `"pass"` | `"partial"` | `"fail"`.
- `recommendation`: `"publish"` | `"revise_skill_md"` | `"redo_homework"`.

Required: `schema_version`, `producer`, `report_id`, `homework_id`,
`training_id`, `verdict`, `score`, `items`, `recommendation`,
`captured_at`.

When `recommendation == "publish"`, classmaster MAY emit
`approval.spec_apply` to install the new bundle as `current` for the
target actor. When `verdict == "needs_revision"`, classmaster hands
off to teacher with the reason for the next iteration.

---

## 15. `actor-defect.v1`

Producer: `actor_classmaster` or an upstream production channel actor.
Consumer: `actor_teacher`.

Records why an actor needs training. This artifact should point at real
production evidence where possible, so classroom can be audited later.

```json
{
  "schema_version": "1",
  "producer": "actor_classmaster",
  "defect_id": "defect-delivery-multi-mr-watcher-2026-05-07",
  "target_actor": "actor_delivery",
  "severity": "p0",
  "summary": "delivery created two MRs but only one was registered for watcher",
  "observed_behavior": "aone/a1 MR CI failed without mr-watcher directed message",
  "expected_behavior": "every MR emits mr-opened.v1 and a [mr-opened v1] block",
  "evidence": [
    {
      "kind": "thread",
      "uri": "loom://thread/thread_9fee4a9462a6",
      "note": "delivery task for base image OpenAPI registration"
    }
  ],
  "captured_at": "2026-05-07T10:00:00Z"
}
```

Required: `schema_version`, `producer`, `defect_id`, `target_actor`,
`severity`, `summary`, `observed_behavior`, `expected_behavior`,
`captured_at`.

Allowed values:
- `severity`: `"p0"` | `"p1"` | `"p2"` | `"p3"`.

---

## 16. `training-record.v1`

Producer: `actor_teacher` or `actor_classmaster`. Consumer: human,
classmaster, future classroom runs.

Indexes the complete classroom loop for one training attempt. It does not
replace `training-plan.v1`, `homework.v1`, `grading-report.v1`, or
`lesson-plan.md`; it links them together and records the publish result.

```json
{
  "schema_version": "1",
  "producer": "actor_teacher",
  "training_id": "training-delivery-multi-mr-5c9e3b2a",
  "target_actor": "actor_delivery",
  "mode": "shadow",
  "input_artifacts": [
    "loom://artifact/<actor-defect>",
    "loom://artifact/<training-plan>",
    "loom://artifact/<definition-of-done>"
  ],
  "output_artifacts": [
    "loom://artifact/<homework>",
    "loom://artifact/<grading-report>",
    "loom://artifact/<lesson-plan>"
  ],
  "verdict": "pass",
  "recommendation": "publish",
  "published": false,
  "summary": "候选 bundle 已覆盖多 MR 注册、directed message router 和工作区隔离。",
  "risks": [
    "shadow 模式尚未替换生产 actor_delivery"
  ],
  "captured_at": "2026-05-07T11:00:00Z"
}
```

Required: `schema_version`, `producer`, `training_id`, `target_actor`,
`mode`, `input_artifacts`, `output_artifacts`, `verdict`,
`recommendation`, `published`, `summary`, `captured_at`.

Allowed values:
- `mode`: `"shadow"` | `"controlled"` | `"incident-intake"`.
- `verdict` and `recommendation`: same as `grading-report.v1`.

---

## Versioning

Add new optional fields at any time without bumping `schema_version`.
A `schema_version` bump means: at least one previously-required field
changed shape, became optional, was renamed, or removed. Producers
SHOULD continue publishing the old version alongside for one release
cycle and consumers SHOULD accept both.
