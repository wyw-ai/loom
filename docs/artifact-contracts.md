# Artifact Contracts

Artifacts are immutable payloads published into Loom and referenced from
messages, tasks, assignments, or run traces. This document only defines generic
public conventions. Product-specific artifact schemas should live in the
application or integration repository that owns that workflow.

## Conventions

- Artifacts SHOULD be JSON unless the schema explicitly names another format.
- File names SHOULD use kebab-case, for example `task-goal.json`.
- Times MUST be RFC 3339 UTC strings.
- Every artifact SHOULD include `schema`, `schemaVersion`, `producer`,
  `createdAt`, and enough stable identifiers for downstream consumers to link
  it back to the source task, message, or external event.
- Producers MAY add optional fields. Consumers MUST ignore unknown fields.
- Artifacts are immutable. To update an artifact, publish a new artifact and
  emit a new Loom event that references it.
- Consumers MUST NOT depend on private workspace paths. Shared data must be
  addressed by artifact id, artifact URI, or an explicit external reference.

## Generic Shapes

### `task-goal.json`

Describes what the user or coordinator wants to accomplish.

```json
{
  "schema": "task-goal",
  "schemaVersion": "1",
  "producer": "actor_planner",
  "taskId": "task_123",
  "title": "Update the onboarding flow",
  "narrative": "Explain the requested change and why it matters.",
  "scope": {
    "channelId": "chan_123",
    "threadId": "thread_123"
  },
  "outOfScope": ["do not change billing behavior"],
  "links": [
    { "kind": "issue", "url": "https://example.com/issues/123" }
  ],
  "createdAt": "2026-06-04T00:00:00Z"
}
```

Required: `schema`, `schemaVersion`, `producer`, `taskId`, `title`,
`narrative`, `createdAt`.

### `definition-of-done.json`

Defines falsifiable completion criteria.

```json
{
  "schema": "definition-of-done",
  "schemaVersion": "1",
  "producer": "actor_planner",
  "taskId": "task_123",
  "criteria": [
    {
      "id": "dod-1",
      "must": "all required checks pass",
      "verify": "run the project test command"
    }
  ],
  "createdAt": "2026-06-04T00:00:00Z"
}
```

Required: `schema`, `schemaVersion`, `producer`, `taskId`, `criteria[]`.

### `workspace-manifest.json`

Describes external resources or workspace mounts needed by a task.

```json
{
  "schema": "workspace-manifest",
  "schemaVersion": "1",
  "producer": "actor_planner",
  "taskId": "task_123",
  "resources": [
    {
      "id": "repo-main",
      "kind": "git",
      "uri": "https://github.com/example/project.git",
      "ref": "main",
      "mount": "repos/project",
      "readonly": false
    }
  ],
  "createdAt": "2026-06-04T00:00:00Z"
}
```

Required per resource: `id`, `kind`, `uri`.

### `validation-report.json`

Records the evidence gathered while validating a task or assignment.

```json
{
  "schema": "validation-report",
  "schemaVersion": "1",
  "producer": "actor_verifier",
  "taskId": "task_123",
  "definitionOfDoneArtifact": "artifact://chan_123/artifact_abc",
  "results": [
    {
      "criterionId": "dod-1",
      "status": "pass",
      "evidence": ["test command exited with code 0"]
    }
  ],
  "createdAt": "2026-06-04T00:00:00Z"
}
```

Allowed result statuses: `pass`, `fail`, `skip`, `unknown`.

### `external-event.json`

Normalizes an event received from an external system.

```json
{
  "schema": "external-event",
  "schemaVersion": "1",
  "producer": "svc_webhook",
  "source": {
    "kind": "webhook",
    "id": "build-system",
    "eventId": "evt_123"
  },
  "summary": "Build completed",
  "occurredAt": "2026-06-04T00:00:00Z",
  "capturedAt": "2026-06-04T00:00:02Z",
  "dedupeKey": "build-system:evt_123",
  "payloadArtifact": "artifact://chan_123/artifact_payload"
}
```

Use this shape when a service wants to preserve a stable, inspectable copy of
an external fact without embedding product-specific fields into Loom core.

## Versioning

`schemaVersion` changes only when a required field is removed, renamed, or
given incompatible semantics. Additive optional fields do not require a version
bump.
