# Loom Documentation

This directory contains public design and implementation notes for Loom.
Product-specific workflows, private deployment notes, and customer or internal
integration runbooks are intentionally kept outside this tree.

## Architecture

- [current-app-implementation.md](./current-app-implementation.md) - current implementation overview.
- [architecture.md](./architecture.md) - process boundaries, ownership, protocol model, scheduling loop, cancellation, and code boundaries.
- [loom-refactor-plan.md](./loom-refactor-plan.md) - target architecture for the Loom refactor.
- [architecture-v1-agent-client.md](./architecture-v1-agent-client.md) - agent client split and adapter model.

## Protocol

- [protocol/open-multi-actor-collaboration-research.md](./protocol/open-multi-actor-collaboration-research.md) - research notes and model tradeoffs.
- [protocol/open-multi-actor-collaboration-protocol-v0.md](./protocol/open-multi-actor-collaboration-protocol-v0.md) - semantic protocol draft.
- [protocol/open-multi-actor-collaboration-schema-v0.md](./protocol/open-multi-actor-collaboration-schema-v0.md) - JSON-RPC schema draft.
- [protocol/agent-coordination-workflow.md](./protocol/agent-coordination-workflow.md) - claim-before-work and rebase-before-send workflow.
- [protocol/task-workflow.md](./protocol/task-workflow.md) - task and assignment workflow.
- [protocol/channel-workspace-model.md](./protocol/channel-workspace-model.md) - channel workspace and artifact sharing model.

## Runtime And Providers

- [command-transport-v0.md](./command-transport-v0.md) - command transport contract.
- [interactive-command-agent-transport-design.md](./interactive-command-agent-transport-design.md) - interactive command transport design.
- [protocol/provider-extension-design.md](./protocol/provider-extension-design.md) - provider manifest and runtime extension design.
- [service-plugin-system-design.md](./service-plugin-system-design.md) - service/plugin host design.
- [scheduler-plugin.md](./scheduler-plugin.md) - scheduler service plugin.

## GUI

- [gui-desktop-design.md](./gui-desktop-design.md) - Loom Desktop GUI design.
- [gui-actor-provider-management-design.md](./gui-actor-provider-management-design.md) - actor, provider, and host management design.

## Contracts And Runbooks

- [artifact-contracts.md](./artifact-contracts.md) - generic artifact contract conventions.
- [e2e-runbook-local.md](./e2e-runbook-local.md) - local end-to-end verification entry points.
