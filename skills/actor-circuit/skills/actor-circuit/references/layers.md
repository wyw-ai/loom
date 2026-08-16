# The Actor Foundation and L0/L1/L2 Model

## Status

This is a working model for organizing Multi-Actor engineering questions, not an established industry standard.

- The **Actor foundation** and **L0** form the defined conceptual core.
- **L1** has an experimental `v0alpha1` YAML representation for Actor circuits.
- **L2** has a preview taxonomy and initial entries in [l2.md](l2.md); most of its taxonomy and design guidance remains future work.

## Foundation: Actor

Actor answers **who participates**.

An Actor is an independently identifiable collaboration boundary with state, capabilities, and a lifecycle. Human, AI Agent, Service, Script, Team, Workflow instance, and External System may all be Actors when they participate through such a boundary.

Actor is not part of L0. It is the foundation imported by every layer.

Use [actor.md](actor.md) for the Actor model.

## L0: Collaboration Logic Circuit

L0 answers **how collaboration state changes**.

Its three core concepts are:

- **Collaboration State** — the state visible to collaboration logic;
- **Gate** — a stable state transition;
- **Circuit** — connected gates, State Elements, and continuations.

An L0 Circuit may describe a complete collaboration or only the fragment relevant to a design question. It does not require an executable start node or a globally closed workflow.

Collaboration State can be decomposed into named **State Elements** such as continuation, ownership, pending request/response or outcome boundaries, waiting conditions, retained work results, convergence satisfaction signals, and round counters. In the circuit analogy, they play a role similar to registers or latches.

One L0 Gate occurrence is one committed collaboration-state transition. Long-running interactions remain explicit: Call opens Boundary State, which Return or another declared outcome may resolve; Wait creates Wait State, which Resume or another declared outcome may resolve; Transfer is the ownership commit produced after any required L1 acceptance protocol.

L0 observes changes in Flow, Result Flow, Responsibility, and Lifecycle. Branch selects continuations; Fan-out turns several selected continuations into independently advancing branches. Fan-in observes satisfaction signals through Join State and enables a successor; it does not consume or merge work results. Wait State and Boundary State may have several possible outcomes, but one continuation must not be advanced twice by conflicting outcomes. L1 supplies the concrete identity, correlation, and commit mechanics.

L0 describes Sequence, Branch, Fan-out, Fan-in, Route, Call, Delegate, Transfer, Return, Wait, Resume, Stop, and related semantics without specifying transport or runtime implementation.

Use [l0.md](l0.md) for the current L0 specification.

## L1: Actor Circuit Protocol

L1 answers **how a concrete L0 circuit is represented as structured, state-aware, and time-aware data**.

Its responsibilities include:

- Actor references and instantiated L0 Gates;
- process edges and observable outcomes;
- State Elements and their ephemeral, durable, or external retention requirements;
- identity and addressing;
- message and artifact envelopes;
- synchronous and asynchronous continuation behavior;
- immediate and store-and-forward delivery;
- delivery and acknowledgement;
- wake activation and audience visibility for cross-Actor messages;
- task, branch, round, correlation, and causation identity;
- context and result carriage;
- responsibility-transfer acceptance;
- offline delivery and resumption signals;
- cancellation, duplication, idempotency, and late-result handling;
- permissions and observable lifecycle events.

L0 defines the semantics; L1 represents their concrete use. When a circuit chooses Fan-out followed by Fan-in, L1 represents Gate instances, process edges, branch identities, retained membership, satisfaction signals, Result State, and a convergence policy. Call and Return remain distinct transitions around Boundary State. A Handoff protocol represents context, delivery, and acceptance before producing the committed L0 Transfer transition.

L1 makes both the collaboration process and its cross-Actor interactions observable. It declares which facts require retention but does not choose the persistence backend or provide the entire end-to-end durability guarantee.

L1 separates four concerns that should not be collapsed: stable and comparable choices are typed parameters; task-dependent judgement remains in prompts; changing collaboration facts live in state; and evaluation-relevant traces are recorded as evidence. A concrete Circuit resolves parameters and prompts before an evaluation run without pretending that prompt semantics form a universal enum.

The current release provides one readable `kind: gate` YAML per L0 base Gate, separate YAML for selected configured forms, product-independent `kind: circuit` YAML, `kind: channel` guidance for concrete communication media, and product-binding YAML. These artifacts are an evolving interchange model for Humans, Agents, and future CLI tooling, not a finished universal execution or wire protocol. Use [l1.md](l1.md) for the model and its bundled examples.

## L2: Multi-Actor Systems

L2 answers **how Actors, L0 circuits, and L1 communication form a complete Multi-Actor system**.

Names such as Orchestrator–Worker, Group Chat, recursive delegation, and Evaluator–Optimizer belong here. A name alone is not a complete description: an L2 system also needs Actor boundaries, state ownership, circuits, protocol assumptions, convergence rules, failure behavior, and termination conditions.

The current release provides [l2.md](l2.md): a working-draft taxonomy built on two axes — continuation topology and decision locus — with full entries for a few observed structures and a candidate list for the rest. Design guidance beyond those entries remains future work.

## Scope boundary: one circuit, not a population

The model describes single collaboration structures — one circuit, one L2 composition — transition by transition. It does not describe the aggregate behavior of many concurrent circuits: throughput distributions, retry storms, load propagation, or other population-level questions are a separate, currently open problem. Do not stretch an L2 entry into a claim about system-level dynamics.

## Durable Execution Plane

Durable execution answers **which collaboration facts survive time and failure, and how unfinished execution continues**.

It is an execution concern across the numbered layers:

- L0 identifies the State Elements required by a circuit;
- L1 represents Gate instances and gives state, events, tasks, branches, and transfers explicit retention and interaction semantics;
- the runtime persists history, checkpoints, timers, leases, or equivalent recovery information;
- L2 selects and communicates the end-to-end reliability guarantee.

An in-process circuit may keep its State Elements in memory. A circuit that promises continuity across restarts or infrastructure failure needs an explicit durability boundary and recovery behavior. Concrete databases, logs, queues, and object stores remain runtime choices.

## Patterns Span the Layers

A collaboration design pattern resolves recurring forces by arranging roles, knowledge, authority, commitment, feedback, and exit behavior. It normally includes Context, Forces, Structure, Protocol, and Consequences.

Fan-out and Fan-in are independent L0 Gates. Using them together forms one possible L0 circuit; neither requires the other. Orchestrator–Worker is an L2 composition. None becomes a design pattern merely by receiving a name. A real pattern may span the Actor foundation, L0, L1, and L2.
