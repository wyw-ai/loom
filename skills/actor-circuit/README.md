# Actor Circuit

> When Actors become circuits — the semantics beneath agent graphs.

```text
npx skills add wyw-ai/actor-circuit
```

One Agent can plan, write code, search, or operate tools. Putting several Agents together, however, does not automatically create a system — so today's agent frameworks converge on the same picture: nodes that do work, edges that route between them, and state flowing along the edges.

The picture shows how to draw a system. It does not say what an edge means. Is a message delivered, accepted, or committed to? Does a fan-in merge work results, or only observe completion signals? Is a paused branch alive, or stalled forever? Actor Circuit exists to make those meanings precise.

![From agent graph to actor circuit](docs/images/graph-to-circuit.png)

Imagine a small product-brainstorming team:

```text
Clarify the problem
  → ask several participants to think from different perspectives
  → retain their ideas
  → converge their completion signals
  → deduplicate, group, and combine the retained ideas
  → review
  → either brainstorm another round or stop
```

Replace product brainstorming with coding, incident response, travel planning, or customer support and some details change immediately: the participants, inputs, tools, and acceptance criteria are different. Remove those business-specific details, however, and the underlying relationships remain familiar. Work still continues, branches, fans out, converges, calls, delegates, transfers, waits, resumes, and stops.

We call these operations **collaboration logic gates**. Connect them around collaboration state and Actors begin to form a circuit. Stateful circuits also retain facts such as the current continuation, owner, pending request/response or outcome boundary, waiting condition, work-result reference, convergence signal, and round.

![The twelve collaboration logic gates](docs/images/gate-to-circuit.png)

## Not only Agents

Real AI-native systems include more than language-model Agents:

```text
Human → AI Agent → Script → Service → Human
```

This project uses **Actor** for any independently identifiable collaboration boundary with state, capabilities, and a lifecycle. A Human, Agent, Service instance, Script execution, Team, or Workflow instance may be an Actor.

Actor is the foundation, not L0.

## Three semantic layers

```text
Actor foundation
  Who participates

L0 — Collaboration Logic Circuit
  How Flow, Result Flow, Responsibility, and Lifecycle change

L1 — Actor Circuit Protocol
  How L0 circuits become structured, state-aware, time-aware data

L2 — Multi-Actor Systems
  How Actors, circuits, and protocol form complete systems
```

Durable execution runs across these layers. L0 identifies what a circuit must remember, L1 represents the circuit and its retention requirements, and the runtime decides how state survives time and failure.

An L0 Circuit may be a complete collaboration or a partial fragment used to explain one design question. L0 makes collaboration meaning visible; it does not require executable entry points, identifiers, or storage choices.

The Actor foundation and L0 form the defined conceptual core. L1 now has an experimental `v0alpha1` YAML representation for complete Actor circuits. Its design rule is simple: stable choices become parameters, task-dependent judgement stays in prompts, runtime facts become state, and evaluation claims require evidence. L2 is a preview: the skill's references include a two-axis taxonomy and initial pattern entries.

## Articles

- [Logic Gates of Multi-Actor Collaboration](docs/01-logic-gates-of-multi-actor-collaboration.md) — the structured L0 article.
- [When Agents Become Circuits](docs/01-logic-gates-of-multi-actor-collaboration-readable.md) — the narrative version, including stateful circuits and the boundary between L0 semantics and execution guarantees.

## Skill

[Actor Circuit](skills/actor-circuit/SKILL.md) gives a capable AI Agent this domain model without prescribing a step-by-step workflow. It does not hand your agent a standard workflow to follow; it teaches the agent to assemble the right circuit at runtime, gate by gate.

Its references contain:

- the [Actor foundation](skills/actor-circuit/references/actor.md);
- the [L0 professional specification](skills/actor-circuit/references/l0.md);
- the [experimental L1 model](skills/actor-circuit/references/l1.md), including one YAML definition per L0 Gate, Circuit YAML for recording and comparing complete processes, and channel guidance for concrete communication media;
- the [L2 preview](skills/actor-circuit/references/l2.md), a two-axis taxonomy of Multi-Actor system structures with full entries for a few of them;
- the [L0/L1/L2 boundaries](skills/actor-circuit/references/layers.md).

There is no CLI or frozen validation schema in this release. The L1 YAML files are intended to become future CLI input and product-integration anchors after they have been exercised against real systems.

## Install

```text
npx skills add wyw-ai/actor-circuit
```

This installs the [Actor Circuit skill](skills/actor-circuit/SKILL.md) into your coding agent — Claude Code, Codex, Cursor, Kimi Code CLI, and 70 more agents are supported through the [skills CLI](https://github.com/vercel-labs/skills).

## Provenance

The underlying workflow and interaction structures are not claimed as original inventions. This project reorganizes established research, existing framework designs, and Multi-Actor engineering experience around a common Actor foundation and a clear layer boundary.

Two further choices borrow a way of thinking from kinetic theory — the 2026 resolution of Hilbert's sixth problem (Deng, Hani, and Ma: deriving fluid equations from Newtonian particle dynamics via Boltzmann's equation). The mathematics itself is not part of this project; what carries over is discipline about scope and observation. First, the model declares its validity regime explicitly: it describes single collaboration circuits, not the aggregate behavior of populations of circuits, which remains an open problem. Second, its observation guidance reflects the same reason that irreversible macroscopic laws cannot be recovered from compressed statistics alone: aggregate metrics summarize a run, but where an interaction history contains feedback cycles, only structured traces and evidence preserve the correlations that explain behavior.
