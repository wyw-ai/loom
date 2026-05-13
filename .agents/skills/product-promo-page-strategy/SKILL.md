---
name: product-promo-page-strategy
description: Design the content strategy, copy, information architecture, feature narrative, and section ordering for product marketing or landing pages. Use this skill whenever the user asks how to design a good product promo page, landing page, product introduction page, SaaS/product website copy, feature page, conversion page, or asks to reference a specific product page's copy/function/structure, even if they explicitly say not to consider visual style.
---

# Product Promo Page Strategy

Use this skill to create or critique the non-visual design of a product宣传页: positioning, messaging, feature selection, section order, conversion path, proof points, and product narrative. Do not lead with colors, typography, gradients, or decorative style unless the user asks for visual design.

The goal is to make the page answer, in order:

1. What is this?
2. Who is it for?
3. Why should I care now?
4. What can it do?
5. How would I actually use it?
6. Why should I trust it?
7. What should I do next?

## Core Principle

A good product宣传页 is not a brochure. It is a structured decision path.

Each section should reduce one concrete uncertainty:

- Relevance: Is this for me?
- Capability: Can it solve my use case?
- Effort: How hard is it to start?
- Trust: Is it credible enough to try?
- Action: What is the next step?

If a section does not reduce uncertainty or move the user closer to action, remove or compress it.

## Reference Pattern: Bailian CLI Page

When a user references the Bailian CLI page or a similar technical product page, extract the following design logic:

- Lead with a specific product name and a concrete mechanism: "CLI", command name, API, SDK, plugin, or integration point.
- Put the one-line value proposition directly under the title. It should combine action, user benefit, and usage simplicity.
- Explain the product in terms of where it fits into the user's existing workflow, not only in terms of vendor capability.
- List capabilities as concrete modules with recognizable names, models, commands, or integrations.
- Show compatibility with tools the target audience already uses, grouped by category.
- Use a scenario matrix to connect "user/tool + capability + outcome", so the reader can map the product to real work.
- Prefer "how this plugs into your stack" over abstract brand messaging.
- For developer or AI infrastructure products, examples and commands often persuade better than adjectives.

This pattern is especially useful for CLI tools, APIs, model platforms, workflow engines, developer platforms, AI agent tools, and enterprise productivity infrastructure.

## Workflow

### 1. Identify The Page Job

Before drafting, infer or ask only if necessary:

- Target audience: buyer, evaluator, developer, operator, creator, or end user.
- Awareness level: cold visitor, solution-aware, product-aware, or existing user.
- Primary conversion: sign up, book demo, install, read docs, get API key, contact sales, or try a command.
- Product maturity: new concept, known category, migration alternative, add-on, or ecosystem tool.

Use these answers to decide page density:

- Cold or broad audience: start with problem and category context.
- Technical/evaluator audience: start with product mechanism, quick start, compatibility, and use cases.
- Existing user audience: start with new capability, migration path, examples, and docs.

### 2. Build The Narrative Spine

Create a concise spine before writing sections:

```text
For [audience],
[product] is a [category/mechanism]
that helps [primary job]
by [main capability or workflow advantage],
so they can [business/user outcome]
without [main friction].
```

Then derive:

- H1: product name, category, or literal offer.
- Subheading: one sentence with action + benefit + ease.
- First CTA: the lowest-friction next action.
- Secondary CTA: docs, demo, examples, or pricing depending on audience.

### 3. Choose Sections By Decision Sequence

Use this default order for technical/SaaS product pages:

1. Hero: product name, crisp value proposition, primary CTA, secondary CTA.
2. Quick proof: logo strip, usage count, ecosystem support, benchmark, or one concrete trust signal.
3. What it does: 4-8 concrete capabilities, each named by user task rather than internal module when possible.
4. How it works: 3-5 step workflow, command flow, integration path, or architecture overview.
5. Compatibility/ecosystem: tools, platforms, formats, models, frameworks, or systems grouped by how users think.
6. Use case matrix: audience/tool + capability + outcome.
7. Examples: commands, screenshots, workflow snippets, before/after, or sample output.
8. Trust and control: security, permissions, observability, reliability, compliance, or enterprise admin.
9. CTA close: repeat the next action with a stronger context-specific reason.

Adjust for other products:

- Consumer app: replace compatibility with social proof, outcomes, and moments of use.
- Enterprise product: add procurement, governance, ROI, security, deployment model, and migration.
- Creative/productivity tool: show before/after workflows and output examples early.
- New category: add a short "why now" or "what changed" section before feature depth.

### 4. Write Copy That Carries Product Understanding

Prefer concrete copy:

- Name the object: command, workspace, agent, workflow, report, file, dataset, campaign.
- Name the action: generate, inspect, sync, deploy, review, retrieve, transcribe, compare.
- Name the environment: Claude Code, Cursor, Slack, GitHub, CRM, data warehouse, browser, mobile.
- Name the outcome: fewer handoffs, faster launch, reliable audit trail, lower manual work, better conversion.

Avoid weak copy:

- "All-in-one", "seamless", "powerful", "next generation", "revolutionary", "unlock potential" without proof.
- Feature lists that do not say who uses the feature or when.
- Benefits that cannot be tied to a workflow.
- CTAs that are vague: "Learn more" as the only action.

For technical products, include at least one of:

- A command or code snippet.
- A workflow diagram described in words.
- A compatibility table.
- A use case matrix.
- A realistic sample input/output.

### 5. Feature Presentation Rules

Do not list every feature equally. Group features by user's mental model:

- By workflow stage: create, connect, automate, monitor, govern.
- By persona: developer, operator, analyst, marketer, admin.
- By input/output: text, image, audio, video, data, API, files.
- By integration surface: CLI, API, SDK, MCP, plugin, app, webhook.
- By job: search, generate, transform, review, publish, measure.

Each feature block should contain:

```text
Feature name: user-facing task
One-line value: what gets easier or possible
Proof/detail: command, model, integration, limit, output, or example
```

### 6. Layout Thinking Without Visual Style

When the user says "不考虑样式", still reason about page structure:

- Above the fold should establish category, audience relevance, and first action.
- Dense capability lists need grouping labels, not a long undifferentiated list.
- Matrices are useful when many tools/capabilities/use cases combine.
- Repeated sections should have parallel phrasing so readers can scan.
- Put examples close to the feature they prove.
- Do not hide the actual product mechanism until late in the page.
- Every CTA should be placed after a reason to act, not randomly inserted.

## Output Formats

### If Creating A Page Strategy

Return:

```markdown
**定位**
[Audience, product category, primary promise, conversion goal]

**页面主线**
[Narrative spine in one paragraph]

**推荐结构**
1. [Section name]: [section job] / [key content]
2. ...

**关键文案**
- H1:
- Subheading:
- Primary CTA:
- Secondary CTA:
- Feature naming examples:

**功能呈现**
[Grouped feature architecture or matrix]

**需要补充的证据**
[Proof points, examples, assets, metrics, screenshots, demos, trust details]
```

### If Critiquing An Existing Page

Return:

```markdown
**主要问题**
[Prioritized issues that affect comprehension or conversion]

**可保留的部分**
[What already works and why]

**结构调整**
[Recommended section order and what each section should prove]

**文案修改方向**
[Specific rewrites or rewrite rules]

**缺失证据**
[Examples, integrations, use cases, trust signals, demos, metrics]
```

### If Asked For A Reusable Skill Or Checklist

Return a concise checklist grouped by:

- Positioning
- First screen
- Feature architecture
- Use cases and examples
- Trust
- Conversion
- Scanability

## Quality Bar

Before finalizing, check:

- Can a target user explain the product after reading only the hero and first feature section?
- Is the main CTA obvious and low-friction?
- Does the page show the product's operating mechanism, not just outcomes?
- Are feature names concrete enough to be used in navigation?
- Is there at least one realistic use case per major audience?
- Does every proof point appear near the claim it supports?
- Would deleting any section make the page less persuasive? If not, delete it.

