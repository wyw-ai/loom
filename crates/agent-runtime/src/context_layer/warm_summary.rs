//! WarmSummaryContextResource — Warm summary layer as a ContextResource.
//!
//! Migrated from agent_serve.rs D1 functions to the D2 ContextResource chain.
//! Reads persisted summary files (`{profile_dir}/summaries/{scope_id}.md`)
//! and assembles them into a PromptSection, respecting the 20% budget
//! fraction of remaining tokens.
//!
//! # Constraints
//!
//! - C-1: Summary content is provider-generated; this resource only reads.
//! - C-3: Never truncates or compresses — skips injection if over budget.
//! - C-5: Summary is produced via a dedicated provider call (summary prompt).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::types::{ScopeKind, ScopeRef};

use crate::envelope::PromptSection;
use crate::usage::estimate_tokens;

use super::{AssemblyContext, ContextResource};

/// Maximum fraction of the remaining token budget that the Warm summary
/// section may occupy (ARCH §B5: 20%).
const WARM_SUMMARY_BUDGET_FRACTION: f64 = 0.2;

/// Warm summary ContextResource.
///
/// Reads persisted summary files and assembles them into a PromptSection.
/// Priority 7 places it between memory (5) and message-list (10) in the
/// chain ordering (ARCH §A4).
pub struct WarmSummaryContextResource {
    effective_scopes: Vec<ScopeKind>,
}

impl WarmSummaryContextResource {
    pub fn new() -> Self {
        Self {
            effective_scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
        }
    }

    // -----------------------------------------------------------------------
    // File-system helpers (migrated from agent_serve.rs)
    // -----------------------------------------------------------------------

    /// Directory under the actor profile where Warm summaries are stored.
    fn summary_dir(profile_dir: &Path) -> PathBuf {
        profile_dir.join("summaries")
    }

    /// File path for a given scope's Warm summary.
    pub fn summary_path(profile_dir: &Path, scope_id: &str) -> PathBuf {
        Self::summary_dir(profile_dir).join(format!("{scope_id}.md"))
    }

    /// Load a persisted Warm summary for the given scope. Returns `None` if
    /// no summary exists or the file cannot be read.
    fn load(profile_dir: &Path, scope_id: &str) -> Option<String> {
        let path = Self::summary_path(profile_dir, scope_id);
        let text = std::fs::read_to_string(&path).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Persist a provider-generated Warm summary for the given scope.
    /// Writes atomically (temp + rename) so a crash mid-write does not
    /// corrupt an existing summary.
    pub fn persist(profile_dir: &Path, scope_id: &str, summary: &str) -> Result<()> {
        let dir = Self::summary_dir(profile_dir);
        crate::acp::create_dir_all_unc(&dir)
            .with_context(|| format!("create warm summary dir {}", dir.display()))?;
        let final_path = Self::summary_path(profile_dir, scope_id);
        let temp_path = final_path.with_extension("md.tmp");
        std::fs::write(&temp_path, summary)
            .with_context(|| format!("write warm summary tmp {}", temp_path.display()))?;
        std::fs::rename(&temp_path, &final_path)
            .with_context(|| format!("rename warm summary {}", final_path.display()))?;
        Ok(())
    }

    /// Remove a persisted Warm summary (e.g. when the scope is deleted).
    /// Errors are logged but not propagated — a stale summary file is
    /// harmless.
    pub fn clear(profile_dir: &Path, scope_id: &str) {
        let path = Self::summary_path(profile_dir, scope_id);
        if path.exists() {
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::debug!(path = %path.display(), %err, "failed to remove warm summary");
            }
        }
    }
}

impl Default for WarmSummaryContextResource {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextResource for WarmSummaryContextResource {
    fn scheme(&self) -> &str {
        "warm-summary"
    }

    fn priority(&self) -> i32 {
        7
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.effective_scopes
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        let raw = match Self::load(ctx.profile_dir, &ctx.scope.id) {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        // Budget check: 20% of remaining budget (C-3: skip, never truncate).
        let summary_budget = ((ctx.budget_remaining as f64) * WARM_SUMMARY_BUDGET_FRACTION) as u64;
        let summary_tokens = estimate_tokens(&raw);
        if summary_tokens > summary_budget && summary_budget > 0 {
            tracing::debug!(
                scope = %ctx.scope.id,
                summary_tokens,
                budget = summary_budget,
                "warm summary exceeds budget; skipping injection (not truncating per C-3)"
            );
            return Ok(Vec::new());
        }

        Ok(vec![PromptSection {
            name: "warm_summary",
            content: format!(
                "=== Context: Warm summary ===\n\
                 The following structured summary was generated by the provider to \
                 preserve context from earlier in this session. Use it as background \
                 for the current turn.\n\n{raw}"
            ),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_and_priority() {
        let r = WarmSummaryContextResource::new();
        assert_eq!(r.scheme(), "warm-summary");
        assert_eq!(r.priority(), 7);
    }

    #[test]
    fn effective_scope_is_thread_and_channel() {
        let r = WarmSummaryContextResource::new();
        assert_eq!(r.effective_scope(), &[ScopeKind::Thread, ScopeKind::Channel]);
    }

    #[test]
    fn no_summary_produces_empty() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "nonexistent_scope".into(),
        };
        let r = WarmSummaryContextResource::new();
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
        let result = r.assemble(&ctx).unwrap();
        assert!(result.is_empty());
    }
}
