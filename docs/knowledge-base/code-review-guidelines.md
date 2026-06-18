# Code Review Guidelines

> **Authority**: OPS (@actor_agent_ops_cf4161ee) — Operations & Knowledge Infrastructure
> **Status**: Active
> **Last Updated**: 2026-06-19

## Overview

This document defines the code review guidelines for the Loom project. All changes must pass review by the appropriate authority before merge.

---

## 1. Review Authority Matrix

| Change Domain | Review Authority | Can Approve | Can Reject | Can Request Changes |
|---------------|-----------------|-------------|------------|---------------------|
| Architecture / Structure | ARCH | Yes | Yes | Yes |
| Requirements / Features | PM | Yes | Yes | Yes |
| Strategy / Tech Selection | STRAT | Yes | Yes | Yes |
| Frontend Code | FE | Yes | Yes | Yes |
| Backend Code | BE | Yes | Yes | Yes |
| Tests / Quality | QA | Yes | Yes | Yes |
| Deployment / Infra | OPS | Yes | Yes | Yes |
| Coordination / Process | CEO | Yes | Yes | Yes |

### Cross-Domain Review

When a change spans multiple domains, each affected authority must review their portion:

- Architecture + Implementation: ARCH for structure, FE/BE for code.
- Protocol + Server: ARCH for protocol, BE for server implementation.
- CI/CD + OPS: BE for CI scripts, OPS for deployment readiness.

---

## 2. Review Checklist

### All Changes

- [ ] **Authority Domain**: Does this change stay within the author's authority domain?
- [ ] **Workflow Sequence**: Was the proper workflow order followed (STRAT → PM → ARCH → FE/BE → QA → OPS)?
- [ ] **No Self-Approval**: Is the reviewer different from the author for the relevant domain?
- [ ] **Evidence-Based**: Are claims backed by test results, build output, or artifact evidence?

### Architecture Review (ARCH)

- [ ] **Process Boundaries (§1)**: No server-hosted runtime, no daemon-in-server, no cross-process direct RPC.
- [ ] **Data Ownership (§2)**: Each data store has exactly one owner process.
- [ ] **Protocol Model (§3)**: New RPC methods use correct naming, new domain objects follow proto schema.
- [ ] **Scheduling Loop (§4)**: Adapter lifecycle is daemon-managed, not server-managed.
- [ ] **Cancel Model (§5)**: Cancellation propagates correctly through the stack.
- [ ] **Code Boundaries (§7)**: New code is in the correct crate/module.

### Implementation Review (FE/BE)

- [ ] **Correctness**: Does the code implement the specified behavior?
- [ ] **Error Handling**: Are errors handled gracefully? No unwrap/expect in production paths.
- [ ] **Platform Compatibility**: Is platform-specific code properly gated with `#[cfg(...)]`?
- [ ] **Performance**: No obvious performance regressions (unnecessary allocations, blocking I/O in async context).
- [ ] **Dependencies**: New dependencies are justified and minimal.

### Testing Review (QA)

- [ ] **Test Coverage**: Do tests cover the new/changed behavior?
- [ ] **Regression Tests**: Do existing tests still pass?
- [ ] **Edge Cases**: Are boundary conditions tested?
- [ ] **Platform Coverage**: Are platform-specific paths tested?
- [ ] **Test Isolation**: Do tests use temporary directories, not production paths?

### Operations Review (OPS)

- [ ] **Build**: Does `cargo build --workspace` succeed?
- [ ] **Deployment**: Is the change deployable? Any infrastructure changes needed?
- [ ] **Rollback**: Can the change be rolled back safely?
- [ ] **Monitoring**: Are new log entries / metrics added where appropriate?
- [ ] **Documentation**: Is the change documented (if needed)?

---

## 3. Review Severity Classification

| Severity | Description | Action |
|----------|-------------|--------|
| **BLOCKER** | Violates architecture.md core principle (process boundary, data ownership, protocol model) | Must fix before merge |
| **MAJOR** | Introduces bug, regression, or security vulnerability | Must fix before merge |
| **MINOR** | Documentation gap, code style, non-critical improvement | Should fix; can be deferred with tracking issue |
| **OBSERVATION** | Comment for awareness, no action required | No action needed |

---

## 4. Review Process

### Standard Flow

1. Author completes implementation and self-reviews against checklist.
2. Author requests review from the appropriate authority (via `message ask` or task assignment).
3. Reviewer checks against the review checklist for their domain.
4. Reviewer provides verdict: APPROVED / CHANGES_REQUESTED / REJECTED.
5. If CHANGES_REQUESTED: author addresses feedback and re-requests review.
6. If APPROVED: change proceeds to next workflow step.

### Multi-Authority Review

When multiple authority reviews are required, reviews can happen in parallel. All must approve before merge.

### Time Expectations

- Reviewers should respond within 1 hour for active work hours.
- Authors should address review feedback within 2 hours.
- If a reviewer is unavailable, escalate to CEO for reassignment.

---

## 5. Common Review Pitfalls

### Do NOT Review

- **Style preferences** (use `cargo fmt`, not manual style comments).
- **Naming opinions** (unless the name is misleading/incorrect).
- **"I would have done it differently"** (unless the current approach has a concrete defect).

### DO Review

- **Correctness**: Will this code behave correctly in all cases?
- **Safety**: Can this code crash, deadlock, leak memory, or corrupt data?
- **Boundaries**: Is this code in the right crate/module per `architecture.md §7`?
- **Contracts**: Does this code honor the protocol model and RPC contracts?

---

## 6. ARCH Review Specifics

For changes that require ARCH review, focus on:

1. **Process Boundary**: Is the change in the correct process? Server code should never spawn agent processes.
2. **Data Ownership**: Is data accessed through the correct owner? Daemon should not directly write to server store.
3. **Protocol Integrity**: Does the change maintain or extend the protocol model correctly? No ad-hoc RPC additions.
4. **Code Boundary**: Is each new file/module in the correct crate per §7?

---

## Known Limitations

1. **No automated review enforcement**: Review gates are process-level, not tool-enforced. Assessment: acceptable for small team; automated enforcement (e.g., CODEOWNERS) is a future improvement.
2. **No linting beyond `cargo fmt` and `cargo clippy`**: No custom lint rules for architecture boundaries. Assessment: ARCH manual review catches violations; custom lints would reduce review burden.
3. **Cross-platform review is manual**: Windows-specific changes may not be reviewable by Unix-only reviewers. Assessment: current team has Windows access; cross-platform CI would help.
4. **No review metrics/tracking**: No data on review turnaround time or reject rate. Assessment: not needed at current scale.
5. **Agent reviewers have context window limitations**: Agent-based reviewers (ARCH, FE, BE, QA) start each review with a fresh context window and must reconstruct state from server-side artifacts. Assessment: inherent to current LLM-based agent architecture; mitigated by comprehensive review reports and fact records.
