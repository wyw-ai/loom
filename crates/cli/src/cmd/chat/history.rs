use std::collections::HashSet;

use chrono::{DateTime, Local, Utc};
use proto::types::Message;
#[cfg(test)]
use proto::types::{Event, RelationKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::markdown;

#[derive(Debug, Clone)]
pub struct Bubble {
    pub actor_id: String,
    pub turn_id: Option<String>,
    pub kind: BubbleKind,
    pub text: String,
    pub ts: DateTime<Utc>,
    pub reply_to_source_id: Option<String>,
    pub trailing_source_id: Option<String>,
    pub delivery: DeliveryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BubbleKind {
    /// Chat message; subsequent same-actor messages can append in place.
    Stream,
    /// Non-chat control messages (action.response, etc.)
    Static,
    /// `action.request` messages targeting the human. Rendered with extra
    /// prominence so the operator notices that the agent is parked waiting
    /// for a decision; counted by `pending_action_requests` until an
    /// `action.response` matches the trailing source id.
    ActionRequest,
    /// system / informational lines (server hints, errors)
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    /// Not relevant (incoming messages, system lines).
    NotApplicable,
    /// Outgoing message; waiting for the server to echo it back through the stream.
    Pending,
    /// Outgoing message; server has echoed it.
    Delivered,
}

/// Latest pinned announcement for the current scope. Derived from
/// `announcement.set` / `announcement.clear` messages as they arrive.
#[derive(Debug, Clone)]
pub struct Announcement {
    /// Body text (markdown). Rendered by the announcement panel.
    pub text: String,
    /// Actor that posted/edited the announcement.
    pub actor_id: String,
    /// Server timestamp of the `announcement.set` message.
    pub ts: DateTime<Utc>,
}

#[derive(Default)]
pub struct History {
    pub bubbles: Vec<Bubble>,
    /// Latest announcement, or `None` if none has been set or the most
    /// recent message was an `announcement.clear`.
    pub current_announcement: Option<Announcement>,
    /// Source ids of `action.request` messages that have already been
    /// answered in this scope. Stored as a side set because the response and
    /// request live in different bubbles and their own source ids differ.
    acked_request_ids: HashSet<String>,
}

pub struct RenderedHistory {
    pub lines: Vec<Line<'static>>,
    pub total_rows: u16,
    pub selected_row_range: Option<(u16, u16)>,
}

impl History {
    #[cfg(test)]
    pub fn push_event(&mut self, ev: &Event) {
        match ev.kind.as_str() {
            // Chat content is carried by Message records. Ignore legacy
            // content events so the TUI does not mix old and new streams.
            "content.add" => {}
            "action.request" => self.push_action_request(ev, format_action_request(ev)),
            "action.response" => {
                if let Some(target) = responds_to_target(ev) {
                    self.acked_request_ids.insert(target);
                }
                self.push_static(ev, format_action_response(ev));
            }
            // Pinned announcement updates: render in the right-side panel
            // rather than as a chat bubble. We still want them to flow
            // through `push_event` so legacy journal replays
            // converge naturally on the latest value.
            "announcement.set" => self.apply_announcement_set(ev),
            "announcement.clear" => self.current_announcement = None,
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
            reply_to_source_id: None,
            trailing_source_id: None,
            delivery: DeliveryState::NotApplicable,
        });
    }

    pub fn push_message(&mut self, message: &Message) {
        for b in self.bubbles.iter_mut().rev() {
            if b.delivery == DeliveryState::Pending
                && b.trailing_source_id.as_deref() == Some(message.id.as_str())
            {
                b.delivery = DeliveryState::Delivered;
                return;
            }
        }
        match message
            .metadata
            .get("kind")
            .and_then(serde_json::Value::as_str)
        {
            Some("action.request") => {
                self.push_action_request_message(message, format_action_request_message(message));
                return;
            }
            Some("action.response") => {
                if let Some(target) = message.parent_message_id.as_ref() {
                    self.acked_request_ids.insert(target.clone());
                }
                self.push_static_message(message, format_action_response_message(message));
                return;
            }
            Some("announcement.set") => {
                self.apply_announcement_message(message);
                return;
            }
            Some("announcement.clear") => {
                self.current_announcement = None;
                return;
            }
            _ => {}
        }
        self.bubbles.push(Bubble {
            actor_id: message.author_actor_id.clone(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: message.body.clone(),
            ts: message.created_at,
            reply_to_source_id: message.parent_message_id.clone(),
            trailing_source_id: Some(message.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    /// Push our own outgoing message immediately on send, with the
    /// server-assigned message id so we can flip to Delivered when it echoes
    /// back through stream/update.
    pub fn push_outgoing(
        &mut self,
        actor_id: &str,
        message_id: &str,
        ts: DateTime<Utc>,
        text: String,
        parent_message_id: Option<String>,
    ) {
        self.bubbles.push(Bubble {
            actor_id: actor_id.to_string(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text,
            ts,
            reply_to_source_id: parent_message_id,
            trailing_source_id: Some(message_id.to_string()),
            delivery: DeliveryState::Pending,
        });
    }

    /// Reduce an `announcement.set` event into `current_announcement`.
    /// Empty `payload.text` is treated as a clear so callers can either
    /// emit `announcement.clear` or post `set` with `text: ""`.
    #[cfg(test)]
    fn apply_announcement_set(&mut self, ev: &Event) {
        let text = ev
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if text.trim().is_empty() {
            self.current_announcement = None;
            return;
        }
        self.current_announcement = Some(Announcement {
            text,
            actor_id: ev.actor_id.clone(),
            ts: ev.occurred_at,
        });
    }

    #[cfg(test)]
    fn push_static(&mut self, ev: &Event, text: String) {
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::Static,
            text,
            ts: ev.occurred_at,
            reply_to_source_id: reply_target(ev),
            trailing_source_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    #[cfg(test)]
    fn push_action_request(&mut self, ev: &Event, text: String) {
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::ActionRequest,
            text,
            ts: ev.occurred_at,
            reply_to_source_id: reply_target(ev),
            trailing_source_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    fn apply_announcement_message(&mut self, message: &Message) {
        let text = message
            .metadata
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or(message.body.as_str())
            .to_string();
        if text.trim().is_empty() {
            self.current_announcement = None;
            return;
        }
        self.current_announcement = Some(Announcement {
            text,
            actor_id: message.author_actor_id.clone(),
            ts: message.created_at,
        });
    }

    fn push_static_message(&mut self, message: &Message, text: String) {
        self.bubbles.push(Bubble {
            actor_id: message.author_actor_id.clone(),
            turn_id: None,
            kind: BubbleKind::Static,
            text,
            ts: message.created_at,
            reply_to_source_id: message.parent_message_id.clone(),
            trailing_source_id: Some(message.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    fn push_action_request_message(&mut self, message: &Message, text: String) {
        self.bubbles.push(Bubble {
            actor_id: message.author_actor_id.clone(),
            turn_id: message
                .metadata
                .get("runId")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            kind: BubbleKind::ActionRequest,
            text,
            ts: message.created_at,
            reply_to_source_id: message.parent_message_id.clone(),
            trailing_source_id: Some(message.id.clone()),
            delivery: DeliveryState::NotApplicable,
        });
    }

    pub fn pending_action_requests(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for b in &self.bubbles {
            if b.kind == BubbleKind::ActionRequest {
                if let Some(eid) = b.trailing_source_id.as_ref() {
                    if !self.acked_request_ids.contains(eid) {
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

    pub fn actor_for_source(&self, source_id: &str) -> Option<&str> {
        self.bubbles
            .iter()
            .find(|b| b.trailing_source_id.as_deref() == Some(source_id))
            .map(|b| b.actor_id.as_str())
    }

    pub fn bubble_is_collapsible(&self, index: usize) -> bool {
        self.bubbles
            .get(index)
            .map(|bubble| bubble_body_rows(bubble).len() > COLLAPSED_BODY_LINES)
            .unwrap_or(false)
    }

    pub fn render_lines(
        &self,
        width: u16,
        selected_bubble_idx: Option<usize>,
        expanded_bubble_indices: &HashSet<usize>,
        display_for: &dyn Fn(&str) -> String,
        kind_for: &dyn Fn(&str) -> Option<String>,
    ) -> RenderedHistory {
        let mut out = Vec::new();
        let mut total_rows = 0usize;
        let mut selected_row_range = None;

        for (idx, b) in self.bubbles.iter().enumerate() {
            // Visual breathing room between bubbles. The spacer is owned by
            // no bubble — it sits outside `start_row..total_rows`, so the
            // selected-range cursor never lands on it.
            if idx > 0 {
                out.push(Line::from(Span::raw("")));
                total_rows = total_rows.saturating_add(1);
            }
            let selected = selected_bubble_idx == Some(idx);
            let gutter = selection_gutter(selected);
            let start_row = total_rows;

            // System bubbles are informational chatter (welcome, errors,
            // dividers). Render as a single dim line with no header so they
            // don't clutter the stream.
            if b.kind == BubbleKind::System {
                let line = Line::from(vec![
                    gutter.clone(),
                    Span::styled(
                        format!("· {}", display_text(&b.text)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                total_rows = total_rows.saturating_add(wrapped_rows(&line, width));
                out.push(line);
                if selected && total_rows > start_row {
                    selected_row_range = Some((
                        start_row.min(u16::MAX as usize) as u16,
                        total_rows.saturating_sub(1).min(u16::MAX as usize) as u16,
                    ));
                }
                continue;
            }

            // Reply quote line above the bubble (parent preview). Always on
            // its own row — wrapping is handled by `Paragraph::wrap`.
            if let Some(p) = b
                .reply_to_source_id
                .as_deref()
                .map(|pid| lookup_parent(pid, &self.bubbles, display_for))
            {
                let line = build_quote_line(&gutter, &p);
                total_rows = total_rows.saturating_add(wrapped_rows(&line, width));
                out.push(line);
            }

            // Header: `[name] [kind] yyyy/m/d HH:MM   [delivery?]`
            let actor_color = match b.kind {
                BubbleKind::Stream => Color::Cyan,
                BubbleKind::Static => Color::Yellow,
                BubbleKind::ActionRequest => Color::Magenta,
                BubbleKind::System => Color::DarkGray, // unreachable — handled above
            };
            let actor_style = if selected {
                Style::default()
                    .fg(actor_color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
                    .fg(actor_color)
                    .add_modifier(Modifier::BOLD)
            };
            let mut header = vec![
                gutter.clone(),
                Span::styled(format!("[{}]", display_for(&b.actor_id)), actor_style),
            ];
            if let Some(k) = kind_for(&b.actor_id) {
                if !k.is_empty() {
                    header.push(Span::raw(" "));
                    header.push(Span::styled(
                        format!("[{}]", k),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            header.push(Span::raw(" "));
            header.push(Span::styled(
                b.ts.with_timezone(&Local)
                    .format("%Y/%-m/%-d %H:%M")
                    .to_string(),
                Style::default().fg(Color::DarkGray),
            ));
            if let Some(span) = delivery_span(b.delivery) {
                header.push(Span::raw("  "));
                header.push(span);
            }
            let header_line = Line::from(header);
            total_rows = total_rows.saturating_add(wrapped_rows(&header_line, width));
            out.push(header_line);

            // Body. Stream bubbles go through the markdown renderer; control
            // bubbles render their pre-formatted text verbatim.
            let body_rows: Vec<Vec<Span<'static>>> = bubble_body_rows(b);
            let body_row_count = body_rows.len();
            let collapsed =
                body_row_count > COLLAPSED_BODY_LINES && !expanded_bubble_indices.contains(&idx);
            let visible_body_rows = if collapsed {
                COLLAPSED_BODY_LINES
            } else {
                body_row_count
            };
            for row_spans in body_rows.into_iter().take(visible_body_rows) {
                let mut spans = vec![gutter.clone()];
                spans.extend(row_spans);
                let line = Line::from(spans);
                total_rows = total_rows.saturating_add(wrapped_rows(&line, width));
                out.push(line);
            }
            if collapsed {
                let remaining = body_row_count.saturating_sub(COLLAPSED_BODY_LINES);
                let hint = Line::from(vec![
                    gutter.clone(),
                    Span::styled(
                        format!("… {remaining} more lines (→ expand)"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                total_rows = total_rows.saturating_add(wrapped_rows(&hint, width));
                out.push(hint);
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

/// Maximum body rows shown before a bubble collapses to a `… N more lines`
/// hint. Toggled per-bubble via the right-arrow / `expanded_history` set.
const COLLAPSED_BODY_LINES: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParentPreview {
    actor: String,
    text: String,
    available: bool,
}

fn lookup_parent(
    parent_id: &str,
    bubbles: &[Bubble],
    display_for: &dyn Fn(&str) -> String,
) -> ParentPreview {
    bubbles
        .iter()
        .find(|b| b.trailing_source_id.as_deref() == Some(parent_id))
        .map(|b| ParentPreview {
            actor: display_for(&b.actor_id),
            text: preview_text(&b.text),
            available: true,
        })
        .unwrap_or_else(|| ParentPreview {
            actor: String::new(),
            text: String::new(),
            available: false,
        })
}

fn build_quote_line(gutter: &Span<'static>, p: &ParentPreview) -> Line<'static> {
    let label = if p.available {
        format!("↩ @{}: {}", p.actor, p.text)
    } else {
        "↩ (message unavailable)".into()
    };
    Line::from(vec![
        gutter.clone(),
        Span::styled(label, Style::default().fg(Color::DarkGray)),
    ])
}

fn display_text(text: &str) -> &str {
    // ACP markdown chunks often begin with blank lines. Keep the stored event
    // payload intact, but collapse those leading breaks in the TUI so a newly
    // arrived reply does not render as "header + empty space".
    text.trim_start_matches(|c| c == '\n' || c == '\r')
}

fn bubble_body_rows(bubble: &Bubble) -> Vec<Vec<Span<'static>>> {
    let rows = match bubble.kind {
        BubbleKind::Stream => {
            markdown::render_to_rows(display_text(&bubble.text), Style::default())
        }
        BubbleKind::ActionRequest => {
            let style = Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD);
            display_text(&bubble.text)
                .split('\n')
                .map(|line| vec![Span::styled(line.to_string(), style)])
                .collect()
        }
        _ => display_text(&bubble.text)
            .split('\n')
            .map(|line| vec![Span::raw(line.to_string())])
            .collect(),
    };
    trim_trailing_blank_rows(rows)
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
    bubble.kind != BubbleKind::System && bubble.trailing_source_id.is_some()
}

fn reply_target_label(
    bubble: &Bubble,
    display_for: &dyn Fn(&str) -> String,
) -> Option<(String, String)> {
    if !is_replyable_bubble(bubble) {
        return None;
    }
    let source_id = bubble.trailing_source_id.as_ref()?.clone();
    let actor = display_for(&bubble.actor_id);
    let preview = preview_text(&bubble.text);
    Some((source_id, format!("@{}: {}", actor, preview)))
}

#[cfg(test)]
fn reply_target(ev: &Event) -> Option<String> {
    ev.relations
        .iter()
        .find(|r| matches!(r.kind, RelationKind::RepliesTo))
        .map(|r| r.target.id.clone())
}

/// Source id this legacy test event responds to, e.g. an `action.response`
/// pointing back at the `action.request` it answers. Used to retire the
/// request from the pending set in `pending_action_requests`.
#[cfg(test)]
fn responds_to_target(ev: &Event) -> Option<String> {
    ev.relations
        .iter()
        .find(|r| matches!(r.kind, RelationKind::RespondsTo))
        .map(|r| r.target.id.clone())
}

fn delivery_span(state: DeliveryState) -> Option<Span<'static>> {
    match state {
        DeliveryState::NotApplicable => None,
        DeliveryState::Pending => Some(Span::styled("⏳", Style::default().fg(Color::DarkGray))),
        DeliveryState::Delivered => Some(Span::styled("✓", Style::default().fg(Color::Green))),
    }
}

/// Selected bubbles get a high-contrast magenta block as the gutter so the
/// chevron shows up clearly even on light or low-contrast terminal themes.
/// Width matches the unselected `"  "` gutter so wrap math stays identical.
fn selection_gutter(selected: bool) -> Span<'static> {
    if selected {
        Span::styled(
            "▌ ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("  ")
    }
}

/// Drop trailing rows whose spans are all empty so a markdown body ending in
/// `\n\n` doesn't pad the bubble with phantom rows.
fn trim_trailing_blank_rows(mut rows: Vec<Vec<Span<'static>>>) -> Vec<Vec<Span<'static>>> {
    while rows
        .last()
        .map(|r| r.iter().all(|s| s.content.as_ref().is_empty()))
        .unwrap_or(false)
    {
        rows.pop();
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
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

#[cfg(test)]
fn format_action_request(ev: &Event) -> String {
    let title = ev
        .payload
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(action)");
    let mut s = format!("⚠ action.request {} — {}", short_id(&ev.id), title);
    if let Some(arr) = ev.payload.get("choices").and_then(|v| v.as_array()) {
        for choice in arr {
            let cid = choice.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let label = choice.get("label").and_then(|v| v.as_str()).unwrap_or("");
            s.push_str(&format!("\n  - {}: {}", cid, label));
        }
    }
    s
}

fn format_action_request_message(message: &Message) -> String {
    let title = message
        .metadata
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(action)");
    let mut s = format!("⚠ action.request {} — {}", short_id(&message.id), title);
    if let Some(arr) = message.metadata.get("choices").and_then(|v| v.as_array()) {
        for choice in arr {
            let cid = choice.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let label = choice.get("label").and_then(|v| v.as_str()).unwrap_or("");
            s.push_str(&format!("\n  - {}: {}", cid, label));
        }
    }
    s
}

#[cfg(test)]
fn format_action_response(ev: &Event) -> String {
    let opt = ev
        .payload
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    format!("✓ action.response → {}", opt)
}

fn format_action_response_message(message: &Message) -> String {
    let opt = message
        .metadata
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    format!("✓ action.response → {}", opt)
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
    use std::collections::HashSet;

    use super::{
        display_text, preview_text, reply_target, reply_target_label, Bubble, BubbleKind,
        DeliveryState, History,
    };
    use chrono::Utc;
    use proto::types::{
        DeliveryPolicy as MessageDeliveryPolicy, Event, Message, MessageIntent, MessageKind, Ref,
        RefKind, Relation, RelationKind, ScopeKind, ScopeRef,
    };
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
    fn pending_action_requests_after_variant_switch_finds_unacknowledged_bubble() {
        let mut history = History::default();
        let req = Event {
            id: "evt_action_1".into(),
            kind: "action.request".into(),
            actor_id: "actor_agent_opencode".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            turn_id: Some("turn_1".into()),
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({
                "title": "Approve write to /etc/passwd?",
                "choices": [
                    { "id": "allow", "label": "Allow once" },
                    { "id": "deny", "label": "Deny" },
                ],
            }),
            relations: vec![Relation {
                kind: RelationKind::DirectedTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_human_current".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };
        history.push_event(&req);

        let pending = history.pending_action_requests();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, "evt_action_1");
        assert_eq!(
            history.bubbles.last().map(|b| b.kind.clone()),
            Some(BubbleKind::ActionRequest)
        );

        // An action.response carrying the same trailing source id (pushed via
        // push_static through the normal `action.response` path) clears it.
        let resp = Event {
            id: "evt_action_1".into(),
            kind: "action.response".into(),
            actor_id: "actor_human_current".into(),
            scope: req.scope.clone(),
            turn_id: None,
            seq: 2,
            occurred_at: Utc::now(),
            payload: json!({ "optionId": "allow", "kind": "accepted" }),
            relations: vec![Relation {
                kind: RelationKind::RespondsTo,
                target: Ref {
                    kind: RefKind::Event,
                    id: "evt_action_1".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };
        history.push_event(&resp);
        assert!(history.pending_action_requests().is_empty());
    }

    #[test]
    fn actor_for_source_finds_bubble_owner() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_agent_opencode".into(),
            turn_id: None,
            kind: BubbleKind::Static,
            text: "hello".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        assert_eq!(
            history.actor_for_source("evt_1"),
            Some("actor_agent_opencode")
        );
        assert_eq!(history.actor_for_source("evt_missing"), None);
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
            reply_to_source_id: None,
            trailing_source_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });
        history.push_system("system");
        history.bubbles.push(Bubble {
            actor_id: "actor_b".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "second".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_2".into()),
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
            reply_to_source_id: None,
            trailing_source_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let rendered =
            history.render_lines(80, Some(0), &HashSet::new(), &|id| id.to_string(), &|_| {
                None
            });

        // Header row + 2 body rows ("hello", "world").
        assert_eq!(rendered.lines.len(), 3);
        assert_eq!(rendered.selected_row_range, Some((0, 2)));
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
            reply_to_source_id: None,
            trailing_source_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let rendered =
            history.render_lines(16, Some(0), &HashSet::new(), &|id| id.to_string(), &|_| {
                None
            });
        let expected = Paragraph::new(rendered.lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(16) as u16;

        assert_eq!(rendered.total_rows, expected);
    }

    fn line_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn parent_and_reply() -> History {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "bojun.cbj".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "the original message that someone is going to reply to".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_parent".into()),
            delivery: DeliveryState::NotApplicable,
        });
        history.bubbles.push(Bubble {
            actor_id: "Coder".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "这个任务已经完成了".into(),
            ts: Utc::now(),
            reply_to_source_id: Some("evt_parent".into()),
            trailing_source_id: Some("evt_reply".into()),
            delivery: DeliveryState::NotApplicable,
        });
        history
    }

    #[test]
    fn reply_target_label_omits_source_id() {
        let bubble = Bubble {
            actor_id: "bojun.cbj".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello world".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_abc123def456".into()),
            delivery: DeliveryState::NotApplicable,
        };
        let (id, label) = reply_target_label(&bubble, &|id| id.to_string()).unwrap();
        assert_eq!(id, "evt_abc123def456");
        assert_eq!(label, "@bojun.cbj: hello world");
        assert!(!label.contains("evt_"));
    }

    #[test]
    fn render_lines_emits_quote_line_above_reply() {
        let history = parent_and_reply();
        let rendered =
            history.render_lines(80, None, &HashSet::new(), &|id| id.to_string(), &|_| None);
        // parent (header + body) + spacer + quote + reply (header + body) = 6 rows.
        assert_eq!(rendered.lines.len(), 6);
        let quote = line_text(&rendered.lines[3]);
        assert!(quote.contains("↩ @bojun.cbj:"), "got: {quote:?}");
        assert!(quote.contains("the original"), "got: {quote:?}");
        assert!(!quote.contains("evt_"), "quote leaked source id: {quote:?}");
        let reply_header = line_text(&rendered.lines[4]);
        assert!(reply_header.contains("[Coder]"), "got: {reply_header:?}");
        let reply_body = line_text(&rendered.lines[5]);
        assert!(
            reply_body.contains("这个任务已经完成了"),
            "got: {reply_body:?}"
        );
        assert!(
            !reply_body.contains("↩"),
            "reply leaked inline arrow: {reply_body:?}"
        );
        // Spacer row (index 2) is empty, owned by no bubble.
        assert_eq!(line_text(&rendered.lines[2]), "");
    }

    #[test]
    fn render_lines_quote_falls_back_when_parent_missing() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "Coder".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "responding to someone".into(),
            ts: Utc::now(),
            reply_to_source_id: Some("evt_long_gone".into()),
            trailing_source_id: Some("evt_reply".into()),
            delivery: DeliveryState::NotApplicable,
        });
        let rendered =
            history.render_lines(80, None, &HashSet::new(), &|id| id.to_string(), &|_| None);
        // quote + header + body = 3 rows.
        assert_eq!(rendered.lines.len(), 3);
        let quote = line_text(&rendered.lines[0]);
        assert!(quote.contains("(message unavailable)"), "got: {quote:?}");
        assert!(!quote.contains("evt_"));
    }

    #[test]
    fn render_lines_selected_range_includes_quote_line() {
        let history = parent_and_reply();
        // Layout: parent header (0), parent body (1), spacer (2), quote (3),
        // reply header (4), reply body (5). Selecting the reply (index 1)
        // covers the quote + header + body but NOT the spacer.
        let rendered =
            history.render_lines(80, Some(1), &HashSet::new(), &|id| id.to_string(), &|_| {
                None
            });
        assert_eq!(rendered.selected_row_range, Some((3, 5)));
    }

    #[test]
    fn render_lines_header_uses_new_layout_with_kind_label() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_human_self".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_x".into()),
            delivery: DeliveryState::NotApplicable,
        });
        let rendered = history.render_lines(
            80,
            None,
            &HashSet::new(),
            &|id| {
                if id == "actor_human_self" {
                    "bojun.cbj".to_string()
                } else {
                    id.to_string()
                }
            },
            &|id| {
                if id == "actor_human_self" {
                    Some("human".to_string())
                } else {
                    None
                }
            },
        );
        assert_eq!(rendered.lines.len(), 2);
        let header = line_text(&rendered.lines[0]);
        assert!(header.contains("[bojun.cbj]"), "got: {header:?}");
        assert!(header.contains("[human]"), "got: {header:?}");
        // Header has no `[HH:MM:SS]` prefix anymore.
        assert!(!header.contains("[HH"), "got: {header:?}");
    }

    #[test]
    fn push_message_renders_chat_message() {
        let mut history = History::default();
        let message = make_message("msg_1", "actor_human_self", "please look", None);
        history.push_message(&message);

        let display_for = |id: &str| match id {
            "actor_human_self" => "bojun.cbj".to_string(),
            other => other.to_string(),
        };
        let rendered = history.render_lines(120, None, &HashSet::new(), &display_for, &|_| None);
        // header + body
        assert_eq!(rendered.lines.len(), 2);
        let body = line_text(&rendered.lines[1]);
        assert!(body.contains("please look"), "got: {body:?}");
    }

    #[test]
    fn long_messages_are_collapsed_by_default() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_a".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "1\n2\n3\n4\n5\n6\n7".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_long".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let rendered =
            history.render_lines(80, Some(0), &HashSet::new(), &|id| id.to_string(), &|_| {
                None
            });

        assert!(history.bubble_is_collapsible(0));
        // Header (1) + 5 visible body rows + hint (1) = 7.
        assert_eq!(rendered.lines.len(), 7);
        let hint = line_text(rendered.lines.last().unwrap());
        assert!(hint.contains("2 more lines"));
    }

    #[test]
    fn expanded_long_messages_render_full_body() {
        let mut history = History::default();
        history.bubbles.push(Bubble {
            actor_id: "actor_a".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "1\n2\n3\n4\n5\n6\n7".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_long".into()),
            delivery: DeliveryState::NotApplicable,
        });

        let mut expanded = HashSet::new();
        expanded.insert(0);
        let rendered =
            history.render_lines(80, Some(0), &expanded, &|id| id.to_string(), &|_| None);

        // Header (1) + 7 body rows = 8.
        assert_eq!(rendered.lines.len(), 8);
        let last = line_text(rendered.lines.last().unwrap());
        assert!(last.contains('7'));
    }

    fn make_message(
        id: &str,
        actor: &str,
        body: &str,
        parent_message_id: Option<String>,
    ) -> Message {
        Message {
            id: id.into(),
            target: "#chan_demo:msg_root".into(),
            author_actor_id: actor.into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            created_at: Utc::now(),
            kind: MessageKind::Human,
            body: body.into(),
            mentions: Vec::new(),
            audience: Vec::new(),
            intent: MessageIntent::Chat,
            delivery_policy: MessageDeliveryPolicy::NotifyOnly,
            parent_message_id,
            thread_root_message_id: Some("msg_root".into()),
            task_id: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn content_events_are_ignored_by_chat_history() {
        let mut history = History::default();
        let ev = Event {
            id: "evt_old_content".into(),
            kind: "content.add".into(),
            actor_id: "actor_human".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "old event body" }),
            relations: Vec::new(),
            _meta: None,
        };
        history.push_event(&ev);
        assert!(history.bubbles.is_empty());
    }

    #[test]
    fn echoed_message_finalizes_pending_outgoing_bubble() {
        let mut history = History::default();
        history.push_outgoing(
            "actor_human",
            "msg_reply",
            Utc::now(),
            "got it".into(),
            Some("msg_parent".into()),
        );
        assert_eq!(history.bubbles[0].delivery, DeliveryState::Pending);

        let message = make_message(
            "msg_reply",
            "actor_human",
            "got it",
            Some("msg_parent".into()),
        );
        history.push_message(&message);

        // Still a single bubble; the echoed message flips the optimistic row.
        assert_eq!(history.bubbles.len(), 1);
        assert_eq!(history.bubbles[0].delivery, DeliveryState::Delivered);
        assert_eq!(history.bubbles[0].text, "got it");
    }

    fn announcement_event(id: &str, actor: &str, text: &str, kind: &str) -> Event {
        Event {
            id: id.into(),
            kind: kind.into(),
            actor_id: actor.into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": text }),
            relations: vec![],
            _meta: None,
        }
    }

    #[test]
    fn announcement_set_updates_current_announcement_without_pushing_bubble() {
        let mut history = History::default();
        let ev = announcement_event("evt_a1", "actor_human", "release notes", "announcement.set");
        history.push_event(&ev);

        assert!(
            history.bubbles.is_empty(),
            "announcement must not pollute bubbles"
        );
        let a = history
            .current_announcement
            .as_ref()
            .expect("announcement set");
        assert_eq!(a.text, "release notes");
        assert_eq!(a.actor_id, "actor_human");
    }

    #[test]
    fn announcement_set_replaces_previous() {
        let mut history = History::default();
        history.push_event(&announcement_event(
            "evt_a1",
            "a",
            "first",
            "announcement.set",
        ));
        history.push_event(&announcement_event(
            "evt_a2",
            "b",
            "second",
            "announcement.set",
        ));
        let a = history
            .current_announcement
            .as_ref()
            .expect("announcement set");
        assert_eq!(a.text, "second");
        assert_eq!(a.actor_id, "b");
    }

    #[test]
    fn announcement_clear_drops_current() {
        let mut history = History::default();
        history.push_event(&announcement_event(
            "evt_a1",
            "a",
            "first",
            "announcement.set",
        ));
        history.push_event(&announcement_event("evt_a2", "a", "", "announcement.clear"));
        assert!(history.current_announcement.is_none());
        assert!(history.bubbles.is_empty());
    }

    #[test]
    fn announcement_set_with_blank_text_clears() {
        // Empty `text` on a `.set` is treated as a clear so callers can pick
        // either spelling without us caring which.
        let mut history = History::default();
        history.push_event(&announcement_event(
            "evt_a1",
            "a",
            "first",
            "announcement.set",
        ));
        history.push_event(&announcement_event(
            "evt_a2",
            "a",
            "   ",
            "announcement.set",
        ));
        assert!(history.current_announcement.is_none());
    }
}
