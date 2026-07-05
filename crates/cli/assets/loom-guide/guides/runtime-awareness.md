# Runtime Awareness

Loom no longer relies on a long Loom-owned system prompt to teach agents how the runtime works. Stable runtime awareness is split across three surfaces:

- `AGENTS.md` in the agent workspace contains stable actor/channel facts and concise operating rules.
- The default `loom` skill routes common situations to the right action or guide topic.
- `loom guide` contains the detailed official manual.

Dynamic facts stay out of `AGENTS.md`: current trigger, thread, task, assignment, latest message, local time, private-trigger state, and other turn-specific values belong in the current prompt or environment.

## AGENTS.md

Loom writes a generated block into `{agent.workspace}/AGENTS.md` using these markers:

```markdown
<!-- BEGIN loom -->
...
<!-- END loom -->
```

Rules:

- If the file does not exist, Loom creates it.
- If the file exists without markers, Loom prepends the generated block.
- If the markers exist, Loom refreshes the block when stable actor/channel facts change.
- Content outside the markers is owned by the user or project and must be preserved.

The block should contain stable facts only:

- actor id and display name;
- channel id, title, topic, and stable membership;
- workspace path;
- AgentSpec static instructions;
- concise pointers to the Loom skill and guide.

## Runtime Environment

Inside a daemon-managed agent turn, common environment variables include:

```text
LOOM_SERVER
LOOM_DAEMON_SOCKET
LOOM_ACTOR
LOOM_CHANNEL_ID
LOOM_SCOPE_ID
LOOM_SCOPE_KIND
LOOM_REPLY_TARGET
LOOM_RUN_ID
LOOM_TRIGGER_MESSAGE_ID
LOOM_TRIGGER_ACTOR
LOOM_TRIGGER_PRIVATE
LOOM_TRIGGER_PRIVATE_TO
LOOM_TRIGGER_PRIVATE_TO_FLAGS
```

Treat these values as current-turn context. If a value is missing or ambiguous, query the server with `loom --json ...`.

## Prompt Boundary

Default provider prompts should not include a Loom runtime manual as a system prompt. Providers should receive the composed turn payload, usually `{prompt.full}`, while native instruction discovery reads `AGENTS.md` and projected skills from the workspace.

`prompt.system` may still exist as a configurable output for compatibility, memory, or user-defined provider templates, but Loom's default providers should not append a Loom-owned system prompt.

## State Freshness

Do not rely on memory or local files for current collaboration state. Before acting on mutable state, query Loom:

```bash
loom --json channel list
loom --json actor list
loom --json thread list
loom --json message read --target "$LOOM_REPLY_TARGET"
loom --json task list --source-message "$LOOM_TRIGGER_MESSAGE_ID"
loom --json inbox list
```

Use `--include-private` only when you intentionally need private messages addressed to you.
