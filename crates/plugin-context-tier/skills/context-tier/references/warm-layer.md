# Warm Layer — Cross-Turn Summary

The Warm layer stores compressed summaries of conversation history, allowing
the provider session to be reset without losing essential context. It bridges
the gap between the ephemeral Hot layer and the permanent Cold layer.

## Definition

The Warm layer is a markdown summary of the conversation in a specific scope,
stored at:

```
{profile_dir}/summaries/{scope_id}.md
```

- One file per scope (thread or channel).
- Atomic writes (temp file + rename) ensure crash safety.
- Survives across daemon restarts.
- Managed by the runtime — do not edit or delete manually.

## Session Reset Lifecycle (6 Steps)

When a scope's conversation exceeds reset thresholds, Loom executes a 6-step
session reset:

### Step 1: Trigger Detection

Before each turn (never on the first turn), Loom evaluates reset conditions:

| Condition | Threshold |
| --- | --- |
| Token usage exceeds budget | `> 80%` of the wake-context token budget |
| Message count exceeds hard limit | `> 50` messages in the scope |
| Message count below minimum | `< 20` messages — reset **never** triggers |

The token budget reuses the existing `wake_context_token_budget` (default
900, configurable per agent).

**Code location**: `agent_serve.rs` (reset evaluation before turn dispatch)

### Step 2: Summary Generation

Loom composes a **summary-generation prompt** (not the normal turn prompt)
using a 5-section template and sends it to the provider. The provider
generates a summary of the conversation so far.

The summary-generation turn produces **no visible reply** — it is an internal
lifecycle event.

**Code location**: `agent_serve.rs` (summary prompt composition and dispatch)

### Step 3: Persistence

The generated summary text is persisted to:

```
{profile_dir}/summaries/{scope_id}.md
```

Written atomically: write to temp file, then rename to final path. This
ensures crash safety — either the complete summary is written or nothing.

**Code location**: `agent_serve.rs` / summary persistence logic

### Step 4: Session Reset

The adapter session for that scope is reset:

- **ACP adapter**: The scope→session mapping is removed. A new session is
  created lazily on the next turn.
- **Interactive adapter**: The session file is deleted.

**Code location**: `agent_serve.rs` (adapter session reset)

### Step 5: Tracking Reset

Internal tracking state for the scope is reset:
- Delivery cursor positions
- Processed message markers
- Turn counters

This ensures the reset is "clean" — the next turn starts fresh.

**Code location**: `agent_serve.rs` (tracking state reset)

### Step 6: Re-Queue Pending Triggers

The original trigger message (the one that would have been processed) is
re-queued. It will be processed in the next turn, with the Warm summary
injected into the prompt envelope.

This ensures no work is lost — the trigger message is held and processed
immediately after the reset.

**Code location**: `agent_serve.rs` (trigger re-queue logic)

## Complete Reset Timeline

```
Turn N: Threshold met (token > 80% or messages > 50)
  ↓
Step 1: Detect threshold
  ↓
Step 2: Send summary-generation prompt → provider generates summary
  ↓
Step 3: Persist summary to summaries/{scope_id}.md (atomic write)
  ↓
Step 4: Reset adapter session (remove session mapping / delete session file)
  ↓
Step 5: Reset tracking state (cursor, markers, counters)
  ↓
Step 6: Re-queue original trigger message
  ↓
Turn N+1: Fresh session, Warm summary injected, trigger message processed
```

## What the agent sees

After a session reset, the prompt envelope includes a `warm_summary` section
(positioned between `runtime_context` and the user message):

```
[profile_prompt_files]
[runtime_context]
[bootstrap_memory]
[warm_summary]              ← Warm layer: summary of prior conversation
[delivery_context]          ← Hot layer: current turn
[user_message]
```

The Warm summary replaces the full conversation history that the provider
would otherwise re-process. The agent experience is transparent: the summary
provides enough context to continue the workflow.

### Budget allocation

The Warm summary is allocated **20% of the remaining budget**:

```
remaining_budget = total_budget - fixed_sections
warm_summary_budget = remaining_budget * 0.2
```

If the summary exceeds this allocation, it is **skipped** (not truncated, per
C-3 constraint). A resource with `priority: 0` is always included regardless
of budget.

## WarmSummaryContextResource

The Warm layer is injected through `WarmSummaryContextResource`:

| Property | Value |
| --- | --- |
| Scheme | `warm-summary` |
| Priority | 7 |
| Mount | `warm` |
| Budget share | 20% of remaining |

**Code location**: `context_layer/warm_summary.rs`

### Assembly flow

```
compose_with_context_chain()
  ↓
WarmSummaryContextResource.assemble()
  → reads {profile_dir}/summaries/{scope_id}.md
  ↓
if file exists:
  → format as PromptSection
  → allocate 20% of remaining budget
  → inject into prompt envelope
if file does not exist:
  → skip (no warm summary for this scope yet)
```

## Practical guidance

- **If you see a `warm_summary` section**: The session was reset. This is
  expected behavior — continue working from the summary.
- **The summary is not exhaustive.** It captures key decisions, outcomes, and
  context. For detailed history, use Cold layer retrieval.
- **Do not edit `summaries/` files.** They are runtime-managed and will be
  regenerated on the next reset.
- **A summary-generation turn produces no reply.** If your turn feels
  consumed by an internal event with no trigger to act on, the next turn
  will deliver the actual work.
