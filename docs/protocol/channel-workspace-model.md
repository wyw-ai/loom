# Channel Workspace Model

## Background

Joi originally treated `channels/<channel-id>/workspace/` as the default live working directory for every agent in the channel. That kept permissions symmetric, but it also let one agent's scratch files affect another agent's reasoning. A partially written file could be mistaken for shared truth.

The fix is to keep a shared channel namespace while separating private scratch state from published shared state.

## Design Goals

- Keep agents equal in capability and access shape.
- Prevent one agent's in-progress workspace changes from polluting another agent's work.
- Preserve a channel-level filesystem root for shared assets and channel context.
- Keep workspace ownership obvious from the path.

## Core Model

The channel directory is a shared namespace, not a single live workspace.

- `channel root`: shared container for channel-scoped state
- `agent workspace`: private default working directory for one agent inside the channel
- `shared`: channel-scoped published files and artifacts
- `attachments` / `threads/<thread-id>/context`: existing published context managed by Joi

Directory layout:

```text
~/.agentx/channels/<channel-id>/
  attachments/
  threads/
  shared/
    artifacts/
  agents/
    <agent-id>/
      workspace/
      logs/
  runtime/
  logs/
  cache/
  mcp/
```

## Workdir Semantics

Default agent `cwd` is now:

```text
~/.agentx/channels/<channel-id>/agents/<agent-id>/workspace
```

The cwd is not configurable in `AgentTransport`; Joi computes it from the
current channel and actor for every dispatch.

Supported templates:

- `{agent.workspace}`: the agent's private workspace
- `{agent.root}`: the agent's channel-scoped private root
- `{agent.profile}`: the agent's persistent state directory (Skills, MCP configs, long-lived agent memory, local model weights, etc.) — per-actor, shared across threads
- `{agent.logs}`: the agent's private log directory
- `{channel.root}`: the channel root
- `{channel.shared}`: the channel shared directory
- `{channel.sharedArtifacts}`: the shared artifact directory

## Runtime Environment Variables

Command transport subprocesses receive the following channel-scoped environment
variables. ACP transports receive the channel workspace through
`session/new.cwd`; the long-lived ACP process itself only gets actor-level
environment variables.

- `AGENTX_CHANNEL_ROOT`
- `AGENTX_CHANNEL_SHARED`
- `AGENTX_CHANNEL_SHARED_ARTIFACTS`
- `AGENTX_AGENT_ROOT`
- `AGENTX_AGENT_WORKSPACE`
- `AGENTX_AGENT_LOGS`

## Sharing Model

Private workspaces are for drafts. Shared state should be explicit.

Near-term shared channels already exist through:

- channel messages
- message attachments
- thread context files

Implemented in the current codebase:

- `artifact_publish` copies selected agent outputs into `shared/artifacts/`
- published artifacts get stable `artifact://<channel-id>/<artifact-id>` references
- handoffs are stored separately in `agent_handoffs` and can be created or inspected through both MCP and local CLI entry points
- `artifact_list`, `artifact_get`, and `artifact_read` let agents consume shared artifacts without depending on native filesystem tools
- a local artifact CLI provides the same publish/list/get/read flow for runtimes where ACP-side MCP injection is unavailable or unreliable
- a local handoff CLI provides the same collaboration fallback for runtimes where handoff MCP tools are unavailable
- the desktop UI resolves `artifact://...` links in agent markdown replies and opens the published file or directory

## Non-Goals

- Hard security isolation against a malicious local user or a malicious agent runtime
- Preventing an agent from intentionally walking outside its workspace when the runtime has unrestricted local filesystem access

This model solves accidental cross-agent interference first. Stronger isolation would require sandboxing or a mediated filesystem tool surface.

## Rollout

Phase 1 implemented in code:

- create per-agent private workspace directories under each channel
- resolve default workdir to the private workspace
- keep channel-level shared directories

Phase 2 implemented:

- artifact publish/share MCP API
- artifact URI resolution in desktop messages

Future refinement:

- richer in-message artifact cards or previews
