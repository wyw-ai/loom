# Loom Documentation

This directory contains public design and implementation notes for Loom.
Product-specific workflows, private deployment notes, and customer or internal
integration runbooks are intentionally kept outside this tree.

Use this index as the entry point when changing Loom architecture, protocol,
runtime/provider behavior, GUI product flows, or local verification steps.

## Architecture

- [current-app-implementation.md](./current-app-implementation.md) - current app topology, process composition, data locations, agent lifecycle, runtime configuration, and validation focus.
- [architecture.md](./architecture.md) - process boundaries, data ownership, protocol model, scheduling loop, cancellation model, agent management commands, and code boundaries.
- [architecture-v1-agent-client.md](./architecture-v1-agent-client.md) - v1 agent client split, adapter model, registry ownership, wakeup flow, and deployment quick start.

## Protocol

- [protocol/open-multi-actor-collaboration-research.md](./protocol/open-multi-actor-collaboration-research.md) - research notes, tradeoffs, rejected approaches, terminology, and design decisions for open multi-actor collaboration.
- [protocol/open-multi-actor-collaboration-protocol-v0.md](./protocol/open-multi-actor-collaboration-protocol-v0.md) - semantic protocol draft covering domain objects, invariants, references, resources, events, relationships, operations, and interaction patterns.
- [protocol/open-multi-actor-collaboration-schema-v0.md](./protocol/open-multi-actor-collaboration-schema-v0.md) - JSON-RPC and object schema draft, including initialization, capability negotiation, methods, artifact ingress, and notifications.
- [protocol/agent-coordination-workflow.md](./protocol/agent-coordination-workflow.md) - claim-before-work, canonical thread, rebase-before-send, parallel contribution, review, timeout, and takeover workflow.
- [protocol/task-workflow.md](./protocol/task-workflow.md) - task data model, states, creation, claim, references, artifact links, facts/projections, assignments, preflight, workspace leases, and CLI binding.
- [protocol/channel-workspace-model.md](./protocol/channel-workspace-model.md) - channel workspace model, workdir semantics, runtime environment variables, sharing model, and rollout notes.

## Runtime And Providers

- [protocol/agent-runtime-awareness.md](./protocol/agent-runtime-awareness.md) - current protocol for Loom runtime awareness through workspace `AGENTS.md`, the default `loom` skill, `loom guide`, and prompt/system-prompt boundaries.
- [agent-turn-input-analysis.md](./agent-turn-input-analysis.md) - analysis of what agents actually receive per turn (GUI message → server delivery → worker queue → prompt), how bursts of messages are queued and replayed, design flaws measured against Loom's principles, and a turn-input contract proposal.
- [protocol/provider-extension-design.md](./protocol/provider-extension-design.md) - historical provider manifest design. Runtime-guidance and system-prompt boundaries in this document are superseded by `agent-runtime-awareness.md`.
- [command-transport-v0.md](./command-transport-v0.md) - command transport schema, session bookkeeping, first-run capture, resume arguments, output mapping, and worked examples.
- [interactive-command-agent-transport-design.md](./interactive-command-agent-transport-design.md) - interactive command transport architecture, schema, session lifecycle, prompt composition, completion detection, output pipeline, and rollout.
- [service-plugin-system-design.md](./service-plugin-system-design.md) - service/plugin host goals, topology, service-vs-agent boundary, ServiceSpec, runtime, state layout, protocol needs, and rollout.
- [scheduler-plugin.md](./scheduler-plugin.md) - scheduler ServiceSpec, config and job fields, trigger chain, state directory, protocol relationship, cron patterns, and debugging.

## GUI

- [gui-desktop-design.md](./gui-desktop-design.md) - Loom Desktop goals, technical stack, information architecture, design tokens, component slots, state model, Tauri bridge, UI update mapping, shortcuts, slash commands, and milestones.
- [gui-actor-provider-management-design.md](./gui-actor-provider-management-design.md) - Actors workspace product logic, host registration, provider availability, agent/service management, profile files, prompt studio, runtime configuration, backend changes, UI principles, rollout, and acceptance criteria.

## Context Layer

- [context-layer/README.md](./context-layer/README.md) - index for ContextLayer documentation (pluggable context-assembly system).
- [context-layer/getting-started.md](./context-layer/getting-started.md) - what ContextLayer is, when to enable it, minimal configuration, and scope inheritance.
- [context-layer/architecture.md](./context-layer/architecture.md) - core concepts (ContextResource trait, AssemblyContext, ResourceProvider, Registry), design constraints, token budget waterfall, and Hot/Warm/Cold layering.
- [context-layer/configuration.md](./context-layer/configuration.md) - complete agentcontext.json schema, per-scheme configuration, merge semantics, and template variables.
- [context-layer/api-reference.md](./context-layer/api-reference.md) - trait method signatures, struct fields, built-in provider APIs, and custom provider example.
- [context-layer/source-navigation.md](./context-layer/source-navigation.md) - key source file paths and line numbers across the codebase.
- [context-layer/message-list-warm-cold.md](./context-layer/message-list-warm-cold.md) - Hot/Warm/Cold context layering, session reset triggers, Warm summary generation and persistence.
- [context-layer/skill-mounting-guide.md](./context-layer/skill-mounting-guide.md) - Skill installation projection, progressive disclosure, MCP mapping.
- [context-layer/data-source-best-practices.md](./context-layer/data-source-best-practices.md) - per-data-source configuration examples, priority allocation, security constraints.
- [context-layer/custom-provider-guide.md](./context-layer/custom-provider-guide.md) - ContextResource/ResourceProvider implementation, inventory self-registration, security constraints.
- [context-layer/memory-plugin-guide.md](./context-layer/memory-plugin-guide.md) - memory plugin architecture (iter2 pluginization), disable/override/customize states, behavior semantics, migration, and troubleshooting.
- [context-layer/third-party-resource-guide.md](./context-layer/third-party-resource-guide.md) - privilege-free third-party resource contract: inventory registration, embedded config, ambient data channels, and the EchoInputResource worked example.

## Contracts And Runbooks

- [artifact-contracts.md](./artifact-contracts.md) - artifact shape conventions, generic contract structures, and versioning rules.
- [release-preview-workflow.md](./release-preview-workflow.md) - versioned `preview-X.Y.Z` release candidates, automatic 0.x minor rollover, recovery, and release-bot setup.
- [e2e-runbook-local.md](./e2e-runbook-local.md) - local end-to-end prerequisites, build commands, core checks, harness entry points, troubleshooting, and release gate.
