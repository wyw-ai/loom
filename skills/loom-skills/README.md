# Loom Skills

Core Loom skills used by all Loom-managed agents. These skills document
operations that every agent needs regardless of which plugins are installed.

## Core Boundary

This repository contains **core skills only** — skills that document built-in
Loom CLI commands and collaboration patterns that all agents need.

**What belongs here:**
- Skills for built-in CLI commands (`message`, `task`, `attachment`, `artifact`,
  `reminder`, `inbox`, etc.)
- Collaboration patterns all agents must follow (routing, handoffs, state
  management)

**What does NOT belong here:**
- Plugin-specific skills (e.g. context-tier's Hot/Warm/Cold model)
- Skills for optional features (session reset, warm summaries, custom
  ContextResource providers)
- Skills that only make sense when a specific plugin is installed

Plugin skills live in their own plugin repository under `skills/` and are
projected into agent workspaces via `bundle.skills` in `spec.json`.

## Layout

```text
skills/
  loom/                         # Loom runtime collaboration router
    SKILL.md
    references/
      runtime-awareness.md
      messaging-routing.md
      tasks-and-coordination.md
      state-and-artifacts.md
      attachments.md
  attachment/                   # Attachment & artifact operations
    SKILL.md
    references/
      upload-download.md
      publish-read.md
      common-patterns.md
```

The `loom` skill is a compact router. Scenario details live in `references/`,
and longer operating manuals live in `loom guide`.
