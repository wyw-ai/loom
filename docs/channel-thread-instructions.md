# Channel/Thread Instructions & Skill Mounting

> This document covers two related features:
> - **Phase 1** — Channel/Thread Instructions (prompt-level constraints)
> - **Phase 2** — Channel/Thread Skill Mounting (hot-pluggable skill registries)

---

# Phase 1: Channel/Thread Instructions

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

## Phase 1 Known Limitations

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

## Phase 1 Change Log

| Version | Date | Change |
|---------|------|--------|
| Phase 1 | 2026-07-17 | Initial implementation: channel/thread `instructions` field + AGENTS.md injection + CLI + JSON-RPC + journal persistence. QA verified PASS on all 8 acceptance criteria. |

---

# Phase 2: Channel/Thread Skill Mounting

## Overview

Phase 2 adds **file-based skill registries** for channel and thread
scopes, enabling hot-pluggable skill mounting. Agents can now discover
and use skills registered at the channel or thread level, in addition
to the existing actor-bundle skills.

When an agent starts a turn, the skill resolution process reads the
channel and thread skill registry files, merges them with actor-bundle
skills by priority, and reconciles symlinks in the workspace's `skills/`
directory — all automatically, with no restart needed.

> **Design authority**: ARCH short-term plan Section 3 (`loom-channel-agent-skill-design/ARCH-短期落地方案-channel-agent与skill入口设计.md`)
> **Norms reference**: STRAT long-term architecture Section 2.3 (`loom-longterm-arch-design/05-workspace-attribute.md`)
> **Implementation**: commit `132c8de` on branch `feat/channel-thread-instructions`

---

## How It Works

### Skill priority chain

When multiple layers register a skill with the same id, the higher
priority layer overrides the lower one:

```
1. Thread skills              (highest — most specific scope)
2. Channel skills
3. Actor bundle skills        (per-agent skill bundles)
4. Default loom skill         (lowest — built-in fallback)
```

This is implemented via a `BTreeMap<String, PathBuf>` where later
inserts overwrite earlier ones. The resolution order in
`current_scope_skill_targets()` is:

1. Actor bundle skills are inserted first
2. Channel skills are inserted next (overriding same-id actor skills)
3. Thread skills are inserted last (overriding same-id channel skills)

#### Reserved skill ids

`loom` is a **reserved skill id**. The official Loom skill is always
backed by the embedded builtin snapshot materialized under
`<data_root>/builtin/skills/loom` and projected into every workspace
via `ensure_default_loom_skill()` + the final `workspace_skill_targets
.insert("loom", loom_skill)` step in `build_adapter_prompt()`.
Channel and thread skill registries MUST NOT override it: entries with
a reserved id are ignored at resolution time and a warning is logged.
This prevents a user-supplied (or malicious) registry from shadowing
the Loom operating-protocol skill that every agent relies on.

### Hot-pluggable design

Skill registry changes take effect on the **next agent turn** — no
restart needed. Here's why:

1. Each turn calls `build_adapter_prompt()` which calls
   `current_scope_skill_targets()`
2. That function reads the registry files fresh from disk
3. `ensure_scope_skill_targets()` reconciles symlinks in
   `{workspace}/skills/` to match the resolved targets
4. The agent discovers skills via the reconciled symlinks

This means you can `loom channel skill add` while agents are running,
and the next turn any agent in that channel picks up the new skill
automatically.

### Registry file locations

```
<data_root>/channels/<channel-id>/
  channel-skills.json          ← channel-level skill registry

<data_root>/channels/<channel-id>/threads/<thread-id>/
  thread-skills.json           ← thread-level skill registry
```

The `<data_root>` is the agent data root (typically `~/.agentx/`).
Registry files are read directly from the filesystem by the agent serve
process — no server RPC involved.

---

## CLI Usage

### Channel skills

```bash
# Add a skill to the channel registry
# (id is derived from the source directory name if --id is omitted)
loom channel skill add <channel_id> <source_path> [--id <skill_id>]

# Remove a skill from the channel registry
loom channel skill remove <channel_id> <skill_id>

# List skills registered for the channel
loom channel skill list <channel_id>
```

### Thread skills

```bash
# Add a skill to the thread registry
loom thread skill add <thread_id> <source_path> [--id <skill_id>]

# Remove a skill from the thread registry
loom thread skill remove <thread_id> <skill_id>

# List skills registered for the thread
loom thread skill list <thread_id>
```

> **Note**: Thread skill commands resolve the `channel_id` from the
> thread via a `thread.list` RPC call, then read/write the thread
> registry at the resolved path.

### JSON output

All commands support `--json`:

```bash
loom --json channel skill list <channel_id>
# → {"skills": [{"id": "obsidian", "source": "/path/to/skill", "added_at": "..."}]}
```

### Practical examples

```bash
# Mount an Obsidian skill at the channel level
loom channel skill add chan_spec01 /home/user/skills/obsidian
# → skill 'obsidian' added to channel chan_spec01

# Mount a PDF skill with a custom id
loom channel skill add chan_spec01 /home/user/skills/pdf-tools --id pdf
# → skill 'pdf' added to channel chan_spec01

# Override a channel skill at the thread level
loom thread skill add thread_bugfix42 /home/user/skills/obsidian-v2 --id obsidian
# → In thread_bugfix42, 'obsidian' now points to obsidian-v2 instead of the channel version

# List what's registered
loom channel skill list chan_spec01
# → obsidian             /home/user/skills/obsidian
#   pdf                  /home/user/skills/pdf-tools

# Remove a skill
loom channel skill remove chan_spec01 pdf
# → skill 'pdf' removed from channel chan_spec01
```

---

## Registry File Format

Both `channel-skills.json` and `thread-skills.json` use the same format:

```json
{
  "skills": [
    {
      "id": "obsidian",
      "source": "/home/user/skills/obsidian",
      "added_at": "1970-01-01T00:00:12345Z"
    },
    {
      "id": "pdf",
      "source": "/home/user/skills/pdf-tools",
      "added_at": "1970-01-01T00:00:12346Z"
    }
  ]
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Skill identifier. Must be unique within the registry. Used as the symlink name in `skills/`. |
| `source` | `string` | Absolute path to the skill directory. This is where the symlink points. |
| `added_at` | `string` | Timestamp when the skill was added. *(See Known Limitation #1 — format is currently incorrect.)* |

### Behavior

- **Missing registry file**: Treated as empty (no skills). No error.
  This ensures backward compatibility — channels/threads without
  registry files work exactly as before.
- **Upsert**: Adding a skill with an existing id replaces the previous
  entry (source and added_at are updated).
- **Remove absent skill**: Returns `removed: false`, no error. The
  registry file is not rewritten.
- **Parent directories**: Created automatically on first write
  (`fs::create_dir_all`).

---

## Implementation Details

### Module: `skill_registry.rs` (NEW)

A new CLI module (`crates/cli/src/cmd/skill_registry.rs`) provides the
core registry data structures and file I/O:

- **`SkillEntry`** — single skill record (id, source, added_at)
- **`SkillRegistry`** — collection of `SkillEntry` with `find()`,
  `upsert()`, `remove()` methods
- **Path helpers**: `channel_skill_registry_path()` and
  `thread_skill_registry_path()` compute the on-disk paths
- **Read/write**: `read_registry()` gracefully handles missing files
  (returns empty registry); `write_registry()` creates parent dirs

### Agent serve integration (`agent_serve.rs`)

`current_scope_skill_targets()` was extended with a `thread_id`
parameter. The resolution order:

1. **Actor bundle skills** (existing logic, unchanged)
2. **Channel skills** — reads `channel-skills.json` from data root,
   inserts each entry into the `BTreeMap` (overriding same-id actor
   skills)
3. **Thread skills** (only in thread scope) — reads
   `thread-skills.json`, inserts each entry (overriding same-id
   channel skills)

Registry read failures are logged at `debug` level and degrade
gracefully (that layer's skills are simply skipped).

### CLI commands (`channel.rs`, `thread.rs`, `main.rs`)

Channel skill commands (`skill_add`, `skill_remove`, `skill_list`)
operate directly on the filesystem via `skill_registry` module — no
server connection needed (the `client` parameter is not used for
channel skills).

Thread skill commands require a `client` connection to resolve the
`channel_id` from the `thread_id` via `thread.list` RPC, then operate
on the filesystem identically to channel skills.

The `--id` flag is optional. When omitted, the skill id is derived
from the `source` path's file name.

---

## Design Rationale

### Why file-based registries (not server RPC)?

ARCH's design chose CLI-side file I/O over server-side RPC for skill
registries because:

1. **Skill registries live in the agent data root** — the same
   filesystem the agent serve process already reads for workspace
   setup, scope paths, and symlinks
2. **No server state to persist** — skills are agent-side concerns,
   not collaboration state like messages or tasks
3. **Simpler implementation** — no new JSON-RPC methods, no journal
   mutations, no store changes
4. **Consistent with existing skill handling** — actor bundle skills
   are already resolved agent-side via filesystem paths

### Why BTreeMap for priority resolution?

A `BTreeMap<String, PathBuf>` naturally implements "last insert wins"
semantics. By inserting in priority order (actor → channel → thread),
higher-priority entries automatically overwrite lower-priority ones
for the same id. This is simpler and more efficient than explicit
conflict detection.

### Relationship to ARCH Phase 3 (.agent config file)

ARCH's design includes an optional Phase 3: a `.agent` JSON config
file that declaratively encapsulates instructions + skills + wake
policy for a channel. Phase 2's registry files are the imperative
(CLI-driven) equivalent of the skills portion. A future Phase 3 could
read `.agent` files and populate the same registries.

---

## Files Changed (commit 132c8de)

| File | Lines | Description |
|------|-------|-------------|
| `crates/cli/src/cmd/skill_registry.rs` | +288 | NEW: `SkillEntry`, `SkillRegistry`, path helpers, read/add/remove API, 6 unit tests |
| `crates/cli/src/cmd/agent_serve.rs` | +46 | Extend `current_scope_skill_targets()` with `thread_id` param + registry reads |
| `crates/cli/src/cmd/channel.rs` | +58 | `skill_add`, `skill_remove`, `skill_list` CLI functions |
| `crates/cli/src/cmd/thread.rs` | +88 | `skill_add`, `skill_remove`, `skill_list` + `resolve_channel_id` helper |
| `crates/cli/src/main.rs` | +72 | `ChannelSkillCmd` + `ThreadSkillCmd` enums, 6 dispatch arms |
| `crates/cli/src/cmd/mod.rs` | +1 | Module registration |
| **Total** | **+552** | **6 files changed** |

---

## Phase 2 Known Limitations

1. **`added_at` timestamp format is incorrect** — The `now_iso()`
   helper in `skill_registry.rs` formats the timestamp as
   `1970-01-01T00:00:{epoch_secs}Z` instead of a proper RFC3339
   UTC datetime. This is cosmetic only — the field is not used for
   any functional logic. A future fix should use `chrono` or the
   `time` crate for correct formatting.

2. **No GUI management yet** — Skill registries can only be managed
   via CLI. Future GUI support would add a skill management panel.

3. **Thread skill commands require server connection** — Unlike
   channel skill commands (which are pure filesystem I/O), thread
   skill commands need a server connection to resolve `channel_id`
   from `thread_id`. This is because the thread-to-channel mapping
   lives on the server.

4. **No skill validation** — Adding a skill does not verify that the
   `source` path exists or contains a valid skill. Invalid paths
   will result in broken symlinks at turn start. The agent will
   simply not discover the skill.

5. **No registry-level priority override** — Priority is fixed
   (thread > channel > actor). There is no mechanism to change the
   priority order or mark a channel skill as non-overridable by
   thread skills.

---

## Phase 2 Change Log

| Version | Date | Change |
|---------|------|--------|
| Phase 2 | 2026-07-17 | Skill mounting: file-based channel/thread skill registries + priority chain + hot-pluggable symlinks + CLI. QA verified PASS on all 8 acceptance criteria. |

---

# Part 3: GUI Integration

> **Commits**: `79b8bc6` (IPC layer), `93b7512` (Channel Configure panel), `710dc08` (Thread Configure panel)
> **Files changed**: 13 files, +766/-6 lines (7 modified, 3 new components, 3 new/modified IPC modules)

## Overview

The GUI integration brings Channel/Thread Instructions and Skills management
into the desktop application. Users can now configure scope-level
instructions and skills directly from the Loom GUI — no CLI required.

The GUI reuses the same backend as the CLI:
- **Instructions** → JSON-RPC server handlers (same as CLI)
- **Skills** → local file I/O on the same `channel-skills.json` /
  `thread-skills.json` registries (same as CLI)

Changes made via GUI are immediately visible to CLI and vice versa.

---

## GUI Entry Points

### Channel Configure Tab

The Channel Panel has four tabs. The **Configure** tab is the 4th tab:

```
┌──────────────────────────────────────────┐
│  Channel Panel                           │
├──────────────────────────────────────────┤
│  [Threads] [Members] [Tasks] [Configure] │
│                                   ^^^^^^ │
│                                   4th tab│
├──────────────────────────────────────────┤
│  ┌─ Instructions ──────────────────────┐ │
│  │  [textarea]                         │ │
│  │  [Save]  [Clear]                    │ │
│  └─────────────────────────────────────┘ │
│  ┌─ Skills ────────────────────────────┐ │
│  │  skill-id    source-path    [🗑]    │ │
│  │  ...                                │ │
│  │  [source path input]                │ │
│  │  [skill id (optional)] [Add]        │ │
│  └─────────────────────────────────────┘ │
└──────────────────────────────────────────┘
```

**Location**: Open any channel → click the **Configure** tab (gear icon,
4th position after Threads, Members, Tasks).

### Thread Configure Button

The Thread Panel has a **Settings gear icon** in the thread title bar
(next to the ScopeTokenSummary and Close button). Clicking it toggles an
inline collapsible configuration area:

```
┌──────────────────────────────────────────┐
│  Thread Title             [📊] [⚙️] [✕]  │
│                            token  config close│
├──────────────────────────────────────────┤
│  ⚙️ Configure (collapsible, max-h 40vh)  │
│  ┌─ Instructions ──────────────────────┐ │
│  │  [textarea]                         │ │
│  │  [Save]  [Clear]                    │ │
│  └─────────────────────────────────────┘ │
│  ┌─ Skills ────────────────────────────┐ │
│  │  ...                                │ │
│  └─────────────────────────────────────┘ │
├──────────────────────────────────────────┤
│  Thread conversation (messages)          │
└──────────────────────────────────────────┘
```

**Location**: Open any thread → click the **gear icon** (⚙️) in the
thread header bar. The config area appears between the header and the
message body, scrollable up to 40% of viewport height.

---

## Instructions Editing Flow

1. **Open Configure** — Channel: click Configure tab. Thread: click gear icon.
2. **Edit text** — A multi-line textarea (8 rows, monospace font) shows
   the current instructions. Edit freely.
3. **Save** — Click the **Save** button. The text is sent to the server
   via JSON-RPC (`channel_set_instruction` / `thread_set_instruction`).
   A green "Saved." confirmation flashes for 2 seconds.
4. **Clear** — Click the **Clear** button to remove all instructions.
   This calls `channel_clear_instruction` / `thread_clear_instruction`.
5. **Immediate effect** — Saved instructions are injected into every
   agent's AGENTS.md on their next turn. No restart needed.

**Empty state**: When no instructions are set, the textarea shows a
placeholder: *"Enter instructions for this channel/thread…"* and a hint:
*"No instructions set. Agents in this {scope} will use only their
agent-level instructions."*

---

## Skills Management Flow

1. **View list** — The Skills section shows all mounted skills as cards.
   Each card displays the skill **id** (bold) and **source path**
   (monospace, truncated).
2. **Add skill** — Enter the skill source path in the input field
   (e.g., `C:/path/to/my-skill`). Optionally enter a custom **skill id**
   in the second field. Click **Add**.
   - If no id is provided, it is auto-derived from the source path
     (directory name).
   - If a skill with the same id already exists, it is replaced (upsert).
3. **Remove skill** — Click the **trash can icon** (🗑) on any skill card
   to remove it.
4. **Immediate effect** — Skill changes write to the local JSON registry
   file. On the next agent turn, the skill resolution reconciles
   symlinks automatically. No restart needed.

**Empty state**: When no skills are mounted, a dashed-border box shows:
*"No skills mounted."*

---

## GUI ↔ CLI Equivalence Table

Every GUI operation has a direct CLI equivalent. Both operate on the
same backend state.

| GUI Action | Location | CLI Equivalent |
|------------|----------|----------------|
| View channel instructions | Channel Configure → Instructions textarea | `loom channel get-instruction <channel_id>` |
| Set channel instructions | Channel Configure → Instructions → Save | `loom channel set-instruction <channel_id> --text "..."` |
| Clear channel instructions | Channel Configure → Instructions → Clear | `loom channel clear-instruction <channel_id>` |
| View thread instructions | Thread gear → Instructions textarea | `loom thread get-instruction <thread_id>` |
| Set thread instructions | Thread gear → Instructions → Save | `loom thread set-instruction <thread_id> --text "..."` |
| Clear thread instructions | Thread gear → Instructions → Clear | `loom thread clear-instruction <thread_id>` |
| List channel skills | Channel Configure → Skills list | `loom channel skill list <channel_id>` |
| Add channel skill | Channel Configure → Skills → Add | `loom channel skill add <channel_id> <source> [--id <id>]` |
| Remove channel skill | Channel Configure → Skills → 🗑 | `loom channel skill remove <channel_id> <skill_id>` |
| List thread skills | Thread gear → Skills list | `loom thread skill list <thread_id>` |
| Add thread skill | Thread gear → Skills → Add | `loom thread skill add <thread_id> <source> [--id <id>]` |
| Remove thread skill | Thread gear → Skills → 🗑 | `loom thread skill remove <thread_id> <skill_id>` |

---

## Permission Model

### Instructions (server-enforced)

All instruction operations (get/set/clear) are routed through the Loom
server's JSON-RPC handlers. The server enforces **channel membership
gating**:

- `channel_set_instruction` — caller must be a member of the channel
- `channel_get_instruction` — caller must be a member of the channel
- `channel_clear_instruction` — caller must be a member of the channel
- `thread_set_instruction` — caller must be a member of the thread's
  parent channel
- `thread_get_instruction` — caller must be a member of the thread's
  parent channel
- `thread_clear_instruction` — caller must be a member of the thread's
  parent channel

Non-members receive an `APP_INVALID_STATE` error. **All channel members
can view and edit instructions** — there is no per-role restriction
beyond membership.

### Skills (local file I/O, no server gating)

Skill operations read/write the local JSON registry files directly
(`channel-skills.json` / `thread-skills.json` in the agent data root).
There is **no server-side membership check** for skill operations —
anyone with local filesystem access to the agent data root can modify
the registries. This matches the CLI behavior (skills are also pure
local file I/O in the CLI).

---

## Technical Architecture

### IPC Layer (commit 79b8bc6)

The Tauri IPC layer bridges the React frontend to the Rust backend:

```
React Component
    ↓ ipc.bridge TypeScript function
    ↓ Tauri invoke()
    ↓ Rust #[tauri::command] handler in crates/gui/src/ipc.rs
    ↓
    ├── Instructions → JSON-RPC call to Loom server (call_raw)
    │   e.g., method::CHANNEL_SET_INSTRUCTION
    │
    └── Skills → Direct local file I/O
        e.g., read_skill_registry() / write_skill_registry()
```

**Files**:
- `crates/gui/src/ipc.rs` (+283 lines) — 12 Tauri command handlers
  (6 instructions + 6 skills), `SkillEntryDto`, `SkillRegistryDto`,
  `read_skill_registry()`, `write_skill_registry()`,
  `derive_skill_id()`, `now_iso()` helpers
- `crates/gui/src/main.rs` (+12 lines) — Command registration in the
  Tauri builder
- `apps/gui-web/src/ipc/bridge.ts` (+117 lines) — TypeScript wrapper
  functions for all 12 IPC commands
- `apps/gui-web/src/ipc/types.ts` (+6 lines) — `SkillEntry` type
- `apps/gui-web/src/lib/types.ts` (+2 lines) — `ChannelPanelTab` type
  gains `"configure"` variant
- `apps/gui-web/src/lib/agent-utils.ts` (+2 lines) —
  `channelPanelDetail`/`channelPanelTitle` for configure tab

### Channel Configure Panel (commit 93b7512)

**New components**:
- `InstructionsSection.tsx` (+130 lines) — Reusable instructions
  editor (textarea + Save/Clear buttons). Accepts `scope` ("channel" |
  "thread") and `scopeId`. Used by both Channel and Thread panels.
- `SkillsSection.tsx` (+153 lines) — Reusable skills manager (list +
  add form + remove). Accepts `scope`, `channelId`, optional
  `threadId`. Used by both Channel and Thread panels.
- `ChannelConfigurePanel.tsx` (+16 lines) — Container that composes
  `InstructionsSection` + `SkillsSection` for channel scope.

**Modified files**:
- `ChannelPanels.tsx` (+10 lines) — Adds "configure" tab to the tab
  array, renders `ChannelConfigurePanel` when selected
- `ChatHeader.tsx` (+3 lines) — Passes configure tab support

### Thread Configure Panel (commit 710dc08)

**Modified file**:
- `ThreadPanel.tsx` (+35 lines) — Adds `showConfig` state, a gear-icon
  toggle button in the thread header, and a collapsible config area
  that renders `InstructionsSection` (scope="thread") +
  `SkillsSection` (scope="thread"). The config area is capped at
  `max-h-[40vh]` with overflow scroll.

### Component Reuse

`InstructionsSection` and `SkillsSection` are **shared components** —
the same React component renders in both the Channel Configure tab and
the Thread Configure collapsible. The `scope` prop ("channel" vs
"thread") determines which IPC functions are called:

```typescript
// InstructionsSection dispatches based on scope:
const fn = scope === "channel"
  ? () => ipc.channelGetInstruction(scopeId)
  : () => ipc.threadGetInstruction(scopeId);

// SkillsSection dispatches based on scope:
const res = scope === "channel"
  ? await ipc.channelSkillList(channelId)
  : await ipc.threadSkillList({ channelId, threadId: threadId! });
```

---

## GUI Integration Change Log

| Version | Date | Change |
|---------|------|--------|
| GUI Phase 1 | 2026-07-17 | IPC layer: 12 Tauri command handlers bridging React to Rust backend (instructions via JSON-RPC, skills via local file I/O) |
| GUI Phase 2 | 2026-07-17 | Channel Configure panel: 4th tab in Channel Panel with Instructions + Skills sections |
| GUI Phase 3 | 2026-07-17 | Thread Configure panel: inline collapsible area toggled by gear icon in thread header |
