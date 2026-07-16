# Channel/Thread Instructions

## Background

Loom agents receive their identity and operating rules through an
`AGENTS.md` bootstrap block injected into their workspace at turn start.
Before this feature, the only instruction channel was the **actor-level**
`AgentSpec.instructions` field — every agent in every channel/thread got
the same system prompt regardless of what it was working on.

In multi-channel setups (e.g. one channel for spec maintenance, another
for bug-fix loops) this was insufficient: agents lacked scope-specific
context such as "this channel maintains the Obsidian vault at path X" or
"this thread is a P0 hotfix, skip non-essential ceremony."

Channel/Thread Instructions solve this by adding two new optional
free-text fields — `Channel.instructions` and `Thread.instructions` —
that are projected into every member agent's `AGENTS.md` block at turn
start, layered on top of the agent's own identity instructions.

> **Design authority**: ARCH short-term plan (`loom-channel-agent-skill-design/ARCH-短期落地方案-channel-agent与skill入口设计.md`)
> **Norms reference**: STRAT long-term architecture (`loom-longterm-arch-design/05-workspace-attribute.md`)
> **Implementation**: commit `627620c` on branch `feat/channel-thread-instructions`

---

## How It Works

### Instruction priority chain

When an agent starts a turn, Loom assembles the `AGENTS.md` block by
concatenating instruction layers in a fixed order:

```
1. Loom runtime bootstrap     (actor id, channel info, operating rules)
2. Stable agent instructions   (actor-level AgentSpec.instructions)
3. Channel instructions        (NEW — Channel.instructions, if set)
4. Thread instructions         (NEW — Thread.instructions, if set, thread scope only)
```

Each layer is appended as a separate `## ` section inside the
`<!-- BEGIN loom --> ... <!-- END loom -->` block. Later layers do not
overwrite earlier ones — they are additive. This means an agent receives
its identity (layer 2), then any channel-wide constraints (layer 3),
then any thread-specific guidance (layer 4), all in a single prompt
prefix.

### AGENTS.md block structure (with instructions)

```markdown
<!-- BEGIN loom -->
# Loom runtime bootstrap
...
## Stable agent instructions
<actor-level instructions>

## Channel instructions
<channel-level instructions, if set>

## Thread instructions
<thread-level instructions, if set, thread scope only>
<!-- END loom -->
```

When a field is `None` or empty/whitespace-only, its section is omitted
entirely — no empty headers appear in the output.

### Scope behavior

| Scope | Channel instructions | Thread instructions |
|-------|---------------------|---------------------|
| Channel-level turn | ✅ injected | ❌ not injected |
| Thread-level turn | ✅ injected | ✅ injected |

Thread instructions are only fetched when the current scope is a thread
(`ScopeKind::Thread`). In channel-level turns, only channel instructions
apply.

---

## CLI Usage

### Channel instructions

```bash
# Set from inline text
loom channel set-instruction <channel_id> --text "This channel maintains the spec document..."

# Set from a file
loom channel set-instruction <channel_id> --file ./channel-rules.md

# View current instructions (prints "(none)" if unset)
loom channel get-instruction <channel_id>

# Clear instructions
loom channel clear-instruction <channel_id>
```

**Flags for `set-instruction`:**
- `--file <path>` — read instruction text from a file
- `--text <string>` — provide instruction text inline
- Exactly one of `--file` or `--text` is required; passing both is an error

### Thread instructions

```bash
# Set from inline text
loom thread set-instruction <thread_id> --text "This thread is a P0 hotfix..."

# Set from a file
loom thread set-instruction <thread_id> --file ./thread-context.md

# View current instructions
loom thread get-instruction <thread_id>

# Clear instructions
loom thread clear-instruction <thread_id>
```

### JSON output

All commands support `--json` for machine-readable output:

```bash
loom --json channel get-instruction <channel_id>
# → {"instructions": "This channel maintains..."}

loom --json channel clear-instruction <channel_id>
# → {"cleared": true}
```

### Practical examples

```bash
# Declare a channel for Obsidian vault maintenance
loom channel set-instruction chan_spec01 --text \
  "This channel maintains the Obsidian vault at C:/Users/hansi/OneDrive/note/ai_note/ai-note/. All agents must use Obsidian-flavored Markdown (wikilinks, callouts, frontmatter)."

# Give a bug-fix thread specific urgency context
loom thread set-instruction thread_bugfix42 --text \
  "P0 hotfix: production crash in auth module. Skip non-essential ceremony. Direct fixes only."

# Check what's set
loom channel get-instruction chan_spec01
loom thread get-instruction thread_bugfix42

# Remove when no longer needed
loom thread clear-instruction thread_bugfix42
```

---

## Data Model

### Channel struct (`proto/src/types.rs`)

```rust
pub struct Channel {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub topic: String,
    pub members: Vec<String>,
    /// Channel-level instructions projected into every member agent's
    /// AGENTS.md. Use this to declare the channel's purpose, rules, and
    /// shared context conventions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    // ... other fields
}
```

### Thread struct (`proto/src/types.rs`)

```rust
pub struct Thread {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub root_message_id: String,
    /// Thread-level instructions. When present, these are appended after
    /// channel instructions in the AGENTS.md block, giving the thread
    /// scope-specific guidance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    // ... other fields
}
```

Both fields are `Option<String>` with `#[serde(default)]`, meaning:
- **No size limit** on instruction text (per Founder directive)
- **Backward compatible** — old journals without the field deserialize
  as `None` with no migration needed
- **Skipped in serialization** when `None` — keeps journal output clean

---

## Backward Compatibility

The `instructions` field uses `#[serde(default)]` on both `Channel` and
`Thread` structs. This provides full backward compatibility:

| Scenario | Behavior |
|----------|----------|
| Old journal (pre-feature) | Deserializes with `instructions: None` — no error, no migration |
| New journal on old binary | Old binary ignores the unknown field — no error |
| Empty/whitespace instruction | Normalized to `None` at the store layer — no empty sections in AGENTS.md |

No journal migration is required. The feature is purely additive.

---

## JSON-RPC Protocol

Six new methods were added to the server protocol:

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `channel/set_instruction` | `{ channelId, instructions }` | `{ channel }` | Set/replace channel instructions |
| `channel/get_instruction` | `{ channelId }` | `{ instructions }` | Read channel instructions |
| `channel/clear_instruction` | `{ channelId }` | `{ cleared }` | Clear channel instructions |
| `thread/set_instruction` | `{ threadId, instructions }` | `{ thread }` | Set/replace thread instructions |
| `thread/get_instruction` | `{ threadId }` | `{ instructions }` | Read thread instructions |
| `thread/clear_instruction` | `{ threadId }` | `{ cleared }` | Clear thread instructions |

### Access control

All six handlers enforce **channel membership gating**: the calling
actor must be a member of the channel (for channel instructions) or the
channel that owns the thread (for thread instructions). Non-members
receive an `APP_INVALID_STATE` error.

---

## Journal Persistence

Two new `Mutation` variants were added to the journal:

```rust
pub enum Mutation {
    // ...
    ChannelInstructionSet {
        channel_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    ThreadInstructionSet {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    // ...
}
```

Both are **idempotent on replay** — re-applying the same mutation sets
the field to the same value. The store normalizes empty/whitespace-only
strings to `None` before persisting.

---

## Implementation Details

### Context assembly (`agent_serve.rs`)

When building the `AgentsMdContext` for a turn, the agent runtime:

1. Fetches the channel from the server → extracts `channel.instructions`
2. If the scope is a thread, fetches the thread list → finds the
   matching thread → extracts `thread.instructions`
3. Passes both into `AgentsMdContext` for rendering

Thread list fetch failures are logged at `debug` level and degrade
gracefully (thread instructions become `None`).

### AGENTS.md rendering (`agents_md.rs`)

The `loom_block()` function renders channel and thread instructions as
separate `## ` sections after the existing `## Stable agent instructions`
section. Instruction text passes through `sanitize_marker_text()` to
prevent injection of Loom's own `BEGIN loom` / `END loom` markers.

### Store layer (`store.rs`)

`set_channel_instructions()` and `set_thread_instructions()` both:
- Verify the entity exists (return `NotFound` if not)
- Normalize the input (trim + filter empty → `None`)
- Append a journal mutation
- Update the in-memory state
- Emit a `ChannelUpdated` / `ThreadUpdated` store event

---

## Design Rationale

### Why prompt-level instructions (not a new config entity)?

ARCH's design philosophy was **minimal invasion**: reuse the existing
`AGENTS.md` injection pipeline rather than creating a new context
assembly system. Channel/thread instructions are just two new optional
fields that flow through the same `loom_block()` renderer that already
handles agent instructions.

This avoids:
- New persistent entities or config files
- Changes to the provider prompt assembly
- Complex merge/conflict resolution logic

### Why additive layering (not override)?

Instructions are concatenated, not merged with conflict resolution.
This is intentional:
- Agent identity (layer 2) is always preserved
- Channel constraints (layer 3) add scope context without removing identity
- Thread guidance (layer 4) adds task-specific context

If an agent needs to "override" a channel instruction, it does so in
its own reasoning — the prompt presents all layers and the model
reconciles them.

### Relationship to STRAT's long-term AgentContext vision

STRAT's `05-workspace-attribute.md` proposes a future `AgentContext`
assembler that unifies all context sources (instructions, skills,
workspace files, AGENTS.md) into a single ordered composition. This
feature is the **Phase 0** step of that roadmap: it adds the
channel/thread instruction fields and injection, but does not yet
build the full assembler. The injection order implemented here
(agent → channel → thread) matches STRAT's recommended priority chain.

### No token budget limits

Per Founder directive, instruction text has no size limit. The
`Option<String>` field accepts arbitrary-length text. Users are
responsible for keeping instructions concise enough to avoid
excessive token consumption — the ARCH design doc recommends a
soft guideline of ~2KB per instruction layer.

---

## Files Changed (commit 627620c)

| File | Lines | Description |
|------|-------|-------------|
| `crates/proto/src/types.rs` | +10 | `Channel` + `Thread` structs gain `instructions` field |
| `crates/proto/src/methods.rs` | +80 | 6 JSON-RPC method constants + param/result types |
| `crates/agent-runtime/src/agents_md.rs` | +67 | `AgentsMdContext` +2 fields, render logic, 4 unit tests |
| `crates/cli/src/cmd/agent_serve.rs` | +42 | Context assembly fetches channel + thread instructions |
| `crates/cli/src/cmd/channel.rs` | +54 | `set/get/clear_instruction` CLI command functions |
| `crates/cli/src/cmd/thread.rs` | +54 | `set/get/clear_instruction` CLI command functions |
| `crates/cli/src/main.rs` | +81 | Subcommand registration, help text, payload reader |
| `crates/server/src/handlers/mod.rs` | +190 | 6 handlers with membership gating |
| `crates/server/src/store.rs` | +136 | `set_channel/thread_instructions` + journal replay arms |
| `crates/server/src/journal.rs` | +12 | 2 new `Mutation` variants |
| `crates/cli/src/cmd/chat/app.rs` | +5 | Context field initialization |
| `crates/cli/src/cmd/chat/sidebar.rs` | +2 | Context field initialization |
| `crates/cli/src/cmd/message.rs` | +1 | Context field initialization |
| **Total** | **+731** | **13 files changed** |

---

## Known Limitations

1. **No GUI management yet** — instructions can only be set via CLI.
   Future GUI support would add a settings panel for channel/thread
   instructions.

2. **No hot-reload** — instruction changes take effect on the next
   agent turn, not mid-turn. An agent already running with an old
   instruction set will continue using it until its next wake.

3. **No per-agent override** — channel instructions apply to ALL member
   agents equally. There is no mechanism for one agent to ignore or
   selectively apply channel instructions.

4. **Thread instructions require thread list fetch** — the context
   assembly fetches the full thread list to find the matching thread's
   instructions. In channels with many threads, this is O(n) per turn.
   Optimization (direct thread-by-id fetch) is deferred.

5. **No instruction versioning** — setting new instructions overwrites
   the previous value. The journal retains the mutation history, but
   there is no CLI command to view or revert to previous instruction
   versions.

---

## Change Log

| Version | Date | Change |
|---------|------|--------|
| Phase 1 | 2026-07-17 | Initial implementation: channel/thread `instructions` field + AGENTS.md injection + CLI + JSON-RPC + journal persistence. QA verified PASS on all 8 acceptance criteria. |
