# `data/agents/` — profile-mode agent snapshots

This directory is the repository copy of the currently deployed Joi agent
profiles. Agent behavior now comes from profile files, not legacy skill files.

Each agent directory contains:

- `spec.json` — the daemon-generated profile-mode `AgentSpec` snapshot. It has
  `identity.files` and no bundle/bootstrap skill loading.
- `profile/identity.md` — stable role, responsibilities, input/output protocol,
  and workflow rules injected on every turn.
- `profile/soul.md` — operating contract and guardrails injected on every turn.
- Optional `profile/references/*` — profile-owned reference data used by the
  actor, for example classroom regression cases.

The checked-in profile files are intended to be synced to the machine data root
under:

```text
<machine-data-root>/agents/<actor_id>/profile/
```

## Current deployed agents

| Directory | Actor id | Provider | Purpose |
| --- | --- | --- | --- |
| `a1-bug-triage` | `actor_a1_bug_triage` | Claude | A1 feedback triage |
| `router` | `actor_router` | Claude | Human-facing routing and orchestration |
| `discovery` | `actor_discovery` | Claude | Task discovery, repo analysis, delivery handoff |
| `examiner` | `actor_examiner` | Claude | Spec/MR/design/terminal quality review |
| `delivery` | `actor_delivery` | Claude | Implementation, MR, review/CI handling |
| `classmaster` | `actor_classmaster` | Codex | Classroom training control plane |
| `teacher` | `actor_teacher` | Codex | Training homework/grading/profile patch authoring |
| `canfeng-codex` | `canfeng-codex` | Codex | General engineering seat |
| `canfeng-copilot` | `canfeng-copilot` | Copilot | General engineering seat |
| `canfeng-deepseek` | `canfeng-deepseek` | Claude | General engineering seat |
| `canfeng-glm` | `canfeng-glm` | Claude | General engineering seat |
| `canfeng-minimax` | `canfeng-minimax` | Claude | General engineering seat |

## Validation

All specs should be valid JSON and must not reference agent bundles:

```sh
find data/agents -name spec.json -print0 | xargs -0 -n1 jq empty
find data/agents -path '*/bundle/*' -print
```
