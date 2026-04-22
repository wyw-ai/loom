use chrono::{DateTime, Local, Utc};
use proto::types::{Event, RelationKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

#[derive(Debug, Clone)]
pub struct Bubble {
    pub actor_id: String,
    pub turn_id: Option<String>,
    pub kind: BubbleKind,
    pub text: String,
    pub ts: DateTime<Utc>,
    pub reply_to_event_id: Option<String>,
    pub trailing_event_id: Option<String>,
    pub delivery: DeliveryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BubbleKind {
    /// content.add (streamed); subsequent same-actor/turn chunks append in place
    Stream,
    /// non-streaming events (handoff, action.request/response, system)
    Static,
    /// system / informational lines (server hints, errors)
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    /// Not relevant (incoming events, system lines).
    NotApplicable,
    /// Outgoing message; waiting for the server to echo it back through the stream.
    Pending,
    /// Outgoing message; server has echoed it.
    Delivered,
}

#[derive(Default)]
pub struct History {
    pub bubbles: Vec<Bubble>,
}

pub struct RenderedHistory {
    pub lines: Vec<Line<'static>>,
    pub total_rows: u16,
    pub selected_row_range: Option<(u16, u16)>,
}

impl History {
    pub fn push_event(&mut self, ev: &Event) {
        match ev.kind.as_str() {
            // content.add carrying a HandsOffTo relation is a handoff; the
            // text body is the handoff message. Render it statically rather
            // than streaming so it stands apart from regular chat.
            "content.add" if hands_off_target(ev).is_some() => {
                self.push_static(ev, format_handoff(ev))
            }
            "content.add" => self.append_stream(ev),
            "action.request" => self.push_static(ev, format_action_request(ev)),
            "action.response" => self.push_static(ev, format_action_response(ev)),
            // turn.close is intentionally suppressed; delivery state on the
            // outgoing bubble already conveys "the server saw it", and the
            // closing chunk arrives as a normal content.add event.
            "turn.close" => {}
            other => self.push_static(ev, format!("{}: {}", other, ev.payload)),
        }
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.bubbles.push(Bubble {
            actor_id: "system".into(),
            turn_id: None,
            kind: BubbleKind::System,
            text: text.into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: None,
            delivery: DeliveryState::NotApplicable,
        });
    }

    /// Push our own outgoing content.add bubble immediately on send, with
    /// the server-assigned event id so we can flip to Delivered when it
    /// echoes back through stream/update.
    pub fn push_outgoing(
        &mut self,
        actor_id: &str,
        event_id: &str,
        ts: DateTime<Utc>,
        text: String,
        reply_to_event_id: Option<String>,
    ) {
        self.bubbles.push(Bubble {
            actor_id: actor_id.to_string(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text,
            ts,
            reply_to_event_id,
            trailing_event_id: Some(event_id.to_string()),
            delivery: DeliveryState::Pending,
        });
    }

    fn append_stream(&mut self, ev: &Event) {
        let text = ev
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(b) = self.bubbles.last_mut() {
            // If this is our own outgoing bubble we already pushed locally,
            // just flip to Delivered and skip — we'd otherwise duplicate the text.
            if b.delivery == DeliveryState::Pending
                && b.trailing_event_id.as_deref() == Some(ev.id.as_str())
            {
                b.delivery = DeliveryState::Delivered;
                return;
            }
            if b.kind == BubbleKind::Stream
                && b.actor_id == ev.actor_id
                && b.turn_id == ev.turn_id
                && b.delivery == DeliveryState::NotApplicable
            {
                b.text.push_str(&text);
                b.trailing_event_id = Some(ev.id.clone());
                return;
            }
        }
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::Stream,
            text,
            ts: ev.occurred_at,
            reply_to_event_id: reply_target(ev),
            trailing_event_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    fn push_static(&mut self, ev: &Event, text: String) {
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::Static,
            text,
            ts: ev.occurred_at,
            reply_to_event_id: reply_target(ev),
            trailing_event_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    pub fn pending_action_requests(&self) -> Vec<(String, String)> {
        let mut acks: std::collections::HashSet<String> = std::collections::HashSet::new();
        for b in &self.bubbles {
            if b.text.starts_with("✓ action.response → ") {
                if let Some(eid) = b.trailing_event_id.as_ref() {
                    acks.insert(eid.clone());
                }
            }
        }
        let mut out = Vec::new();
        for b in &self.bubbles {
            if b.text.starts_with("⌗ action.request") {
                if let Some(eid) = b.trailing_event_id.as_ref() {
                    if !acks.contains(eid) {
                        out.push((eid.clone(), b.text.clone()));
                    }
                }
            }
        }
        out
    }

    pub fn reply_targets(&self, display_for: &dyn Fn(&str) -> String) -> Vec<(String, String)> {
        self.bubbles
            .iter()
            .rev()
            .filter_map(|b| reply_target_label(b, display_for))
            .collect()
    }

    pub fn newest_replyable_index(&self) -> Option<usize> {
        self.bubbles.iter().rposition(is_replyable_bubble)
    }

    pub fn older_replyable_index(&self, current: Option<usize>) -> Option<usize> {
        let end = current.unwrap_or(self.bubbles.len());
        (0..end)
            .rev()
            .find(|idx| is_replyable_bubble(&self.bubbles[*idx]))
    }

    pub fn newer_replyable_index(&self, current: usize) -> Option<usize> {
        ((current + 1)..self.bubbles.len()).find(|idx| is_replyable_bubble(&self.bubbles[*idx]))
    }

    pub fn reply_target_at(
        &self,
        index: usize,
        display_for: &dyn Fn(&str) -> String,
    ) -> Option<(String, String)> {
        let bubble = self.bubbles.get(index)?;
        reply_target_label(bubble, display_for)
    }

    pub fn actor_for_event(&self, event_id: &str) -> Option<&str> {
        self.bubbles
            .iter()
            .find(|b| b.trailing_event_id.as_deref() == Some(event_id))
            .map(|b| b.actor_id.as_str())
    }

    pub fn render_lines(
        &self,
        width: u16,
        selected_bubble_idx: Option<usize>,
        display_for: &dyn Fn(&str) -> String,
    ) -> RenderedHistory {
        let mut out = Vec::new();
        let mut total_rows = 0usize;
        let mut selected_row_range = None;

        for (idx, b) in self.bubbles.iter().enumerate() {
            let ts = b.ts.with_timezone(&Local).format("%H:%M:%S").to_string();
            let actor = display_for(&b.actor_id);
            let (color, prefix) = match b.kind {
                BubbleKind::Stream => (Color::Cyan, ""),
                BubbleKind::Static => (Color::Yellow, "· "),
                BubbleKind::System => (Color::DarkGray, "· "),
            };
            let selected = selected_bubble_idx == Some(idx);
            let gutter = selection_gutter(selected);
            let header = vec![
                gutter.clone(),
                Span::styled(format!("[{}] ", ts), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    actor,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(prefix.to_string()),
            ];
            let reply_prefix = b
                .reply_to_event_id
                .as_ref()
                .map(|id| format!("↩ {}  ", short_id(id)))
                .unwrap_or_default();
            let mut first = true;
            let start_row = total_rows;
            for line in display_text(&b.text).split('\n') {
                let rendered = if first {
                    let mut spans = header.clone();
                    spans.push(Span::styled(
                        reply_prefix.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                    spans.push(Span::raw(line.to_string()));
                    if let Some(span) = delivery_span(b.delivery) {
                        spans.push(Span::raw("  "));
                        spans.push(span);
                    }
                    first = false;
                    Line::from(spans)
                } else {
                    Line::from(vec![
                        gutter.clone(),
                        Span::raw(format!("            {}", line)),
                    ])
                };
                total_rows = total_rows.saturating_add(wrapped_rows(&rendered, width));
                out.push(rendered);
            }
            if selected && total_rows > start_row {
                selected_row_range = Some((
                    start_row.min(u16::MAX as usize) as u16,
                    total_rows.saturating_sub(1).min(u16::MAX as usize) as u16,
                ));
            }
        }
        RenderedHistory {
            lines: out,
            total_rows: total_rows.min(u16::MAX as usize) as u16,
            selected_row_range,
        }
    }
}

fn display_text(text: &str) -> &str {
    // ACP markdown chunks often begin with blank lines. Keep the stored event
    // payload intact, but collapse those leading breaks in the TUI so a newly
    // arrived reply does not render as "header + empty space".
    text.trim_start_matches(|c| c == '\n' || c == '\r')
}

fn preview_text(text: &str) -> String {
    let line = display_text(text)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let squashed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.is_empty() {
        "(empty)".into()
    } else if squashed.chars().count() > 48 {
        format!("{}…", squashed.chars().take(48).collect::<String>())
    } else {
        squashed
    }
}

fn is_replyable_bubble(bubble: &Bubble) -> bool {
    bubble.kind != BubbleKind::System && bubble.trailing_event_id.is_some()
}

fn reply_target_label(
    bubble: &Bubble,
    display_for: &dyn Fn(&str) -> String,
) -> Option<(String, String)> {
    if !is_replyable_bubble(bubble) {
        return None;
    }
    let event_id = bubble.trailing_event_id.as_ref()?.clone();
    let actor = display_for(&bubble.actor_id);
    let preview = preview_text(&bubble.text);
    Some((
        event_id.clone(),
        format!("{}  {}  {}", short_id(&event_id), actor, preview),
    ))
}

fn reply_target(ev: &Event) -> Option<String> {
    ev.relations
        .iter()
        .find(|r| matches!(r.kind, RelationKind::RepliesTo))
        .map(|r| r.target.id.clone())
}

fn delivery_span(state: DeliveryState) -> Option<Span<'static>> {
    match state {
        DeliveryState::NotApplicable => None,
        DeliveryState::Pending => Some(Span::styled("⏳", Style::default().fg(Color::DarkGray))),
        DeliveryState::Delivered => Some(Span::styled("✓", Style::default().fg(Color::Green))),
    }
}

fn selection_gutter(selected: bool) -> Span<'static> {
    if selected {
        Span::styled("> ", Style::default().fg(Color::Magenta))
    } else {
        Span::raw("  ")
    }
}

fn wrapped_rows(line: &Line<'_>, width: u16) -> usize {
    if width == 0 {
        return 1;
    }
    Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width)
        .max(1)
}

fn format_action_request(ev: &Event) -> String {
    let title = ev
        .payload
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(action)");
    let mut s = format!("⌗ action.request {} — {}", short_id(&ev.id), title);
    if let Some(arr) = ev.payload.get("choices").and_then(|v| v.as_array()) {
        for choice in arr {
            let cid = choice.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let label = choice.get("label").and_then(|v| v.as_str()).unwrap_or("");
            s.push_str(&format!("\n  - {}: {}", cid, label));
        }
    }
    s
}

fn format_action_response(ev: &Event) -> String {
    let opt = ev
        .payload
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    format!("✓ action.response → {}", opt)
}

fn hands_off_target(ev: &Event) -> Option<String> {
    ev.relations
        .iter()
        .find(|r| matches!(r.kind, RelationKind::HandsOffTo))
        .map(|r| r.target.id.clone())
}

fn format_handoff(ev: &Event) -> String {
    // Body lives in `text` for content.add+HandsOffTo; legacy `handoff.offer`
    // events used `message` — keep the fallback so old journals render.
    let msg = ev
        .payload
        .get("text")
        .and_then(|v| v.as_str())
        .or_else(|| ev.payload.get("message").and_then(|v| v.as_str()))
        .unwrap_or("");
    let to = hands_off_target(ev).unwrap_or_default();
    format!("↪ handoff → {}: {}", to, msg)
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        display_text, preview_text, reply_target, Bubble, BubbleKind, DeliveryState, History,
    };
    use chrono::Utc;
    use proto::types::{Event, Ref, RefKind, Relation, RelationKind, ScopeKind, ScopeRef};
    use ratatui::widgets::{Paragraph, Wrap};
    use serde_json::json;

    #[test]
    fn display_text_drops_leading_linebreaks() {
        assert_eq!(display_text("\n\nhello"), "hello");
        assert_eq!(display_text("\r\n\nhello"), "hello");
        assert_eq!(display_text("hello"), "hello");
    }

    #[test]
    fn preview_text_uses_first_non_empty_line() {
        assert_eq!(preview_text("\n\nhello world"), "hello world");
        assert_eq!(preview_text("\n\n"), "(empty)");
    }

    #[test]
    fn reply_target_extracts_replies_to_relation() {
        let event = Event {
            id: "evt_2".into(),
            kind: "content.add".into(),
            actor_id: "actor_a".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_1".into(),
            },
            turn_id: Some("turn_1".into()),
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "hello" }),
            relations: vec![Relation {
                kind: RelationKind::RepliesTo,
                target: Ref {
                    kind: RefKind::Event,
                    id: "evt_1".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };
        assert_eq!(reply_target(&event).as_deref(), Some("evt_1"));
    }

    #[test]
    fn actor_for_event_finds_bubble_owner() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_agent_opencode".into(),
            turn_id: None,
            kind: BubbleKind::Static,
            text: "hello".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        assert_eq!(
            history.actor_for_event("evt_1"),
            Some("actor_agent_opencode")
        );
        assert_eq!(history.actor_for_event("evt_missing"), None);
    }

    #[test]
    fn replyable_navigation_skips_system_bubbles() {
        let mut history = History::default();
        history.push_system("system");
        history.bubbles.push(Bubble {
            actor_id: "actor_a".into(),
            turn_id: None,
            kind: BubbleKind::Static,
            text: "first".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });
        history.push_system("system");
        history.bubbles.push(Bubble {
            actor_id: "actor_b".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "second".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_2".into()),
            delivery: DeliveryState::NotApplicable,
        });

        assert_eq!(history.newest_replyable_index(), Some(3));
        assert_eq!(history.older_replyable_index(None), Some(3));
        assert_eq!(history.older_replyable_index(Some(3)), Some(1));
        assert_eq!(history.newer_replyable_index(1), Some(3));
        assert_eq!(history.newer_replyable_index(3), None);
    }

    #[test]
    fn render_lines_marks_selected_bubble_range() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_a".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello\nworld".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let rendered = history.render_lines(80, Some(0), &|id| id.to_string());

        assert_eq!(rendered.lines.len(), 2);
        assert_eq!(rendered.selected_row_range, Some((0, 1)));
    }

    #[test]
    fn render_lines_total_rows_matches_paragraph_wrapping() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_a".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "this is a line that should wrap in a narrow history pane".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let rendered = history.render_lines(16, Some(0), &|id| id.to_string());
        let expected = Paragraph::new(rendered.lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(16) as u16;

        assert_eq!(rendered.total_rows, expected);
    }
}
