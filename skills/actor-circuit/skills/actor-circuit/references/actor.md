# Actor Foundation

## Contents

1. [Definition](#definition)
2. [Operational test](#operational-test)
3. [Static definitions and active Actors](#static-definitions-and-active-actors)
4. [Boundary properties](#boundary-properties)
5. [Recursive boundaries](#recursive-boundaries)
6. [What Actor is not](#what-actor-is-not)
7. [Relationship to L0/L1/L2](#relationship-to-l0l1l2)

## Definition

An Actor is an independently identifiable collaboration boundary that has state, capabilities, and a lifecycle and can participate in collaboration-state transitions.

The abstract model is:

```text
Actor = Identity Boundary + Actor-local State + Capability + Lifecycle
```

- **Identity boundary** distinguishes one participant from another across a collaboration. L1 later defines how that identity is represented and addressed.
- **Actor-local state** is what the Actor may preserve or change within its boundary.
- **Capability** is what the Actor can contribute or commit to doing.
- **Lifecycle** makes its availability, activity, waiting, completion, and disappearance distinguishable where relevant.

The definition does not require intelligence, autonomy in the human sense, or use of a language model.

## Operational test

A participant can be treated as an Actor at a chosen level of abstraction when the collaboration can meaningfully distinguish:

- input crossing into the boundary;
- state or work advancing within the boundary;
- an observable outcome, refusal, failure, or lifecycle change crossing out;
- the participant from other participants over the relevant collaboration scope.

Actor is therefore not a fixed technology type. It is a modeling boundary selected for the collaboration being described.

The same technology can fall on either side of that boundary:

```text
Agent → internal library call
        The library stays inside the Agent boundary.

Agent → request → remote Service → observable outcome
                  The Service can be modeled as an Actor.
```

## Static definitions and active Actors

A type, source file, configuration, or role description is not an active Actor by itself.

| Static definition | Active collaboration boundary |
| --- | --- |
| Service code and API | A Service instance receiving input and producing outcomes |
| Workflow definition | A Workflow instance that can wait, resume, and terminate |
| Script file | A Script execution with concrete input and outcome |
| Agent prompt | A running Agent with a concrete identity and lifecycle |
| Team structure | A Team acting through a shared external boundary |

This distinction explains why a Service or Workflow **can** be an Actor without claiming that every Service definition or Workflow document **is** an Actor.

## Boundary properties

### Heterogeneous participants

Human, AI Agent, Service, Script, Team, Workflow instance, and External System can share the Actor abstraction. They need not have equal capabilities, intelligence, authority, or availability.

Their equality is semantic: the collaboration model does not require a new control language for every participant category.

### Boundary-relative autonomy

An Actor advances state within its boundary without exposing every internal step to the caller. This does not imply unrestricted autonomy. Policy, permissions, budgets, and supervision may constrain it.

### Observable outcomes

An Actor need not always succeed. Completion, partial completion, failure, refusal, blocking, waiting, and cancellation can all be meaningful outcomes.

### Stable identity, variable implementation

An Actor may change model, process, machine, or internal team membership while preserving its collaboration identity. Conversely, several executions of the same implementation may be different Actors.

## Recursive boundaries

Actor boundaries are recursive and viewpoint-dependent.

```text
External view:  Research Team = one Actor

Internal view:  Research Team
                ├─ Planner
                ├─ Search Actors
                └─ Reviewer
```

A Workflow instance may similarly be one Actor to its caller while containing an internal L0 circuit and several Actors.

Recursion does not erase internal responsibility. It only allows a higher-level circuit to treat a composite boundary as one participant.

## What Actor is not

- **Not a model.** An AI Actor may change its model without losing identity.
- **Not a process.** A process may stop while the Actor remains recoverable or addressable.
- **Not a role.** Planner and Reviewer are responsibilities that an Actor may assume in a collaboration.
- **Not necessarily a person.** Teams and systems may expose a valid Actor boundary.
- **Not automatically every tool.** A library call hidden inside an Actor boundary is an implementation detail; a separately identifiable Service participating in the collaboration may be an Actor.
- **Not a synonym for Agent.** Agent is one possible Actor implementation.

## Relationship to L0/L1/L2

Actor is an unnumbered foundation:

```text
Actor foundation  defines who can participate
L0                defines collaboration-state transitions
L1                represents circuits and cross-Actor interaction semantics
L2                composes Actors, circuits, and protocol into systems
```

Actor identity exists conceptually at the foundation. Concrete identifiers, addresses, discovery, authentication, and delivery belong to L1.
