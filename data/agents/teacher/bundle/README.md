# teacher — bundle

Two-phase agent: **design** (turns task-goal/DoD into a `lesson-plan.md`)
and **validation** (scores delivery output against the DoD into a
`validation-report.json`).

- Consumes: `task-goal.json` (§1), `definition-of-done.json` (§2),
  delivery evidence artifacts.
- Produces: `lesson-plan.md` (§4), `validation-report.json` (§5).
- Hands off to: the executor named in the lesson plan (typically
  `delivery` or `lesson-designer`); after validation, hands back to
  `classmaster` / the upstream router.

Provider: `claude` via `interactive_command`. Same envelope as
`classmaster` (see `data/agents/README.md`); `maxTurnMs = 1800000`
since validation runs may exec evidence checks via skill tools.
