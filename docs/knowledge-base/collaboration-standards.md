# Collaboration Development Standards

> **Authority**: OPS (@actor_agent_ops_cf4161ee) — Operations & Knowledge Infrastructure
> **Status**: Active
> **Last Updated**: 2026-06-19

## Overview

This document defines the collaboration standards for the Loom development team. All actors (human and agent) must follow these standards when contributing to the project.

---

## 1. Team Structure

```
Founder
  ↓
CEO

STRAT  PM  ARCH  FE  BE  QA  OPS
```

### Authority Domains

| Actor | Authority |
|-------|-----------|
| CEO   | Coordination Authority |
| STRAT | Strategy & Market Intelligence Authority |
| PM    | Product Requirement Authority |
| ARCH  | Technical Design Authority |
| FE    | Frontend Implementation Authority |
| BE    | Backend Implementation Authority |
| QA    | Verification Authority |
| OPS   | Operations & Knowledge Infrastructure Authority |

### Core Principles

- Stay within your authority domain.
- Refuse work that belongs to another actor.
- Escalate ambiguity to CEO.
- Never redefine another actor's responsibilities.
- Never approve your own work.
- Use evidence over assumptions.
- Prefer explicit artifacts over informal discussion.

---

## 2. Workflow Sequence

### Standard Workflow

```
New Product / New Iteration:
  STRAT → PM → ARCH → FE/BE → QA → OPS

Bug Fix:
  PM → FE/BE → QA → OPS

Architecture Change (pure internal refactor):
  PM → ARCH → FE/BE → QA → OPS

Architecture Change (with tech selection):
  PM → STRAT → ARCH → FE/BE → QA → OPS

Infrastructure / Config Change:
  Direct execution by authorized role → post announcement in thread
  (production risk → OPS evaluation required)
```

### Lifecycle Rules

- Only QA may determine completion status.
- Only CEO communicates directly with Founder.
- Do not bypass workflow.
- Do not perform another actor's responsibilities.
- Do not change requirements without PM approval.
- Do not change architecture without ARCH approval.
- Do not deploy without OPS review.
- Do not mark work complete without QA approval.

---

## 3. Branch Naming Convention

| Branch Type | Pattern | Example |
|-------------|---------|---------|
| Feature iteration | `iter/<description>` | `iter/windows-stability-and-knowledge` |
| Bug fix PR | `pr/<description>` | `pr/windows-port-full-analysis` |
| Hotfix | `hotfix/<description>` | `hotfix/critical-server-crash` |
| Release | `release/<version>` | `release/v1.2.0` |

### Branch Flow

```
feature/iter branch → PR → dev → main
hotfix branch → PR → dev → main (cherry-pick to release if needed)
```

- `dev` is the integration branch; all feature work merges here first.
- `main` is the production branch; only merge from `dev` after full QA verification.
- Never commit directly to `dev` or `main` without PR workflow.

---

## 4. PR Workflow

### Creation

1. Create branch from latest `dev`.
2. Implement changes following the workflow sequence (STRAT → PM → ARCH → FE/BE → QA → OPS).
3. Each actor produces typed artifacts in their domain.
4. QA provides final acceptance verdict.

### Requirements

- PR title: concise description of change scope.
- PR description must include:
  - Root cause analysis (why).
  - Method description (how).
  - Solution design rationale.
  - Loom Native compliance assessment.
  - Test coverage summary.
  - Cross-platform impact (if applicable).
- All tests must pass before merge.
- ARCH review required for structural changes.
- QA sign-off required for completion.

### Merge Criteria

- All acceptance criteria met.
- ARCH approval (if applicable).
- QA verdict: PASS.
- OPS deployment readiness confirmed.
- No unresolved Founder escalation items at P0/P1 level.

---

## 5. Review Process

### Role-Based Review

| Change Type | Required Reviewers |
|-------------|-------------------|
| Architecture | ARCH |
| Requirements | PM |
| Code (FE) | FE |
| Code (BE) | BE |
| Tests | QA |
| Deployment | OPS |
| Strategy | STRAT |

### Review Checklist

- [ ] Code follows architecture.md boundaries.
- [ ] No process-boundary violations.
- [ ] No data-ownership breaches.
- [ ] Tests cover new/changed behavior.
- [ ] Cross-platform impact assessed.
- [ ] Documentation updated.
- [ ] No regression in existing functionality.

---

## 6. Communication

### Thread Discipline

- One activity = one thread. Do not scatter across multiple threads.
- Use `$LOOM_REPLY_TARGET` for all public messages in a thread.
- Private information uses `--private-to`, never public thread.
- Route messages to the actor who must act next with `message ask`.

### Task Lifecycle

- Claim source message before substantive work.
- Publish typed outputs with `loom task artifact attach`.
- Record evidence with `loom task fact append`.
- Complete assignments with `loom task assignment update --status completed`.

---

## 7. Decision Priority

```
Founder → CEO → Authority Owner → Implementers
```

### Artifact Priority

```
Approved Artifact > Discussion > Assumption
```

---

## Known Limitations

1. **No automated CI enforcement**: GitHub Actions quota exhaustion means workflow compliance is manual. Assessment: acceptable for current scale; CI re-enablement is a P1 follow-up.
2. **Agent context windows are stateless**: Each turn is a fresh session; agents rely on server-side state (thread messages, artifacts, facts) for continuity. Assessment: current Loom design handles this; cross-turn memory is server-side by design.
3. **No automated branch policy enforcement**: Branch protection rules are not configured on the GitHub repository. Assessment: team size is small (single human + agents); manual enforcement is manageable at current scale but should be automated as team grows.
4. **Windows-only loom-shell**: The `loom-shell` management GUI is Windows-only (`cfg(windows)`). Unix users must manage services manually. Assessment: acceptable for current Windows-priority development phase; Unix GUI equivalent is a future work item.
5. **No formal SLA for agent response times**: Agents may take 1-5 minutes to respond depending on provider latency and task complexity. Assessment: inherent to LLM-based agent architecture; not a collaboration standard defect.
