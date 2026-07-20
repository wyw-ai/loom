# Runtime Awareness

This CI fixture keeps Loom Rust checks independent from the private official
runtime-content repositories.

Agents still receive runtime context through `AGENTS.md`, the default `loom`
skill, and `loom guide` topics. Production builds should embed the official
`loom-guide` repository instead of this fixture.
