## ADDED Requirements

### Requirement: Interactive command transport
The system SHALL support an `interactive_command` agent transport for command-line agents whose process lifetime is not the turn completion boundary.

#### Scenario: Load interactive transport
- **WHEN** `joi agent serve` loads an agent spec whose transport kind is `interactive_command`
- **THEN** the system SHALL construct an interactive command adapter for that actor

#### Scenario: Preserve existing transports
- **WHEN** `joi agent serve` loads specs using `acp_stdio` or `command`
- **THEN** the system SHALL preserve their existing adapter behavior

### Requirement: Scope-bound provider sessions
The system SHALL maintain one provider session per actor and Joi scope, where a scope is either `thread:<id>` or `channel:<id>`.

#### Scenario: First handoff in thread creates provider session
- **WHEN** an interactive command actor receives its first handoff in a thread scope
- **THEN** the system SHALL create and persist a provider session id for that actor and thread

#### Scenario: Later handoff in same thread resumes provider session
- **WHEN** the same interactive command actor receives another handoff in the same thread scope
- **THEN** the system SHALL resume the previously persisted provider session id

#### Scenario: Channel common area has separate provider session
- **WHEN** an interactive command actor receives handoffs in a channel common area and in a thread under that channel
- **THEN** the system SHALL use separate provider session ids for the channel scope and the thread scope

#### Scenario: Different actors do not share provider sessions
- **WHEN** two different interactive command actors receive handoffs in the same thread
- **THEN** the system SHALL maintain separate provider session ids for each actor

### Requirement: Session signature invalidation
The system SHALL create a new provider session when the saved provider session signature no longer matches the active actor configuration.

#### Scenario: Command signature changes
- **WHEN** an interactive command actor has a saved session and its command or argument templates change
- **THEN** the next handoff SHALL create a new provider session instead of resuming the saved one

#### Scenario: Model changes
- **WHEN** an interactive command actor has a saved session and the active selected model changes
- **THEN** the next handoff SHALL create a new provider session instead of resuming the saved one

#### Scenario: Settings signature changes
- **WHEN** an interactive command actor has a saved session and its resolved Claude settings mode or settings path changes
- **THEN** the next handoff SHALL create a new provider session instead of resuming the saved one

### Requirement: Configurable new and resume commands
The system SHALL let interactive command specs define separate argument templates for creating a provider session and resuming a provider session.

#### Scenario: Claude first-run args
- **WHEN** a Claude interactive command spec defines `newArgs` as `["{prompt}", "--session-id", "{session_id}"]`
- **THEN** the first handoff for a scope SHALL invoke Claude with the resolved prompt and generated session id

#### Scenario: Claude resume args
- **WHEN** a Claude interactive command spec defines `resumeArgs` as `["{prompt}", "--resume", "{session_id}"]` and a session exists for the scope
- **THEN** later handoffs for that scope SHALL invoke Claude with the saved session id

#### Scenario: Copilot same args for create and resume
- **WHEN** a Copilot interactive command spec defines both `newArgs` and `resumeArgs` using `--resume {session_id}`
- **THEN** the system SHALL use the same Joi-managed session id for first and later invocations of that scope

### Requirement: Model argument support
The system SHALL append a provider model argument for interactive command actors only when an active model is configured.

#### Scenario: Configured model appends model argument
- **WHEN** an interactive command actor has active model `claude-sonnet-4.6`
- **THEN** the system SHALL append `--model=claude-sonnet-4.6` to the resolved command arguments

#### Scenario: Missing model omits model argument
- **WHEN** an interactive command actor has no active model configured
- **THEN** the system SHALL NOT append any model argument to the resolved command arguments

#### Scenario: Model argument follows session arguments
- **WHEN** the system resolves `newArgs` or `resumeArgs` for an interactive command actor with an active model
- **THEN** the system SHALL append the `--model=<model>` token after the resolved session arguments

### Requirement: Prompt completion contract
The system SHALL be able to inject a completion contract into interactive command prompts so the agent knows how to signal that its final reply is complete.

#### Scenario: Default sentinel contract
- **WHEN** an interactive command actor does not override the completion sentinel
- **THEN** the system SHALL instruct the agent to output `__JOI_DONE__` on a line by itself after the final user-visible answer

#### Scenario: Custom sentinel contract
- **WHEN** an interactive command actor configures a custom sentinel
- **THEN** the system SHALL inject that sentinel into the prompt completion contract

#### Scenario: Contract preserves user message
- **WHEN** the system builds an interactive command prompt
- **THEN** the system SHALL preserve the original user message in a clearly delimited user message section

### Requirement: Completion detection
The system SHALL detect interactive command turn completion using configured output completion rules.

#### Scenario: Sentinel completes turn
- **WHEN** the adapter reads the configured sentinel from interactive command output
- **THEN** the system SHALL mark the provider command output complete for that turn

#### Scenario: Sentinel stripped from final message
- **WHEN** the adapter emits the final user-visible text for a completed interactive command turn
- **THEN** the emitted text SHALL NOT include the configured sentinel when sentinel stripping is enabled

#### Scenario: Hard timeout fails turn
- **WHEN** an interactive command turn exceeds the configured maximum turn duration before completion is detected
- **THEN** the system SHALL fail the turn and apply the configured timeout kill policy

### Requirement: Done contract
The system SHALL treat an interactive command turn as successfully done only after the configured completion signal is detected and adapter-side completion work is finished.

#### Scenario: Successful done requires sentinel
- **WHEN** an interactive command process exits without emitting the configured sentinel
- **THEN** the system SHALL NOT treat the turn as successfully done

#### Scenario: Successful done emits final text and finished event
- **WHEN** the adapter detects the configured sentinel and has final user-visible text
- **THEN** the system SHALL emit the final text without the sentinel and SHALL emit a successful finished event

#### Scenario: Successful done updates provider session
- **WHEN** the adapter completes a first-run interactive command turn successfully
- **THEN** the system SHALL persist or update the provider session record before reporting the turn as successfully finished

#### Scenario: Kill failure prevents successful done
- **WHEN** completion is detected but the configured completion kill policy and fallback fail to terminate a still-running child process
- **THEN** the system SHALL report the adapter turn as failed instead of successful

### Requirement: Kill policy
The system SHALL let actor specs configure how interactive command processes are terminated after completion, cancellation, and timeout.

#### Scenario: Completion kill policy
- **WHEN** completion is detected and the child process remains alive
- **THEN** the system SHALL apply the actor's completion kill policy

#### Scenario: Cancellation kill policy
- **WHEN** a user cancels an open turn for an interactive command actor
- **THEN** the system SHALL apply the actor's cancellation kill policy and eventually emit a failed or cancelled finish event

#### Scenario: Fallback kill policy
- **WHEN** the configured graceful kill action does not terminate the child within the configured grace period
- **THEN** the system SHALL apply the configured fallback kill action

### Requirement: Output enhancement
The system SHALL support conservative output enhancement for interactive command output before it becomes Joi content.

#### Scenario: Strip ANSI output
- **WHEN** ANSI stripping is enabled for an interactive command actor
- **THEN** the system SHALL remove ANSI control sequences from emitted user-visible text

#### Scenario: Keep stderr out of final answer
- **WHEN** the interactive command writes to stderr
- **THEN** the system SHALL NOT mix stderr into the final user-visible answer by default

#### Scenario: Stream partial text
- **WHEN** the interactive command emits user-visible text before completion
- **THEN** the system MAY stream partial text as adapter text events before final completion

### Requirement: Claude settings policy
The system SHALL support Claude-specific settings policies for interactive command actors.

#### Scenario: Global Claude settings
- **WHEN** a Claude actor uses settings mode `global`
- **THEN** the system SHALL NOT add a `--settings` argument for Claude

#### Scenario: Actor-profile Claude settings
- **WHEN** a Claude actor uses settings mode `actor_profile`
- **THEN** the system SHALL pass `--settings {agent.profile}/claude/settings.json` after resolving the actor profile path

#### Scenario: Custom Claude settings
- **WHEN** a Claude actor uses settings mode `custom` with a path template
- **THEN** the system SHALL pass `--settings` with the resolved custom path

### Requirement: Resume failure behavior
The system SHALL fail loudly by default when a saved provider session cannot be resumed.

#### Scenario: Resume failure does not retry by default
- **WHEN** an interactive command invocation using a saved provider session fails with a session-not-found style error
- **THEN** the system SHALL report the turn as failed and SHALL NOT automatically rerun the prompt as a new provider session by default

#### Scenario: User reset creates new session
- **WHEN** a saved provider session mapping is reset for an actor and scope
- **THEN** the next handoff SHALL create a new provider session for that actor and scope
