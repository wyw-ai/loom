//! Render a set of memory records as a labeled prompt section.

use super::record::MemoryRecord;

pub struct MemoryRenderer;

impl MemoryRenderer {
    /// Shown first, as "the durable things this agent should always know".
    pub fn render_bootstrap(records: &[MemoryRecord]) -> String {
        render_section("Bootstrap memory", records)
    }

    /// Shown second, as "records likely relevant to this turn".
    pub fn render_turn(records: &[MemoryRecord]) -> String {
        render_section("Relevant memory", records)
    }
}

fn render_section(title: &str, records: &[MemoryRecord]) -> String {
    if records.is_empty() {
        return format!("{title}:\n(none)");
    }
    let lines = records
        .iter()
        .map(|r| {
            let ty = if r.record_type.is_empty() {
                "note"
            } else {
                r.record_type.as_str()
            };
            let conf = if r.confidence.is_empty() {
                "unknown"
            } else {
                r.confidence.as_str()
            };
            format!("- [{ty} / {conf}] {}", r.summary.trim())
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{title}:\n{lines}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::MemorySource;

    fn rec(ty: &str, conf: &str, summary: &str) -> MemoryRecord {
        MemoryRecord {
            schema_version: 1,
            id: "x".into(),
            actor_id: "actor_test".into(),
            ts: "2026-04-01T00:00:00Z".into(),
            record_type: ty.into(),
            status: "accepted".into(),
            summary: summary.into(),
            detail: String::new(),
            confidence: conf.into(),
            source: MemorySource::default(),
            tags: vec![],
        }
    }

    #[test]
    fn empty_section_has_none_marker() {
        let s = MemoryRenderer::render_bootstrap(&[]);
        assert!(s.contains("(none)"));
    }

    #[test]
    fn rendered_lines_carry_type_and_confidence() {
        let s = MemoryRenderer::render_turn(&[
            rec("fact", "high", "user prefers Rust"),
            rec("decision", "medium", "chose tokio over async-std"),
        ]);
        assert!(s.contains("[fact / high] user prefers Rust"));
        assert!(s.contains("[decision / medium] chose tokio over async-std"));
    }

    #[test]
    fn missing_fields_get_defaults() {
        let s = MemoryRenderer::render_bootstrap(&[rec("", "", "bare")]);
        assert!(s.contains("[note / unknown] bare"));
    }
}
