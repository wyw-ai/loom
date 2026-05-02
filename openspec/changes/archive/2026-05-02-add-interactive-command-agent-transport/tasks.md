## 1. Schema and Configuration

- [x] 1.1 Extend `AgentTransport` schema with `interactive_command` configuration blocks for session, prompt, completion, output, and kill policy.
- [x] 1.2 Add typed structs/enums for interactive session policy, prompt enhancement, completion detection, output enhancement, kill actions, and optional transport model.
- [x] 1.3 Add Claude settings policy schema with `global`, `actor_profile`, and `custom` modes.
- [x] 1.4 Ensure existing `acp_stdio` and `command` specs deserialize without behavior changes.

## 2. Session Lifecycle

- [x] 2.1 Implement provider session records keyed by `actor_id`, `scope.kind`, and `scope.id` for interactive command actors.
- [x] 2.2 Generate Joi-owned UUID provider session ids for first use when configured with `joi_uuid_per_scope`.
- [x] 2.3 Compute session signatures from command templates, transport kind, selected model, prompt contract version, and resolved settings policy.
- [x] 2.4 Create a new provider session when no record exists or when the saved signature differs from active configuration.
- [x] 2.5 Resume the saved provider session for later handoffs in the same actor and scope.
- [x] 2.6 Fail loudly on resume failure by default without automatically retrying the prompt as a new provider session.

## 3. Interactive Command Adapter

- [x] 3.1 Add an `InteractiveCommandAdapter` implementation in `agent-runtime`.
- [x] 3.2 Implement `newArgs` and `resumeArgs` template expansion with `{prompt}`, `{session_id}`, scope, actor, and agent path variables.
- [x] 3.3 Append `--model=<model>` after resolved `newArgs` / `resumeArgs` when an active model is configured, and append nothing when no model is configured.
- [x] 3.4 Spawn the configured CLI in the resolved scope workspace with resolved environment variables.
- [x] 3.5 Collect stdout/stderr or terminal output while preserving enough data to emit user-visible text and errors separately.
- [x] 3.6 Detect the configured completion sentinel and emit `AdapterEvent::Text` plus `AdapterEvent::Finished`.
- [x] 3.7 Enforce the Done contract: process exit without sentinel is failed, and success requires final text flush, session record update, completion kill policy, and successful `Finished`.
- [x] 3.8 Enforce `maxTurnMs` and fail the turn when completion is not detected in time.
- [x] 3.9 Implement adapter cancellation so user turn cancellation applies the configured cancellation kill policy.
- [x] 3.10 Apply completion and timeout kill policies, including grace period and fallback action.

## 4. Prompt and Output Enhancement

- [x] 4.1 Add prompt contract composition for interactive command actors using the existing Joi envelope.
- [x] 4.2 Inject the default `__JOI_DONE__` sentinel instruction when no custom sentinel is configured.
- [x] 4.3 Preserve the original user message in a clearly delimited section after Joi context and completion instructions.
- [x] 4.4 Strip the completion sentinel from final user-visible output when configured.
- [x] 4.5 Implement ANSI stripping for user-visible output when enabled.
- [x] 4.6 Keep stderr out of final user-visible messages by default and surface it through error or trace paths.

## 5. Claude Settings Policy

- [x] 5.1 Resolve `global` mode by omitting `--settings`.
- [x] 5.2 Resolve `actor_profile` mode to `{agent.profile}/claude/settings.json` and ensure the parent directory exists.
- [x] 5.3 Resolve `custom` mode using existing template variables and pass the resolved path through `--settings`.
- [x] 5.4 Include the resolved settings mode/path in the interactive command session signature.

## 6. `joi agent serve` Wiring

- [x] 6.1 Wire `transport.kind = "interactive_command"` to construct the new adapter.
- [x] 6.2 Pass scope workspace, scope env, template vars, selected model, and actor profile paths into the interactive adapter configuration.
- [x] 6.3 Preserve same-scope FIFO behavior for interactive command turns.
- [x] 6.4 Ensure channel scopes and thread scopes use distinct provider session records.

## 7. Tests

- [x] 7.1 Add schema deserialization tests for interactive command specs and Claude settings modes.
- [x] 7.2 Add unit tests for provider session keying across actor, thread scope, and channel scope.
- [x] 7.3 Add unit tests for signature invalidation on command, model, prompt contract, and settings changes.
- [x] 7.4 Add adapter tests for first-run args, resume args, session id template expansion, and conditional `--model=<model>` appending.
- [x] 7.5 Add completion detection tests for sentinel success, sentinel stripping, process exit without sentinel failure, and timeout failure.
- [x] 7.6 Add Done contract tests proving success requires final text flush, session record update, completion kill policy, and successful `Finished`.
- [x] 7.7 Add kill policy tests for completion, cancellation, grace period, and fallback.
- [x] 7.8 Add regression tests proving `acp_stdio` and `command` specs still route to existing adapters.

## 8. Documentation and Examples

- [x] 8.1 Document the `interactive_command` transport schema and lifecycle.
- [x] 8.2 Add Claude examples for `--session-id`, `--resume`, and settings modes.
- [x] 8.3 Add Copilot examples for Joi-managed session ids with `--resume`.
- [x] 8.4 Document the default one-session-per-agent-per-scope rule for both thread scopes and channel common-area scopes.
- [x] 8.5 Document completion contract guidance, the default `__JOI_DONE__` sentinel, and the Done contract.
