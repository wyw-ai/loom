# Loom Documentation

This directory contains public design and implementation notes for Loom.
Product-specific workflows, private deployment notes, and customer or internal
integration runbooks are intentionally kept outside this tree.

Use this index as the entry point when changing Loom architecture, protocol,
runtime/provider behavior, GUI product flows, or local verification steps.

## Architecture

- [current-app-implementation.md](./current-app-implementation.md) - current app topology, process composition, data locations, agent lifecycle, runtime configuration, and validation focus.
- [architecture.md](./architecture.md) - process boundaries, data ownership, protocol model, scheduling loop, cancellation model, agent management commands, and code boundaries.
- [loom-refactor-plan.md](./loom-refactor-plan.md) - target breaking-change specification for Loom's actor/channel/thread/task/service model.
- [architecture-v1-agent-client.md](./architecture-v1-agent-client.md) - v1 agent client split, adapter model, registry ownership, wakeup flow, and deployment quick start.

## Protocol

- [protocol/open-multi-actor-collaboration-research.md](./protocol/open-multi-actor-collaboration-research.md) - research notes, tradeoffs, rejected approaches, terminology, and design decisions for open multi-actor collaboration.
- [protocol/open-multi-actor-collaboration-protocol-v0.md](./protocol/open-multi-actor-collaboration-protocol-v0.md) - semantic protocol draft covering domain objects, invariants, references, resources, events, relationships, operations, and interaction patterns.
- [protocol/open-multi-actor-collaboration-schema-v0.md](./protocol/open-multi-actor-collaboration-schema-v0.md) - JSON-RPC and object schema draft, including initialization, capability negotiation, methods, artifact ingress, and notifications.
- [protocol/agent-coordination-workflow.md](./protocol/agent-coordination-workflow.md) - claim-before-work, canonical thread, rebase-before-send, parallel contribution, review, timeout, and takeover workflow.
- [protocol/task-workflow.md](./protocol/task-workflow.md) - task data model, states, creation, claim, references, artifact links, facts/projections, assignments, preflight, workspace leases, and CLI binding.
- [protocol/channel-workspace-model.md](./protocol/channel-workspace-model.md) - channel workspace model, workdir semantics, runtime environment variables, sharing model, and rollout notes.

## Runtime And Providers

- [protocol/provider-extension-design.md](./protocol/provider-extension-design.md) - provider manifest design, Loom boundary decisions, provider modes, prompt parts, template variables, session strategy, output parsing, and runtime architecture.
- [command-transport-v0.md](./command-transport-v0.md) - command transport schema, session bookkeeping, first-run capture, resume arguments, output mapping, and worked examples.
- [interactive-command-agent-transport-design.md](./interactive-command-agent-transport-design.md) - interactive command transport architecture, schema, session lifecycle, prompt composition, completion detection, output pipeline, and rollout.
- [service-plugin-system-design.md](./service-plugin-system-design.md) - service/plugin host goals, topology, service-vs-agent boundary, ServiceSpec, runtime, state layout, protocol needs, and rollout.
- [scheduler-plugin.md](./scheduler-plugin.md) - scheduler ServiceSpec, config and job fields, trigger chain, state directory, protocol relationship, cron patterns, and debugging.

## GUI

- [gui-desktop-design.md](./gui-desktop-design.md) - Loom Desktop goals, technical stack, information architecture, design tokens, component slots, state model, Tauri bridge, UI update mapping, shortcuts, slash commands, and milestones.
- [gui-actor-provider-management-design.md](./gui-actor-provider-management-design.md) - Actors workspace product logic, host registration, provider availability, agent/service management, profile files, prompt studio, runtime configuration, backend changes, UI principles, rollout, and acceptance criteria.

## Contracts And Runbooks

- [artifact-contracts.md](./artifact-contracts.md) - artifact shape conventions, generic contract structures, and versioning rules.
- [e2e-runbook-local.md](./e2e-runbook-local.md) - local end-to-end prerequisites, build commands, core checks, harness entry points, troubleshooting, and release gate.
