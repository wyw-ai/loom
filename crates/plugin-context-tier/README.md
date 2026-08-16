# loom-plugin-context-tier

Loom Hot/Warm/Cold three-tier context lifecycle management plugin.

## Overview

This plugin provides operational guidance and Rust ContextResources for
Loom's temperature-based context management model. It is an independent,
pluggable package that self-registers via `inventory::submit!` — Loom
discovers it at runtime without knowing this crate's concrete types.

Separate from [`loom-skills`](https://github.com/wyw-ai/skills)
(collaboration operations) and
[`actor-circuit`](https://github.com/wyw-ai/actor-circuit)
(multi-actor vocabulary).

## Rust Crate

This package is a Rust crate that self-registers two ContextResources
via `inventory::submit!`:

| Resource | Scheme | Priority | Description |
| --- | --- | --- | --- |
| WarmSummaryContextResource | `warm-summary` | 7 | Reads persisted Warm summary files |
| MessageListProvider | `message-list` | 10 | Wraps delivery cursor context |

Loom discovers these at runtime via `agent_runtime::discover_plugins()`
(`inventory::iter::<ContextResourcePlugin>`). The `agent-runtime` crate
uses `extern crate loom_plugin_context_tier;` to force-link this crate,
ensuring inventory registrations are not stripped by the linker.

**Zero coupling**: Loom interacts with these resources only through
`dyn ContextResource` (defined in `context-layer-core`). Adding a new
plugin requires zero changes to Loom source code.

## Three-Tier Model

| Tier | Lifecycle | Storage | Description |
| --- | --- | --- | --- |
| **Hot** | Single turn | Prompt envelope | Current turn's delivery context, inbox, trigger message |
| **Warm** | Cross-turn | `{profile_dir}/summaries/{scope_id}.md` | Compressed summary generated on session reset |
| **Cold** | Permanent | Loom server database | Full message history, retrieved via CLI |

## Package Structure

```
loom-plugin-context-tier/
├── src/
│   ├── lib.rs                              ← inventory::submit! self-registration
│   ├── warm_summary.rs                     ← WarmSummaryContextResource
│   └── message_list.rs                     ← MessageListProvider
├── skills/
│   └── context-tier/
│       ├── SKILL.md                        ← Operations guide
│       └── references/
│           ├── hot-layer.md                ← Hot layer: current turn context
│           ├── warm-layer.md               ← Warm layer: session reset lifecycle
│           └── cold-layer.md               ← Cold layer: CLI historical retrieval
├── context-resources/
│   └── default-agentcontext.json           ← Temperature model config template
├── Cargo.toml                              ← Crate manifest (depends on context-layer-core)
├── plugin.json                             ← Plugin manifest (id, layer, context_resources)
└── README.md
```

## Installation

### Option A: Skill installation only (provider-native)

Add `context-tier` to the agent's `bundle.skills` in `spec.json`:

```json
{
  "bundle": {
    "skills": [
      { "id": "context-tier", "source": "{agent.bundle}/skills/context-tier" }
    ]
  }
}
```

This projects the skill to the workspace for provider-native reading. The
temperature model's ContextResource capabilities (warm-summary, message-list)
are already included in `default_agent_context_spec()` and work without this
skill package.

### Option B: Full configuration (skill + context resources)

1. Install the skill (Option A above).
2. Reference `context-resources/default-agentcontext.json` in the agent's
   `agentcontext.json` to explicitly configure the temperature model
   resources.

## Configuration Template

The `context-resources/default-agentcontext.json` template includes:

| Priority | Resource | Tier | Description |
| --- | --- | --- | --- |
| 5 | `memory` | — | Agent memory |
| 7 | `warm-summary` | Warm | Session reset summary injection |
| 10 | `message-list` | Hot | Current turn delivery context |
| 20 | `file` | — | Workspace file resources |

> **Note**: Cold layer needs no ContextResource — it is accessed through CLI
> commands (`message read`, `message search`).

## Design Decisions

| Decision | Choice | Rationale |
| --- | --- | --- |
| Skill package carries Rust code | Yes | Rust crate with inventory self-registration for compile-time plugin discovery |
| Three tiers independently mountable | No (atomic) | Hot + Warm declared together; Cold is CLI-only |
| SkillContextResource dependency | No | Temperature model core capabilities are independent ContextResources |
| Cold layer needs ContextResource | No | CLI + server-side; SKILL.md documentation covers it |
| Registration mechanism | inventory::submit! | Compile-time self-registration; Loom discovers via inventory::iter without knowing concrete types |

## Compatibility

- **Loom D2+**: The default resource chain (`default_agent_context_spec()`)
  already includes `warm-summary` (priority=7) and `message-list`
  (priority=10). This plugin provides the operational documentation and
  the Rust crate implementations.
- **Loom D1**: D1 fallback was removed in commit `3b391ba`. Warm summary and
  message-list now work exclusively through the ContextResource chain.

## Related Packages

| Package | Scope |
| --- | --- |
| [loom-skills](https://github.com/wyw-ai/skills) | Loom collaboration operations (messaging, tasks, state, attachments) |
| [actor-circuit](https://github.com/wyw-ai/actor-circuit) | Multi-actor vocabulary (L0/L1/L2 gate model) |
| **loom-plugin-context-tier** (this package) | Temperature model operations (Hot/Warm/Cold) |

## License

Same as the Loom project.
