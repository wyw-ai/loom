//! ContextResourceBuilder — Builder DSL for creating ContextResources.
//!
//! Eliminates the boilerplate of implementing the ContextResource trait
//! directly. Developers provide only a scheme name, priority, scopes, and
//! an assemble closure; the builder handles the rest.
//!
//! # Example
//!
//! ```ignore
//! use agent_runtime::ContextResourceBuilder;
//!
//! let resource = ContextResourceBuilder::new("my-scheme")
//!     .priority(10)
//!     .assemble(|ctx| {
//!         Ok(vec![PromptSection::from_resource(
//!             "my_section",
//!             "my-scheme",
//!             "Hello world".into(),
//!         )])
//!     })
//!     .build();
//! ```

use anyhow::Result;
use proto::types::ScopeKind;

use crate::envelope::PromptSection;

use super::{AssemblyContext, ContextResource};

/// Builder for creating ContextResources without implementing the trait
/// directly. Uses a closure-based assemble function.
pub struct ContextResourceBuilder {
    scheme: String,
    priority: i32,
    scopes: Vec<ScopeKind>,
    assembler: Option<Box<dyn Fn(&AssemblyContext<'_>) -> Result<Vec<PromptSection>> + Send + Sync>>,
}

impl ContextResourceBuilder {
    /// Create a new builder with the given scheme name.
    /// Defaults: priority = 15, scopes = [Thread, Channel].
    pub fn new(scheme: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            priority: 15,
            scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
            assembler: None,
        }
    }

    /// Set the priority (lower = earlier in chain, 0 = always included).
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set the effective scopes (default: Thread + Channel).
    pub fn scopes(mut self, scopes: Vec<ScopeKind>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Provide the assemble closure that produces PromptSections.
    pub fn assemble<F>(mut self, f: F) -> Self
    where
        F: Fn(&AssemblyContext<'_>) -> Result<Vec<PromptSection>> + Send + Sync + 'static,
    {
        self.assembler = Some(Box::new(f));
        self
    }

    /// Build a boxed ContextResource. Panics if no assemble closure was
    /// provided (call `.assemble()` before `.build()`).
    pub fn build(self) -> Box<dyn ContextResource> {
        let assembler = self.assembler.expect(
            "ContextResourceBuilder::build() called without an assemble closure; \
             call .assemble(|ctx| ...) before .build()",
        );
        Box::new(ClosureContextResource {
            scheme: self.scheme,
            priority: self.priority,
            scopes: self.scopes,
            assembler,
        })
    }
}

/// Internal adapter that implements ContextResource via a stored closure.
struct ClosureContextResource {
    scheme: String,
    priority: i32,
    scopes: Vec<ScopeKind>,
    assembler: Box<dyn Fn(&AssemblyContext<'_>) -> Result<Vec<PromptSection>> + Send + Sync>,
}

impl ContextResource for ClosureContextResource {
    fn scheme(&self) -> &str {
        &self.scheme
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.scopes
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        (self.assembler)(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn builder_produces_correct_scheme_and_priority() {
        let resource = ContextResourceBuilder::new("test-scheme")
            .priority(42)
            .assemble(|_| Ok(vec![]))
            .build();

        assert_eq!(resource.scheme(), "test-scheme");
        assert_eq!(resource.priority(), 42);
    }

    #[test]
    fn builder_default_scopes_are_thread_and_channel() {
        let resource = ContextResourceBuilder::new("test")
            .assemble(|_| Ok(vec![]))
            .build();

        assert_eq!(
            resource.effective_scope(),
            &[ScopeKind::Thread, ScopeKind::Channel]
        );
    }

    #[test]
    fn builder_assemble_closure_invoked() {
        let scope = proto::types::ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
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

        let resource = ContextResourceBuilder::new("test")
            .assemble(|_| {
                Ok(vec![PromptSection::from_resource(
                    "hello",
                    "test",
                    "world".into(),
                )])
            })
            .build();

        let result = resource.assemble(&ctx).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "hello");
        assert_eq!(result[0].content, "world");
    }
}
