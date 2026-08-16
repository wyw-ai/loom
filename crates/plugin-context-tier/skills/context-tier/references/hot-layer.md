# Hot Layer — Current Turn Context

The Hot layer is the live, in-memory context assembled for the current turn.
It has a single-turn lifecycle: it exists only for the duration of one
provider call and is not persisted.

## Definition

The Hot layer contains everything in the current turn's prompt envelope that
is not Warm summary or Cold retrieval:

- **Trigger message**: The message that woke the agent
- **Delivery context**: Pending inbox entries, wake entries, routed messages
- **Runtime context**: Actor id, channel id, scope id, environment variables
- **Profile prompt files**: AGENTS.md and other stable instruction files

## Data Assembly Chain

The Hot layer is assembled through this chain:

```
agent_serve.rs (turn entry)
  ↓
delivery_cursor_context()
  → determines which messages to deliver this turn
  ↓
bootstrap_conversation_context()
  → assembles visible history sample
  ↓
pending_delivery_context()
  → collects pending inbox and wake entries
  ↓
MessageListProvider.assemble()  (priority=10)
  → formats as PromptSection
  ↓
injected into prompt envelope
```

### Code locations

| Component | File |
| --- | --- |
| Turn entry / orchestration | `agent_serve.rs` |
| MessageListProvider | `context_layer/message_list.rs` |
| Delivery cursor logic | `agent_serve.rs` |

## Lifecycle

```
Turn starts
  ↓
Hot layer assembled (delivery cursor + inbox + trigger)
  ↓
Provider processes the turn
  ↓
Agent produces a response
  ↓
Hot layer discarded (not persisted)
  ↓
Next turn: fresh Hot layer assembled
```

The Hot layer is **never persisted**. Each turn gets a fresh assembly based
on the current delivery cursor state.

## What the agent sees

The Hot layer appears in the prompt as:

```
=== System: Local time context ===
...

=== User message ===
New message:
[unread] timestamp Actor[id=...][msgId=...]:
<message body>

Scope #...: N message(s) delivered this turn, M more pending.
...

=== Loom visible history sample ===
<prior messages as context>
```

### Message delivery

The delivery cursor determines which messages are delivered:

| Source | Description |
| --- | --- |
| `wake[]` entries | Primary work for this turn — messages routed to this actor |
| `Loom pending inbox` | Same-scope unread context included without manual inbox read |
| Visible history sample | Recent messages for context (limited by token budget) |

### Token budget

The Hot layer (via `message-list` resource at priority=10) consumes part of
the remaining token budget after fixed sections and higher-priority resources
(memory at 5, warm-summary at 7). If the delivery context exceeds the
remaining budget, the entire section is **skipped** (not truncated).
Resources that exceed the remaining budget are skipped entirely, not
partially included.

## Interaction with Warm and Cold

| Interaction | Description |
| --- | --- |
| **Hot ← Warm** | After a session reset, the Warm summary replaces the full conversation history that would normally be in the Hot layer's visible history sample. |
| **Hot ← Cold** | The Hot layer contains only recent messages. For older history, use Cold layer CLI retrieval (`message read --before`, `message search`). |
| **Hot → Warm** | When the Hot layer grows too large (triggering thresholds), the Warm layer is generated from the accumulated Hot layer content. |

## Practical guidance

- **The trigger message is your primary work.** The `wake[]` entries in the
  USER message are what you must handle this turn.
- **Visible history is context, not new work.** Messages in the history
  sample are for context only — do not re-process them as new requests.
- **If context seems incomplete**, use Cold layer retrieval to fetch older
  messages rather than guessing.
