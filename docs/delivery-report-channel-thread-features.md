# Delivery Report: Channel/Thread Instructions & Skill Mounting

> **Date**: 2026-07-17
> **Branch**: `feat/channel-thread-instructions` (4 commits, LOCAL REPO ONLY)
> **Status**: Phase 1 + Phase 2 delivered, QA verified PASS
> **Technical doc**: [`docs/channel-thread-instructions.md`](./channel-thread-instructions.md) (720 lines)

---

## 1. Delivery Summary

| Phase | Feature | One-line description |
|-------|---------|---------------------|
| Phase 1 | Channel/Thread Instructions | Channel and thread owners can set prompt-level constraints that are injected into every member agent's AGENTS.md at turn start |
| Phase 2 | Channel/Thread Skill Mounting | Channel and thread owners can register skills via file-based registries; agents auto-discover them with priority-based override, no restart needed |

**Problem solved**: Before this delivery, Loom agents received identical system prompts regardless of which channel or thread they were working in. There was no mechanism to tell agents "this channel maintains the Obsidian vault" or "this thread is a P0 hotfix." Now, scope-specific context and skills can be declared at the channel and thread level.

---

## 2. Implementation Inventory

### Commit history (4 commits, all local)

| Commit | Date | Phase | Description |
|--------|------|-------|-------------|
| `627620c` | 2026-07-17 02:15 | P1 — code | Channel/thread instructions field + AGENTS.md injection + CLI + JSON-RPC + journal |
| `d5e1a27` | 2026-07-17 02:19 | P1 — docs | Feature documentation (392 lines) |
| `132c8de` | 2026-07-17 03:00 | P2 — code | Channel/thread skill mounting: file-based registries + priority chain + CLI |
| `000e07a` | 2026-07-17 03:02 | P2 — docs | Phase 2 documentation supplement (+331 lines, total 720) |

### Phase 1 files changed (commit 627620c — 13 files, +731 lines)

| File | Lines | What changed |
|------|-------|-------------|
| `proto/src/types.rs` | +10 | `Channel` + `Thread` structs gain `instructions: Option<String>` field |
| `proto/src/methods.rs` | +80 | 6 JSON-RPC method constants + param/result types |
| `agent-runtime/src/agents_md.rs` | +67 | AGENTS.md context +2 fields, render logic, 4 unit tests |
| `cli/src/cmd/agent_serve.rs` | +42 | Context assembly fetches channel + thread instructions |
| `cli/src/cmd/channel.rs` | +54 | `set/get/clear-instruction` CLI functions |
| `cli/src/cmd/thread.rs` | +54 | `set/get/clear-instruction` CLI functions |
| `cli/src/main.rs` | +81 | Subcommand registration, help text, payload reader |
| `server/src/handlers/mod.rs` | +190 | 6 handlers with membership gating |
| `server/src/store.rs` | +136 | `set_channel/thread_instructions` + journal replay arms |
| `server/src/journal.rs` | +12 | 2 new `Mutation` variants |
| `cli/src/cmd/chat/app.rs` | +5 | Context field initialization |
| `cli/src/cmd/chat/sidebar.rs` | +2 | Context field initialization |
| `cli/src/cmd/message.rs` | +1 | Context field initialization |

### Phase 2 files changed (commit 132c8de — 6 files, +552 lines)

| File | Lines | What changed |
|------|-------|-------------|
| `cli/src/cmd/skill_registry.rs` | +288 | **NEW**: `SkillEntry`, `SkillRegistry`, path helpers, read/add/remove API, 6 unit tests |
| `cli/src/cmd/agent_serve.rs` | +46 | `current_scope_skill_targets()` extended with `thread_id` + registry reads |
| `cli/src/cmd/channel.rs` | +58 | `skill_add`, `skill_remove`, `skill_list` CLI functions |
| `cli/src/cmd/thread.rs` | +88 | `skill_add`, `skill_remove`, `skill_list` + `resolve_channel_id` helper |
| `cli/src/main.rs` | +72 | `ChannelSkillCmd` + `ThreadSkillCmd` enums, 6 dispatch arms |
| `cli/src/cmd/mod.rs` | +1 | Module registration |

### Documentation files

| File | Commit | Lines | Content |
|------|--------|-------|---------|
| `docs/channel-thread-instructions.md` | d5e1a27 + 000e07a | 720 | Complete technical documentation covering both phases |

**Total delivery**: 19 files changed, +1,283 insertions across 4 commits.

---

## 3. Feature Description (User Perspective)

### Phase 1: Channel/Thread Instructions

Channel and thread owners can now set **prompt-level instructions** that every agent in that scope receives. Think of it as a "sticky note" attached to the channel or thread that all agents read at the start of every turn.

**What you can do:**

```bash
# Tell all agents in a channel what this channel is about
loom channel set-instruction chan_spec01 --text "This channel maintains the Obsidian vault at C:/vault. Use Obsidian-flavored Markdown."

# Give a specific thread extra context
loom thread set-instruction thread_bugfix42 --text "P0 hotfix: production crash. Direct fixes only."

# Check what's set
loom channel get-instruction chan_spec01

# Remove when done
loom thread clear-instruction thread_bugfix42
```

**How agents see it**: The instructions appear as `## Channel instructions` and `## Thread instructions` sections inside the agent's AGENTS.md bootstrap block, layered on top of the agent's own identity instructions.

### Phase 2: Channel/Thread Skill Mounting

Channel and thread owners can now **register skills** that agents in that scope automatically discover and use. Skills are hot-pluggable — adding or removing a skill takes effect on the next agent turn, with no restart needed.

**What you can do:**

```bash
# Mount a skill at the channel level (all agents in this channel get it)
loom channel skill add chan_spec01 /path/to/obsidian-skill

# Mount a skill at the thread level (overrides channel skill with same id)
loom thread skill add thread_bugfix42 /path/to/specialized-skill --id obsidian

# List registered skills
loom channel skill list chan_spec01

# Remove a skill
loom channel skill remove chan_spec01 obsidian
```

**Priority chain** (highest to lowest): Thread skills > Channel skills > Actor bundle skills > Default loom skill. When the same skill id exists at multiple levels, the higher-priority level wins.

---

## 4. Verification Results

### QA acceptance

| Criterion | Phase 1 | Phase 2 |
|-----------|---------|---------|
| Acceptance criteria | 8/8 PASS | 8/8 PASS |

### Build & test

| Check | Result |
|-------|--------|
| `cargo build` | PASS |
| `cargo test` | 609 passed, 1 pre-existing failure (unrelated to this feature) |

### Phase 1 unit tests (4 new)

- Channel instructions section rendered when present
- Thread instructions section rendered when present
- Empty instructions sections are omitted
- Instruction marker text is sanitized (no BEGIN/END marker injection)

### Phase 2 unit tests (6 new)

- Missing registry returns empty (backward compatibility)
- Channel skill round-trip (add → read)
- Upsert replaces same id
- Remove returns false when absent
- Thread skill round-trip (add → read)
- Thread and channel registries are independent

---

## 5. Known Limitations

1. **`now_iso()` timestamp format** (cosmetic) — The `added_at` field in skill registry entries uses an incorrect timestamp format (`1970-01-01T00:00:{epoch_secs}Z` instead of proper RFC3339). This is metadata-only and does not affect skill resolution or functionality. Fix requires adding `chrono` or `time` crate dependency.

2. **Phase 3 (.agent config file) deferred** — ARCH's design includes an optional Phase 3: a declarative `.agent` JSON config file that encapsulates instructions + skills + wake policy for a channel. This was not implemented in this delivery. The current CLI-based approach (Phase 1 + Phase 2) covers the same functionality imperatively.

3. **No GUI management** — Both instructions and skills can only be managed via CLI. Future GUI support would add settings panels.

4. **No hot-reload mid-turn** — Instruction and skill changes take effect on the next agent turn, not during an in-progress turn.

5. **Thread skill commands require server connection** — Unlike channel skill commands (pure filesystem I/O), thread skill commands need a server connection to resolve `channel_id` from `thread_id`.

---

## 6. Technical Documentation Reference

The complete technical documentation is at:

**`docs/channel-thread-instructions.md`** (720 lines)

It covers:
- Phase 1: Data model, AGENTS.md injection order, CLI usage, JSON-RPC protocol, journal persistence, backward compatibility, implementation details, design rationale
- Phase 2: Skill priority chain, registry file format, hot-pluggable design, CLI usage, implementation details, design rationale

### Design source materials

| Document | Author | Location |
|----------|--------|----------|
| ARCH short-term plan (Section 2 + 3) | ARCH | `loom-channel-agent-skill-design/ARCH-短期落地方案-channel-agent与skill入口设计.md` |
| STRAT workspace attribute norms | STRAT | `loom-longterm-arch-design/05-workspace-attribute.md` |

---

## 7. Next Steps

This delivery is complete and awaiting Founder direction on:

- **Merge**: Whether to merge `feat/channel-thread-instructions` into `preview`/`main`
- **Push**: Whether to push to remote (currently LOCAL REPO ONLY)
- **Phase 3**: Whether to proceed with the optional `.agent` config file design
- **Bug fix**: Whether to fix the `now_iso()` timestamp issue in this branch or defer
