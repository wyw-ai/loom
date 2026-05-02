## Why

Joi currently supports ACP agents and one-shot command transports, but some useful CLIs such as Claude and Copilot expose resumable interactive sessions that do not exit cleanly after each prompt. Supporting them requires an explicit Joi-managed session lifecycle, completion contract, output parser, and termination policy.

This change lets each agent keep one provider session per Joi scope (`thread:<id>` or `channel:<id>`), while preserving channel/thread isolation and allowing actor-specific Claude settings.

## What Changes

- Add an `interactive_command` agent transport for CLIs that accept prompt text and resume identifiers but may remain interactive after producing a reply.
- Define provider session mapping as one session per `(actor_id, scope.kind, scope.id, transport signature)`, so one agent has one session for a thread and one session for a channel common area.
- Add configurable session creation and resume argument templates for CLIs such as:
  - `claude "{prompt}" --session-id {session_id}` for first use
  - `claude "{prompt}" --resume {session_id}` for later use
  - `copilot "{prompt}" --resume {session_id}` for both first use and resume when supported
- Add prompt enhancement support so Joi can inject its envelope plus a completion contract telling the agent how to finish.
- Add completion detection for interactive output, with a sentinel-first default and timeout fallbacks.
- Add configurable termination policy so actors can declare how Joi should stop the CLI after completion, cancellation, or timeout.
- Add Claude settings policy for global settings, actor-profile settings, and custom settings paths.
- Keep existing `acp_stdio` and `command` transport behavior unchanged.

## Capabilities

### New Capabilities
- `interactive-agent-transport`: Defines interactive command transport behavior, provider session lifecycle, completion detection, kill policy, prompt/output enhancement, and Claude settings policy.

### Modified Capabilities
- None.

## Impact

- Affects agent spec schema in `crates/proto`.
- Adds runtime behavior in `crates/agent-runtime`.
- Wires the new transport in `crates/cli/src/cmd/agent_serve.rs`.
- Adds CLI/local session bookkeeping for interactive provider sessions under the existing agent data root.
- Updates documentation for agent configuration, Claude settings modes, and interactive transport examples.
- Requires tests for session mapping, signature invalidation, prompt contract injection, completion detection, kill policy, and transport selection.
