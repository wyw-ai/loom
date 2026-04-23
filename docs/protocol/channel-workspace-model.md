# Channel Workspace Model

## Background

Joi originally treated `channels/<channel-id>/workspace/` as the default live working directory for every agent in the channel. That kept permissions symmetric, but it also let one agent's scratch files affect another agent's reasoning. A partially written file could be mistaken for shared truth.

The fix is to keep a shared channel namespace while separating private scratch state from published shared state.

## Design Goals

- Keep agents equal in capability and access shape.
- Prevent one agent's in-progress workspace changes from polluting another agent's work.
- Preserve a channel-level filesystem root for shared assets and channel context.
- Keep migration simple for existing agents and specs.

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
      cache/
      logs/
  workspace/            # legacy compatibility directory, no longer the default live cwd
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

Relative workdir values are resolved under the agent's private workspace.

Supported templates:

- `{agent.workspace}`: the agent's private workspace
- `{agent.root}`: the agent's channel-scoped private root
- `{agent.profile}`: the agent's persistent state directory (Skills, MCP configs, long-lived agent memory, local model weights, etc.) — per-actor, shared across threads
- `{agent.logs}`: the agent's private log directory
- `{channel.root}`: the channel root
- `{channel.shared}`: the channel shared directory
- `{channel.sharedArtifacts}`: the shared artifact directory
- `{channel.workspace}`: compatibility alias to `{agent.workspace}`

`{channel.workspace}` stays accepted so existing specs continue to work, but its meaning changes to the agent-private workspace under the new model.

## Runtime Environment Variables

Joi injects the following environment variables for ACP runtimes:

- `AGENTX_CHANNEL_ROOT`
- `AGENTX_CHANNEL_SHARED`
- `AGENTX_CHANNEL_SHARED_ARTIFACTS`
- `AGENTX_CHANNEL_WORKSPACE`
- `AGENTX_AGENT_ROOT`
- `AGENTX_AGENT_WORKSPACE`
- `AGENTX_AGENT_CACHE`
- `AGENTX_AGENT_LOGS`

`AGENTX_CHANNEL_WORKSPACE` is a compatibility alias that resolves to the same path as `AGENTX_AGENT_WORKSPACE`.

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
- preserve compatibility for older `{channel.workspace}` specs

Phase 2 implemented:

- artifact publish/share MCP API
- artifact URI resolution in desktop messages

Future refinement:

- richer in-message artifact cards or previews
