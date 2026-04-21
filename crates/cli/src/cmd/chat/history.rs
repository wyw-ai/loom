use chrono::{DateTime, Local, Utc};
use proto::types::{Event, RelationKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

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
            .filter_map(|b| {
                if b.kind == BubbleKind::System {
                    return None;
                }
                let event_id = b.trailing_event_id.as_ref()?.clone();
                let actor = display_for(&b.actor_id);
                let preview = preview_text(&b.text);
                Some((
                    event_id.clone(),
                    format!("{}  {}  {}", short_id(&event_id), actor, preview),
                ))
            })
            .collect()
    }

    pub fn actor_for_event(&self, event_id: &str) -> Option<&str> {
        self.bubbles
            .iter()
            .find(|b| b.trailing_event_id.as_deref() == Some(event_id))
            .map(|b| b.actor_id.as_str())
    }

    pub fn render_lines(&self, display_for: &dyn Fn(&str) -> String) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for b in &self.bubbles {
            let ts = b.ts.with_timezone(&Local).format("%H:%M:%S").to_string();
            let actor = display_for(&b.actor_id);
            let (color, prefix) = match b.kind {
                BubbleKind::Stream => (Color::Cyan, ""),
                BubbleKind::Static => (Color::Yellow, "· "),
                BubbleKind::System => (Color::DarkGray, "· "),
            };
            let header = vec![
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
            for line in display_text(&b.text).split('\n') {
                if first {
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
                    out.push(Line::from(spans));
                    first = false;
                } else {
                    out.push(Line::from(Span::raw(format!("            {}", line))));
                }
            }
        }
        out
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
}
