# Runtime Awareness

Stable facts are in `AGENTS.md`; current-turn facts are in the prompt and
environment.

## Stable Facts

Read `AGENTS.md` for:

- actor id and display name;
- channel id, title, topic, and members;
- workspace path;
- stable AgentSpec instructions;
- pointers to Loom guide and this skill.

Loom owns only the generated block between:

```markdown
<!-- BEGIN loom -->
...
<!-- END loom -->
```

Project or user rules outside that block must be preserved.

## Dynamic Facts

Treat these as current-turn context:

```text
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

Do not write dynamic turn facts into `AGENTS.md`.

## Prompt Boundary

Loom no longer depends on a long Loom-owned system prompt. Provider defaults
should consume the composed turn payload and rely on workspace-native discovery
for `AGENTS.md` and skills.
