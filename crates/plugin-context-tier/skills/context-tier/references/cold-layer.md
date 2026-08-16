# Cold Layer — Historical Retrieval

The Cold layer is the complete, permanent message history stored on the Loom
server. It does not enter the model context automatically — you retrieve it
on demand using CLI commands.

## Definition

The Cold layer encompasses all messages, tasks, artifacts, and assignments
ever created in a scope, stored permanently on the Loom server. Unlike the
Hot layer (single-turn) and Warm layer (compressed summary), the Cold layer
is never automatically injected into the prompt.

**Access method**: CLI commands (`loom message read`, `loom message search`)

**Code locations**:
- Message read: `message.rs` (server-side read handler)
- Message search: `message.rs` (server-side search handler)

## When to use Cold layer retrieval

| Situation | Action |
| --- | --- |
| You need messages from before a session reset | `message read --before` |
| You need to find a specific topic or decision | `message search --query` |
| You need the full text of a truncated message | `message get <msgId>` |
| You need to review task history | `task list`, `task show` |
| You need to verify what was previously agreed | `message read` + `message search` |

## CLI Commands

### Read messages

```bash
# Read recent messages in a thread
loom --json message read --target "#chan_xxx:msg_yyy"

# Read messages before a specific message (backward pagination)
loom --json message read --target "#chan_xxx:msg_yyy" --before msg_zzz

# Include private messages addressed to you
loom --json message read --target "#chan_xxx:msg_yyy" --include-private
```

### Search messages

```bash
# Search by keyword in a specific thread
loom --json message search --query "deployment" --target "#chan_xxx:msg_yyy"

# Search across a channel
loom --json message search --query "architecture" --target "#chan_xxx"
```

### Get a specific message

```bash
# Get full message body (useful for truncated messages)
loom --json message get msg_abc123
```

### List and inspect tasks

```bash
# List tasks anchored to a message
loom --json task list --source-message msg_abc123

# Show task details
loom --json task show task_xyz789
```

## Pagination Pattern

Message history can be large. Use `--before` for backward pagination:

```bash
# Page 1: Most recent messages
loom --json message read --target "#chan_xxx:msg_yyy"
# → Returns messages with ids: [..., msg_005, msg_006, msg_007]

# Page 2: Messages before the oldest in page 1
loom --json message read --target "#chan_xxx:msg_yyy" --before msg_005
# → Returns messages with ids: [..., msg_001, msg_002, msg_003, msg_004]

# Continue until no more messages are returned
```

## Search Strategy

### Effective search queries

| Query type | Example | When to use |
| --- | --- | --- |
| Keyword | `"deployment"` | Find discussions about a topic |
| Decision | `"APPROVE"` or `"approved"` | Find approval decisions |
| Actor + action | `"@ARCH"` | Find messages from a specific actor |
| Artifact reference | `"art_"` | Find messages referencing artifacts |
| Task reference | `"task_"` | Find messages about tasks |

### Search scope

Always provide `--target` to scope your search:

```bash
# Search in a specific thread (recommended)
loom --json message search --query "keyword" --target "#chan_xxx:msg_yyy"

# Search in a channel (broader)
loom --json message search --query "keyword" --target "#chan_xxx"
```

Without `--target`, search may scan all accessible scopes, which can be slow.

## Relationship to Hot and Warm

```
Full history (Cold)
  ↓ filtered by delivery cursor
Recent messages (Hot visible history sample)
  ↓ compressed on session reset
Summary (Warm)
```

| Layer | What it contains | How to access |
| --- | --- | --- |
| **Hot** | Current turn + recent history sample | Automatically in prompt |
| **Warm** | Compressed summary of prior turns | Automatically in prompt (after reset) |
| **Cold** | Complete permanent history | CLI retrieval only |

### When Hot/Warm is not enough

- **After a session reset**: The Warm summary may not include specific
  details you need. Use Cold retrieval to fetch the original messages.
- **Cross-thread references**: If work spans multiple threads, use Cold
  search to find related discussions.
- **Verification**: Before making a claim about what was decided, verify
  with Cold retrieval rather than relying on memory or summary.

## Practical guidance

- **Read before deciding.** For stateful choices (votes, approvals, reviews),
  read enough current conversation before answering. Do not rely only on the
  latest wake if prior messages determine the choice.
- **Use `--json` for structured output.** The human-readable renderer may
  hide attachment ids and metadata. Use `--json` when you need to parse
  message structure.
- **Search with specific terms.** Generic queries return too many results.
  Use specific keywords, actor ids, or artifact ids.
- **Paginate systematically.** Use `--before` with the oldest message id from
  the previous page to walk backward through history.
