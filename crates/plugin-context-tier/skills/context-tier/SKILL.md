---
name: context-tier
description: Loom Hot/Warm/Cold three-tier context lifecycle management — session reset, warm summary, and historical retrieval operations.
version: 1.0.0
---

# Context Tier — Hot/Warm/Cold Temperature Model

Loom manages context through a three-tier temperature model. Each tier has a
different lifecycle, storage location, and access pattern. Understanding this
model helps you work effectively across session resets and retrieve history
when needed.

## When to use

- You see a `warm_summary` section in your prompt and want to understand what
  happened.
- You need to retrieve older messages that are no longer in your prompt.
- You want to understand how Loom keeps context bounded in long conversations.
- You need to search historical conversation by keyword.

## Three-Tier Model Overview

| Tier | Lifecycle | Storage | What it contains |
| --- | --- | --- | --- |
| **Hot** | Single turn | Prompt envelope (in-memory) | Current turn's delivery context, inbox, trigger message |
| **Warm** | Cross-turn (until next reset) | `{profile_dir}/summaries/{scope_id}.md` | Compressed summary of prior conversation |
| **Cold** | Permanent (server-side) | Loom server database | Full message history, tasks, artifacts |

```
┌─────────────────────────────────────────────────────┐
│                    COLD (permanent)                   │
│   Full history — message read / message search CLI    │
│   ┌───────────────────────────────────────────────┐  │
│   │              WARM (cross-turn summary)          │  │
│   │   summaries/{scope}.md — injected after reset  │  │
│   │   ┌─────────────────────────────────────────┐  │  │
│   │   │          HOT (current turn)              │  │  │
│   │   │   Delivery context + trigger message     │  │  │
│   │   └─────────────────────────────────────────┘  │  │
│   └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

## Scenario Routing

| Scenario | Read |
| --- | --- |
| You need to understand the current turn's delivery context, inbox, or message assembly. | `references/hot-layer.md` |
| You experienced a session reset, see a `warm_summary`, or need to understand the reset lifecycle. | `references/warm-layer.md` |
| You need to retrieve old messages, search history, or read beyond the prompt window. | `references/cold-layer.md` |

## Core Operations

### Hot Layer — Current Turn Context

The Hot layer is what you see in your prompt right now. It includes:
- The trigger message (the message that woke you)
- Delivery context (pending inbox, wake entries)
- Current scope information (actor, channel, thread)

You do not manage the Hot layer — it is assembled automatically each turn.

### Warm Layer — Session Reset

When a scope's conversation grows long, Loom automatically:
1. Generates a summary of the conversation so far
2. Persists it to `summaries/{scope_id}.md`
3. Resets the provider session
4. Injects the summary into subsequent turns

**If you see a `warm_summary` section in your prompt**: The session was reset.
This is normal. Continue working from the summary — it contains the essential
context from prior turns.

See `references/warm-layer.md` for the full 6-step reset lifecycle.

### Cold Layer — Historical Retrieval

The Cold layer is the complete message history stored on the Loom server. It
does not enter the model context automatically. Use CLI commands to retrieve
it on demand:

```bash
# Read recent messages in a thread
loom message read --target "#chan_xxx:msg_yyy"

# Read messages before a specific message (pagination)
loom message read --target "#chan_xxx:msg_yyy" --before msg_zzz

# Search by keyword
loom message search --query "deployment" --target "#chan_xxx:msg_yyy"
```

See `references/cold-layer.md` for full retrieval patterns.

## Configuration

The temperature model is configured through `agentcontext.json` and the
default resource chain (`default_agent_context_spec()`):

| Resource | Priority | Tier | Description |
| --- | --- | --- | --- |
| `warm-summary` | 7 | Warm | Reads `{profile}/summaries/{scope}.md` and injects as prompt section |
| `message-list` | 10 | Hot | Wraps delivery cursor and injects current turn context |

Cold layer needs no ContextResource — it is accessed through CLI commands.

See `context-resources/default-agentcontext.json` for a configuration template.

## What agents should know

- **You do not manage temperature mechanics.** Session reset, summary
  generation, and resource chain assembly are runtime-internal.
- **A `warm_summary` section is normal.** It means the session was reset to
  keep context bounded. Continue working from the summary.
- **A summary-generation turn produces no reply.** If your turn feels like it
  was consumed by an internal lifecycle event, the next turn will deliver the
  actual work.
- **Use Cold layer retrieval when context is missing.** If you need
  information from before a session reset, use `message read` and
  `message search` to retrieve it.
- **Do not edit `summaries/` files.** They are managed by the runtime and
  regenerated as needed.
