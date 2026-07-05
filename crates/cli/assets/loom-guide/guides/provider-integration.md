# Provider Integration

Loom provider manifests describe how a local agent CLI receives the prompt and how Loom decodes its output. Providers should not own Loom runtime rules.

## Default Prompt Shape

Default providers should receive the composed turn payload, usually:

```text
{prompt.full}
```

Loom default providers should not append a Loom-owned system prompt such as `--append-system-prompt {prompt.system}`. The long runtime manual belongs in `loom-guide`, with the `loom` skill and `AGENTS.md` acting as routing surfaces.

`prompt.system` remains available for compatibility and user-defined templates, but it should not contain the Loom runtime manual by default.

## Native Instruction Discovery

Loom writes stable runtime context to:

```text
{agent.workspace}/AGENTS.md
```

Providers that support native instruction discovery should point at the workspace or run with the workspace as cwd. Do not point instruction discovery at `loom_agent_home` as the default Loom rules location.

## Skill Projection

Loom projects skills into provider-native directories in the current workspace:

```text
skills/
.agents/skills/
.claude/skills/
.qoder/skills/
.opencode/skills/
```

Every agent should receive the default `loom` skill. Channel/member scope skills and actor bundle skills are projected alongside it.

## Sessions

Provider sessions are scoped by Loom according to the provider manifest. Session reuse must not be used as the only source of runtime state because turns can be resumed, restarted, or compacted. Query Loom for fresh collaboration state and read `AGENTS.md` for stable actor/channel context.

## Output Contract

Visible collaboration output should be sent through Loom CLI commands. Provider stdout or assistant text is decoded as run output and trace data; it is not a substitute for a visible channel/thread message unless the adapter explicitly maps it that way.
