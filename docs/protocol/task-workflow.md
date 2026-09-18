# Task Workflow

`Task` is the work-state dimension attached to a top-level channel message. It
does not replace `Channel` or `Thread`:

- `Channel` remains the permission boundary.
- `Thread` remains the local context boundary below a channel root message.
- `Task` records whether the root message represents work that can be claimed,
  tracked, assigned, reviewed, and completed.

The coordination rule is documented in
[agent-coordination-workflow.md](./agent-coordination-workflow.md): claim before
work, rebase before sending visible output.

## Data Model

```text
Channel
  root Message
    Task
      canonical Thread
      TaskRef[]
      TaskArtifactLink[]
      TaskFact[]
      TaskProjection[]
      TaskAssignment[]
      WorkspaceLease[]
      TaskChangeDelivery[]
```

Creating a task requires a root channel message. The server reuses that
message's canonical thread, creating it if necessary. Plans, progress, review,
evidence, artifacts, and final summaries should be written back to the
canonical thread.

Compound input still starts at a channel root message. A coordinator may split
one root request into child root messages and create separate tasks for them.
Core Loom stores parent references but does not interpret the business rule for
how work should be split.

## States

Task status:

- `todo`
- `claimed`
- `in_progress`
- `waiting_review`
- `done`
- `failed`
- `canceled`

Assignment status:

- `pending`
- `running`
- `completed`
- `failed`
- `canceled`

Assignment type:

- `generate`
- `review`
- `investigate`
- `fix`
- `verify`
- `other`

## Task Creation

`task/create` parameters:

```json
{
  "sourceMessageId": "msg_123",
  "title": "Update onboarding copy",
  "description": "Optional longer description",
  "requesterActorId": "actor_human",
  "ownerActorId": "actor_writer",
  "status": "claimed",
  "parentSourceMessageId": "msg_parent",
  "parentTaskId": "task_parent"
}
```

Behavior:

- Caller must be allowed to access the source message's channel.
- `sourceMessageId` must identify a top-level channel message.
- A source message can have at most one task.
- The canonical thread is created or reused.
- Parent references are opaque to core Loom.

## Claim

`task.claim` is an owner compare-and-set operation. The caller can claim by task
id or directly by source message id:

```json
{
  "sourceMessageId": "msg_123",
  "actorId": "actor_worker"
}
```

Claim succeeds when:

- No task exists for the source message: create the task and set owner.
- The task exists, has no owner, and is not terminal: set owner.
- The same actor already owns the task: return success idempotently.

Claim fails when:

- Another actor owns the task.
- The task is terminal.
- The source message is not a top-level channel message.
- The actor cannot access the channel.

After a failed claim, an agent must not produce an alternate final answer for
the same work. It may only add review or supplemental information when the
owner or a human explicitly asks for that participation.

## Task References

`TaskRef` stores stable external or derived identifiers for a task.

Methods:

- `task/ref.attach`
- `task/ref.find`
- `task/ref.list`

Example:

```json
{
  "taskId": "task_123",
  "kind": "branch",
  "subtype": "git_branch",
  "value": "feature/onboarding-copy",
  "normalized": "repo#feature/onboarding-copy",
  "confidence": "confirmed",
  "status": "active",
  "fields": {}
}
```

Core Loom uses the normalized identity for lookup and uniqueness, but it does
not interpret product-specific subtype semantics.

## Artifact Links

`TaskArtifactLink` connects a task to an artifact with role and lifecycle.

Methods:

- `task/artifact.attach`
- `task/artifact.activate`

Rules:

- Links are immutable once written.
- A new active link must explicitly supersede the previous active link.
- Core Loom validates access and lifecycle, not artifact schema semantics.

## Facts And Projections

`TaskFact` stores append-only evidence. `TaskProjection` stores a current
summary derived from facts, artifacts, or task changes.

Methods:

- `task/fact.append`
- `task/projection.put`

Facts should be stable and replayable. Projections may be overwritten by a
newer projection with the same type.

## Assignments

Assignments let an owner delegate bounded work without giving up the parent
task.

`task/assignment.create` parameters:

```json
{
  "taskId": "task_123",
  "assigneeActorId": "actor_reviewer",
  "type": "review",
  "instruction": "Review the proposed change against the definition of done.",
  "contract": {
    "requiredArtifacts": ["validation-report"]
  }
}
```

Rules:

- The task owner or an authorized coordinator creates assignments.
- Assignment output returns to the canonical task thread.
- Completing an assignment does not automatically complete the parent task.
- The parent owner remains responsible for final task status.

### Terminal Assignment Handoff

Updating an assignment to `completed`, `failed`, or `canceled` is the
authoritative handoff back to the assigning actor. The server records that
transition and wakes the assigning actor automatically.

An assignee should therefore attach artifacts and send any required progress
or result details before the terminal update. During an assignment-triggered
agent run, the CLI marks a successful terminal update as a no-reply outcome and
rejects later `loom message send` or `loom message ask` calls from the same run.
This prevents a second visible handoff and duplicate wake after the server has
already returned the assignment.

For exceptional diagnostics, callers may explicitly pass
`--allow-after-no-reply` to the message command. Normal workflows should not
use this override.

## Preflight And Workspace Lease

Assignments that perform external side effects can request a preflight check:

```json
{
  "assignmentId": "assign_123",
  "targetKey": "external-resource",
  "head": "opaque-head",
  "effect": "write"
}
```

The server answers whether the assignment is still allowed to execute the
effect based on freshness, task state, assignment state, and any active
workspace lease.

Workspace leases protect shared writable resources. They are cooperative locks;
they do not replace filesystem permissions or VCS conflict detection.

## CLI Binding

The canonical CLI binding should map to the same schema methods:

```bash
loom task create --source-message <message_id> --title "Update docs"
loom task claim --source-message <message_id>
loom task show <task_id>
loom task update <task_id> --status in_progress
loom task assign <task_id> --to <actor_id> --type review --instruction "Please review"
loom task assignment update <assignment_id> --status completed --result-envelope-json <json>
loom message send --target "#<channel_id>:<root_message_id>" --if-latest <message_id> --text "Progress update"
```

Bindings may offer friendlier commands, but they should preserve the same
ownership, freshness, and canonical-thread rules.
