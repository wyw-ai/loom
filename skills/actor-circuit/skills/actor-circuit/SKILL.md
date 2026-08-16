---
name: actor-circuit
description: Actor-centered vocabulary for understanding, explaining, designing, or reviewing collaboration among humans, AI agents, services, scripts, teams, and workflow instances through the L0/L1/L2 Multi-Actor model. Use for Multi-Agent architecture, orchestration, delegation, handoffs, routing, fan-out/fan-in, result flow, waits, retries, termination, responsibility flow, or when distinguishing a primitive gate, a composed circuit, and a complete system.
---

# Actor Circuit

Use the L0/L1/L2 model as a vocabulary for reasoning about collaboration. Preserve the user's problem and choose the representation that makes the relevant relationships clearest.

Read [references/layers.md](references/layers.md) for the layer boundaries and maturity of the model.

Read [references/actor.md](references/actor.md) whenever the task depends on deciding what participates as an Actor or where an Actor boundary lies.

Read [references/l0.md](references/l0.md) whenever the task involves collaboration-state transitions, gates, state elements, circuits, path shape, responsibility flow, waiting, convergence, repetition, or termination.

Read [references/l1.md](references/l1.md) whenever the task needs a structured Actor-circuit definition or involves identity, process edges, state retention, communication timing, delivery, acknowledgement, correlation, asynchronous handoff, offline work, resumption, duplication, late results, or product integration.

Read [references/l2.md](references/l2.md) whenever the task involves a complete Multi-Actor system — composing Actors, circuits, and communication into something that must converge, survive failures, and terminate.

Treat the Actor foundation and L0 as the defined conceptual core. Treat L1 as an experimental `v0alpha1` interchange model and L2 as a preview. Do not present either as an established industry standard.

Treat L0 as the semantic source of truth. If an experimental L1 YAML artifact collapses several L0 committed transitions into one hidden protocol or otherwise conflicts with L0, report the mismatch instead of redefining L0 from the YAML.

When creating or analyzing a Multi-Actor system, make the consequential relationships visible: Actor boundaries, Gates, State Elements, Flow, Result Flow, execution ownership, continuation ownership, unresolved outcomes, convergence, repetition, and termination scope. An L0 Circuit may be a partial conceptual fragment; do not invent executable entry points, identifiers, storage, or protocol details merely to make it look complete.

When translating one L0 Gate into structured data, use its readable `kind: gate` YAML under `assets/l1/gates/`. Keep configured policies and circuit forms distinct under `assets/l1/configured/`.

When the collaboration runs over a concrete communication medium, consult the matching `kind: channel` YAML under `assets/l1/channels/` before fixing communication choices; it names the pitfalls the medium forces and the questions a product must answer.

When a complete process must be recorded, frozen for evaluation, compared, or bound to a product, express it as a `kind: circuit` YAML under `assets/l1/circuits/`. Describe Actors, memory, steps, Gate names, continuations, and communication choices with the smallest readable vocabulary. The bundled Circuits are instances to reference, not starting points for design: Gates and channel assets constrain the design, while the agent remains responsible for the concrete interaction. Map the Circuit to a concrete product with a separate `kind: product-binding` file based on `assets/l1/bindings/product-binding-template.yaml`.

When designing L1, put stable, comparable choices in typed `parameters`; put task-dependent judgement in `prompt_slots`; put runtime facts in `state`; and put auditable run facts in `evidence`. In a Circuit, select Gate parameters under `with` and supply prompt text or references under `prompts` and `prompt`. Freeze resolved parameters and prompts before an evaluation run. Do not invent a closed enum for a fuzzy judgement merely to make it easier to validate.

Keep these guardrails explicit:

- The Actor foundation defines participants; L0 defines collaboration semantics; L1 represents concrete interaction; L2 composes complete systems.
- One Gate occurrence is one committed transition. Do not hide delivery, acceptance, waiting, and commitment inside it.
- Distinguish an event or signal, a Gate outcome, a completion signal, and a work result.
- Branch selects continuations; Fan-out creates independently advancing branches.
- Fan-in updates Join State from satisfaction signals and may enable one continuation once. It does not process work-result content.
- Boundary State, Wait State, and Join State may have several outcomes, but conflicting outcomes must not advance the same continuation twice.
- Route selects an Actor; Delegate changes bounded execution ownership; Transfer changes continuation ownership.
- A message that requests action must wake its recipient; natural-language urgency does not advance a continuation.
- A reply stays within the audience of the message that prompted it; naming an Actor as subject does not add it to the audience.
- Cancel requests termination; Stop is committed termination of a declared scope.
- A Circuit or named orchestration structure is not automatically a collaboration design pattern.
- Aggregate metrics summarize a run; they do not substitute for trace evidence, especially where the interaction history contains feedback cycles — review loops, repeated pairwise exchanges, group discussion, shared boards.

A ProductBinding may map or compensate for Circuit semantics, but must not silently equate delivery, acceptance, and commitment.

Do not force YAML when prose, a table, a diagram, or a review better serves the user's purpose. Use YAML when the artifact must be reused, compared, validated, or anchored to a product.
