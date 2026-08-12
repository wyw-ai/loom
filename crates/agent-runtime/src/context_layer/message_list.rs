//! MessageListProvider — wraps the D1 delivery_cursor_context as a
//! ContextResource.
//!
//! This is a thin wrapper: the actual message query still goes through
//! the existing RPC path (message.list, inbox.list). The provider formats
//! the pre-queried result as a PromptSection named "delivery_context"
//! (same name as current D1 output).

use anyhow::Result;
use proto::types::{ScopeKind, ScopeRef};

use crate::envelope::PromptSection;

use super::{AssemblyContext, ContextResource};

/// Wraps the existing delivery cursor context as a ContextResource.
///
/// The delivery context text is pre-queried by the caller and passed
/// through `AssemblyContext.delivery_context`. This provider simply
/// wraps it as a PromptSection, ensuring output parity with D1.
pub struct MessageListProvider {
    /// All scope kinds — message list is relevant in both thread and
    /// channel scopes.
    effective_scopes: Vec<ScopeKind>,
}

impl MessageListProvider {
    pub fn new() -> Self {
        Self {
            effective_scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
        }
    }
}

impl Default for MessageListProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextResource for MessageListProvider {
    fn scheme(&self) -> &str {
        "message-list"
    }

    fn priority(&self) -> i32 {
        10
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.effective_scopes
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        // Hot layer: reuse the pre-queried delivery_cursor_context output.
        // This produces the same PromptSection as D1's direct
        // delivery_cursor_context call.
        if ctx.delivery_context.trim().is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![PromptSection {
            name: "delivery_context",
            content: ctx.delivery_context.to_string(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn scheme_and_priority() {
        let p = MessageListProvider::new();
        assert_eq!(p.scheme(), "message-list");
        assert_eq!(p.priority(), 10);
    }

    #[test]
    fn empty_delivery_context_produces_nothing() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MessageListProvider::new();
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
    fn nonempty_delivery_context_produces_section() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let p = MessageListProvider::new();
        let ctx = AssemblyContext {
            scope: &scope,
            channel_id: None,
            actor_id: "test_actor",
            profile_dir: Path::new("/tmp"),
            budget_remaining: 1000,
            budget_total: 1000,
            delivery_context: "=== Latest messages ===\nHello world",
            first_turn: false,
        };
        let result = p.assemble(&ctx).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "delivery_context");
        assert!(result[0].content.contains("Hello world"));
    }
}
