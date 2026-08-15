//! MemoryProvider — wraps the existing memory mechanism as a
//! ContextResource.
//!
//! Boundary (ARCH v2 §A6): MemoryProvider does NOT replace MemorySpec.
//! It wraps the existing memory mechanism so it participates in the
//! ContextResource chain when agentcontext.yml is present. When absent,
//! memory continues to work through build_envelope directly.
//!
//! In D2, the MemoryProvider is a thin wrapper that receives
//! pre-rendered memory strings (bootstrap and turn) from the caller,
//! ensuring output parity with D1's build_envelope memory sections.

use anyhow::Result;
use proto::types::ScopeKind;

use crate::envelope::PromptSection;

use super::{AssemblyContext, ContextResource};

/// Wraps existing memory store access for the ContextResource chain.
///
/// The actual memory loading is done by the caller (using the existing
/// build_envelope mechanism) and the rendered strings are passed to
/// this provider via a shared cell. This ensures D1 output parity
/// without duplicating the memory selection logic.
pub struct MemoryProvider {
    effective_scopes: Vec<ScopeKind>,
    /// Pre-rendered bootstrap memory text (set by the caller before
    /// assembly).
    bootstrap_text: String,
    /// Pre-rendered turn memory text.
    turn_text: String,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self {
            effective_scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
            bootstrap_text: String::new(),
            turn_text: String::new(),
        }
    }

    /// Set the pre-rendered memory strings. Called by the caller after
    /// running the existing memory selection logic.
    pub fn with_rendered(mut self, bootstrap: impl Into<String>, turn: impl Into<String>) -> Self {
        self.bootstrap_text = bootstrap.into();
        self.turn_text = turn.into();
        self
    }
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextResource for MemoryProvider {
    fn scheme(&self) -> &str {
        "memory"
    }

    fn priority(&self) -> i32 {
        5
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.effective_scopes
    }

    fn assemble(&self, _ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        let mut sections = Vec::new();

        let bootstrap = self.bootstrap_text.trim();
        if !bootstrap.is_empty() {
            sections.push(PromptSection::from_resource(
                "bootstrap_memory",
                "memory",
                bootstrap.to_string(),
            ));
        }

        let turn = self.turn_text.trim();
        if !turn.is_empty() {
            sections.push(PromptSection::from_resource(
                "turn_memory",
                "memory",
                turn.to_string(),
            ));
        }

        Ok(sections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::types::ScopeRef;
    use std::path::Path;

    #[test]
    fn scheme_and_priority() {
        let p = MemoryProvider::new();
        assert_eq!(p.scheme(), "memory");
        assert_eq!(p.priority(), 5);
    }

    #[test]
    fn empty_memory_produces_nothing() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MemoryProvider::new();
        let ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 1000,
            budget_total: 1000,
            delivery_context: "",
            first_turn: false,
        };
        let result = p.assemble(&ctx).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn rendered_memory_produces_sections() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MemoryProvider::new()
            .with_rendered("Bootstrap memory:\n- fact A", "Relevant memory:\n- note B");
        let ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 1000,
            budget_total: 1000,
            delivery_context: "",
            first_turn: false,
        };
        let result = p.assemble(&ctx).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "bootstrap_memory");
        assert_eq!(result[1].name, "turn_memory");
    }

    #[test]
    fn memory_sections_carry_resource_provenance() {
        // R1.1: memory sections are Resource-sourced; uri is exempted
        // (aggregate over the memory store, no single persistent uri —
        // ARCH design doc §1.3).
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MemoryProvider::new()
            .with_rendered("Bootstrap memory:\n- fact A", "Relevant memory:\n- note B");
        let ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 1000,
            budget_total: 1000,
            delivery_context: "",
            first_turn: false,
        };
        for section in p.assemble(&ctx).unwrap() {
            assert_eq!(
                section.source,
                context_layer_core::SectionSource::Resource {
                    scheme: "memory",
                    uri: None,
                }
            );
        }
    }

    #[test]
    fn memory_resource_honors_skip_not_truncate() {
        // R1.2 (AC-R1-3): under a tiny budget the memory resource must
        // skip sections entirely, never truncate them.
        use context_layer_core::test_support::assert_skip_not_truncate;

        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MemoryProvider::new().with_rendered(
            "Bootstrap memory:\n- fact A\n- fact B\n- fact C",
            "Relevant memory:\n- note B\n- note D",
        );
        let full_ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 10_000,
            budget_total: 10_000,
            delivery_context: "",
            first_turn: false,
        };
        let tiny_ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 1,
            budget_total: 10_000,
            delivery_context: "",
            first_turn: false,
        };
        assert_skip_not_truncate(&p, &full_ctx, &tiny_ctx);
    }
}
