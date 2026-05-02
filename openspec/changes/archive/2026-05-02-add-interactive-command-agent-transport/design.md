## Context

Joi currently has two agent execution models:

- `acp_stdio`: long-lived ACP subprocesses with protocol-level sessions, prompt completion, cancellation, tool updates, and permission requests.
- `command`: one subprocess per prompt where process exit is the completion boundary; Joi can capture and resume a provider session id when the CLI exposes one.

Interactive CLIs such as Claude and Copilot can accept prompt text plus a session identifier, but may remain in an interactive process after producing a reply. In that mode there is no reliable process-exit completion marker. Joi needs a separate transport model that can drive one prompt, detect the final answer, persist the provider session id, and terminate or cancel the process according to actor configuration.

The design must align with Joi's collaboration model: a `ScopeRef` is either `thread:<id>` or `channel:<id>`. A thread represents a task branch, and a channel scope represents the channel common area. The provider session lifecycle should therefore follow `ScopeRef`, not a global actor session.

## Goals / Non-Goals

**Goals:**

- Introduce `interactive_command` as a distinct transport for CLIs that require Joi-managed completion and termination.
- Persist one provider session per `(actor_id, scope.kind, scope.id, session signature)`.
- Treat `thread:<id>` and `channel:<id>` uniformly: each gets its own provider session per actor.
- Support first-run and resume argument templates for Claude and Copilot style CLIs.
- Add an explicit prompt completion contract, with sentinel-based completion as the default.
- Add output parsing that can stream text, strip completion sentinels, and emit `AdapterEvent::Finished`.
- Add kill policy configuration for completion, cancellation, and timeout.
- Add Claude settings policy with global, actor-profile, and custom modes.
- Preserve current `acp_stdio` and `command` behavior.

**Non-Goals:**

- Do not make `joi-server` spawn or supervise local agent subprocesses.
- Do not replace ACP; ACP remains the preferred transport when available.
- Do not infer completion from arbitrary model text without a configured contract.
- Do not share a provider session across different threads or between a channel common area and a thread.
- Do not automatically retry a failed resume by default, because that can duplicate model/tool work.
- Do not implement full terminal UI automation beyond what is required to drive one prompt and collect output.

## Decisions

### Add `interactive_command` rather than overloading `command`

`command` assumes subprocess exit means the turn is complete. Interactive CLIs violate that assumption, so adding completion detection and post-completion kill behavior to `command` would make existing behavior harder to reason about.

`interactive_command` gets its own configuration and adapter. It can reuse utility code for template expansion, session records, output translation, and signal handling where appropriate, but the lifecycle is separate:

1. Resolve or create provider session id for the actor and Joi scope.
2. Build argv from `newArgs` or `resumeArgs`.
3. Append `--model=<model>` when an active model is configured.
4. Spawn the CLI with the resolved cwd/env/settings.
5. Read stdout/stderr or PTY output.
6. Detect configured completion.
7. Emit final text and `Finished`.
8. Apply kill policy to stop the child if it remains alive.

Alternative considered: extend `command` with `completion` and `kill` fields. Rejected because it blurs process-exit and Joi-detected completion semantics.

### Provider session lifecycle follows Joi `ScopeRef`

Session records are keyed by actor plus scope:

```text
actor_id + scope.kind + scope.id + session_signature
```

This means:

- One agent has exactly one provider session for a given thread, unless reset or invalidated.
- One agent has exactly one provider session for a channel common area.
- Different agents in the same thread do not share sessions.
- A channel common area and its threads do not share sessions.

The persisted record stores the provider session id, scope, timestamps, and signature data. The signature includes transport kind, command/args templates, model, Claude settings path/signature, and prompt contract version. If the signature changes, Joi creates a new provider session by default.

Alternative considered: one provider session per actor. Rejected because it leaks context across private channels and unrelated threads.

Alternative considered: one provider session per channel including all threads. Rejected because threads are independent task branches and should not cross-contaminate execution memory.

### Joi owns generated session ids by default

For `interactive_command`, Joi can generate a UUID before the first invocation and pass it to the provider CLI. This supports CLIs whose first-run command accepts a caller-provided session id:

```text
claude "{prompt}" --session-id {session_id}
copilot "{prompt}" --resume {session_id}
```

The session policy includes:

- `idStrategy = "joi_uuid_per_scope"` as the default.
- `newArgs` for first use.
- `resumeArgs` for subsequent use.
- active model support: when a model is configured, append `--model=<model>` to the resolved argv; when no model is configured, append no model argument.
- `onMissing = "create"`.
- `onSignatureChanged = "create"`.
- `onResumeFailed = "fail"` by default.

The default resume failure behavior is fail-loudly rather than retry. A retry can double-run a prompt after partial external execution. Implementations may later add an opt-in `create_and_retry_once`, but it should not be the default.

The model argument is intentionally appended as a single argv token (`--model=xxx`) after `newArgs` or `resumeArgs` are expanded. This keeps the common case simple and avoids requiring every actor spec to duplicate model wiring. If no model is selected or configured, the adapter must not append a model argument.

### Completion is contract-driven

Interactive output needs a reliable completion boundary. The default mechanism is a Joi-injected response contract:

```text
When your final user-visible answer is complete, output this exact marker on a line by itself:
__JOI_DONE__
Do not output anything after the marker.
```

The adapter treats the sentinel as the normal completion signal, strips it from user-visible output, flushes text, updates the provider session record, applies the completion kill policy, and emits `Finished`.

Done is defined in layers:

- Agent reply done: the provider output contains the configured sentinel.
- Adapter turn done: final text has been flushed, the session record is saved or updated, the completion kill policy has been applied, and `AdapterEvent::Finished` has been emitted.
- Joi turn done: `joi agent serve` has translated the final adapter events into server-visible content and turn closure.

For `interactive_command`, process exit alone is not successful Done. A normal successful turn requires sentinel detection. If the process exits before the sentinel, times out, fails to spawn, fails to resume, or cannot be terminated according to policy, the adapter must finish with `success: false` and a summary that explains the failure.

Completion configuration supports:

- `sentinel`: exact marker text, default `__JOI_DONE__`.
- `stripSentinel`: default true.
- `idleTimeoutMs`: optional fallback to detect stalled output.
- `maxTurnMs`: hard deadline for a turn.
- Future extension point for regex completion.

Alternative considered: idle timeout only. Rejected because slow model/tool output and waiting-for-input states are indistinguishable.

### Prompt enhancement is explicit

Joi already composes an envelope with identity, soul, memory, scope bootstrap, and user message. `interactive_command` adds a prompt contract section, but keeps the user's message last or clearly delimited.

The transport can configure a template with placeholders:

- `{joi_envelope}` for the existing composed prompt.
- `{user_message}` for the raw rendered trigger message.
- `{session_id}` for the resolved provider session id.
- Existing agent path variables such as `{agent.profile}`, `{agent.workspace}`, `{scope.id}`, and `{actor.id}`.

The default template appends the completion contract to the Joi envelope before the user message delimiter. Actor-specific prompt enhancement is declarative in the spec rather than hard-coded for one provider.

### Output enhancement is conservative

The initial output pipeline should:

- Read stdout/stderr or PTY output without shell interpretation.
- Strip ANSI sequences when configured.
- Accumulate text until sentinel completion.
- Stream partial text to Joi when safe.
- Remove the sentinel and prompt echoes from the final answer when configured.
- Emit stderr as trace/error information rather than mixing it into final user text by default.

Provider-specific output parsers can be added later, but the base version should work for plain text interactive CLIs.

### Kill policy is actor-configurable

After detecting completion, the child process may still be alive. The actor spec declares how to end it:

- `stdin_eof`
- `ctrl_d`
- `ctrl_c`
- `sigterm`
- `sigkill`
- `none`

The default for completion is `sigterm` with a short grace period and `sigkill` fallback on Unix. Cancellation should prefer a stronger or provider-appropriate policy. Timeout should mark the turn failed and then apply the configured timeout kill policy.

This keeps "how to kill me" close to the actor definition, where provider-specific behavior belongs.

### Claude settings policy is actor-level

Claude settings affect an actor's behavior, permissions, and MCP/tool configuration. They should be stable across threads by default, so the actor profile is a better default location than a channel workspace.

The policy supports:

- `global`: do not pass `--settings`.
- `actor_profile`: pass `--settings {agent.profile}/claude/settings.json`.
- `custom`: pass `--settings <expanded path>`.

`actor_profile` should ensure the parent directory exists, but should not silently invent a settings file with surprising defaults unless explicitly requested by a future scaffold task.

The settings path/signature participates in provider session signature calculation. Changing settings creates a new provider session by default.

## Risks / Trade-offs

- **Sentinel may appear in normal output** → Use a distinctive default marker and allow actor-specific sentinel customization.
- **Agent ignores completion contract** → Use `maxTurnMs` as a hard deadline and surface a failed turn rather than hanging forever.
- **Idle timeout can misclassify slow work** → Treat idle timeout as optional or diagnostic; sentinel remains the primary completion mechanism.
- **Killing a CLI can corrupt provider session state** → Prefer graceful termination first, use provider-specific policy, and keep `sigkill` as fallback only.
- **Prompt echo or terminal control output can pollute answers** → Provide ANSI stripping and prompt/sentinel stripping in the output pipeline.
- **Resume failure retry can duplicate side effects** → Default to failing loudly and require explicit user reset or future opt-in retry.
- **Settings changes can invalidate useful history** → Include settings in the signature for correctness; users can manually bind an external session if they need advanced reuse.
- **Copilot first-run semantics may differ from resume semantics** → Support separate `newArgs` and `resumeArgs`, even if a provider config chooses to make them identical.

## Migration Plan

1. Add schema fields while preserving old specs through optional defaults.
2. Implement the new adapter without changing `acp_stdio` or `command`.
3. Wire `interactive_command` into `joi agent serve`.
4. Add documentation and examples for Claude and Copilot.
5. Existing agents continue using their current transports.

Rollback is straightforward because the new transport is opt-in. Removing or changing an `interactive_command` spec stops using the new adapter without affecting existing ACP or command agents.

## Open Questions

- Should the first implementation use pipes only, or allocate a PTY for CLIs that behave differently when not attached to a terminal?
- What exact Claude and Copilot command syntaxes should be validated against installed versions before adding marketplace entries?
- Should Joi expose `joi agent session reset/list/bind` in the same change, or start with internal session records and add management commands later?
- Should actor-profile Claude settings be scaffolded automatically, or should Joi only create the directory and require users to provide the file?
