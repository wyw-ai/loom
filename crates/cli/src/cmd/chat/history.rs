use std::collections::HashSet;

use chrono::{DateTime, Local, Utc};
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
    pub reply_to_event_id: Option<String>,
    pub trailing_event_id: Option<String>,
    pub delivery: DeliveryState,
    /// True while this bubble is being filled by `turn/stream.update`
    /// notifications. Flipped to `false` when the canonical `content.add`
    /// event for the same turn arrives. Renders a live cursor while true.
    pub streaming: bool,
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
            // turn.close is normally suppressed (delivery state on the
            // outgoing bubble already conveys "the server saw it" and the
            // closing chunk arrives as a normal content.add event).
            // Cancelled turns are the exception — finalize any still-streaming
            // bubble and surface a system divider so other channel members
            // see who pulled the plug.
            "turn.close" => {
                let is_cancelled = ev
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("cancelled"))
                    .unwrap_or(false);
                if is_cancelled {
                    if let Some(turn) = ev.turn_id.as_deref() {
                        for b in self.bubbles.iter_mut().rev() {
                            if b.streaming
                                && b.kind == BubbleKind::Stream
                                && b.actor_id == ev.actor_id
                                && b.turn_id.as_deref() == Some(turn)
                            {
                                b.streaming = false;
                                break;
                            }
                        }
                    }
                    let by = ev
                        .payload
                        .get("_meta")
                        .and_then(|m| m.get("cancelledBy"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    self.push_system(format!("— turn cancelled by @{} —", by));
                }
            }
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
            streaming: false,
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
            streaming: false,
        });
    }

    fn append_stream(&mut self, ev: &Event) {
        let text = ev
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // If we have a streaming bubble for this turn (built up live from
        // turn/stream.update notifications), finalize it: replace its text
        // with the canonical event text (authoritative — handles dropped
        // deltas) and stamp the event id. We scan from the back since
        // streaming bubbles are usually the most recent ones.
        if let Some(turn) = ev.turn_id.as_deref() {
            for b in self.bubbles.iter_mut().rev() {
                if b.streaming
                    && b.kind == BubbleKind::Stream
                    && b.actor_id == ev.actor_id
                    && b.turn_id.as_deref() == Some(turn)
                {
                    b.text = text;
                    b.streaming = false;
                    b.trailing_event_id = Some(ev.id.clone());
                    b.ts = ev.occurred_at;
                    if b.reply_to_event_id.is_none() {
                        b.reply_to_event_id = reply_target(ev);
                    }
                    return;
                }
            }
        }
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
            streaming: false,
        });
    }

    /// Append a partial-text chunk from a `turn/stream.update` notification
    /// into the streaming bubble for `(actor_id, turn_id)`, creating the
    /// bubble on first delta. The bubble flips to non-streaming when the
    /// canonical `content.add` event for this turn arrives via
    /// `append_stream`.
    pub fn append_stream_delta(
        &mut self,
        actor_id: &str,
        turn_id: &str,
        delta: &str,
        ts: DateTime<Utc>,
    ) {
        for b in self.bubbles.iter_mut().rev() {
            if b.streaming
                && b.kind == BubbleKind::Stream
                && b.actor_id == actor_id
                && b.turn_id.as_deref() == Some(turn_id)
            {
                b.text.push_str(delta);
                b.ts = ts;
                return;
            }
        }
        self.bubbles.push(Bubble {
            actor_id: actor_id.to_string(),
            turn_id: Some(turn_id.to_string()),
            kind: BubbleKind::Stream,
            text: delta.to_string(),
            ts,
            reply_to_event_id: None,
            trailing_event_id: None,
            delivery: DeliveryState::NotApplicable,
            streaming: true,
        });
    }

    /// Iterator over `(actor_id, turn_id, started_at)` for every streaming
    /// bubble currently in history. Used by the chat TUI to render an
    /// in-flight status bar above the input area.
    pub fn streaming_turns(&self) -> Vec<(String, String, DateTime<Utc>)> {
        self.bubbles
            .iter()
            .filter(|b| b.streaming)
            .filter_map(|b| {
                let turn_id = b.turn_id.clone()?;
                Some((b.actor_id.clone(), turn_id, b.ts))
            })
            .collect()
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
            streaming: false,
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
            let actor_style = if selected {
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            };
            let header = vec![
                gutter.clone(),
                Span::styled(format!("[{}] ", ts), Style::default().fg(Color::DarkGray)),
                Span::styled(actor, actor_style),
                Span::raw("  "),
                Span::raw(prefix.to_string()),
            ];

            // Reply rendering: when wide enough, hang a dim single-line quote
            // block above the bubble; on narrow terminals, fall back to an
            // inline `↩ @actor` prefix on the body. Either way the parent
            // event id is never shown.
            let parent = b
                .reply_to_event_id
                .as_deref()
                .map(|pid| lookup_parent(pid, &self.bubbles, display_for));
            let start_row = total_rows;

            let inline_quote_prefix = match parent.as_ref() {
                Some(p) if width >= QUOTE_LINE_MIN_WIDTH => {
                    let line = build_quote_line(&gutter, p);
                    total_rows = total_rows.saturating_add(wrapped_rows(&line, width));
                    out.push(line);
                    String::new()
                }
                Some(p) => format!("{}  ", inline_quote_label(p)),
                None => String::new(),
            };

            // Markdown-render the bubble body into rows of styled spans. System
            // / static bubbles bypass markdown — they're already pre-formatted
            // strings (handoff arrows, action.request prefixes, etc.) and we
            // don't want `*` / `#` in those treated as syntax.
            let body_rows = bubble_body_rows(b);
            let body_row_count = body_rows.len();
            let collapsed =
                body_row_count > COLLAPSED_BODY_LINES && !expanded_bubble_indices.contains(&idx);
            let visible_body_rows = if collapsed {
                COLLAPSED_BODY_LINES
            } else {
                body_row_count
            };
            let last_idx = visible_body_rows.saturating_sub(1);
            for (line_idx, row_spans) in body_rows.into_iter().take(visible_body_rows).enumerate() {
                let is_last = line_idx == last_idx;
                let rendered = if line_idx == 0 {
                    let mut spans = header.clone();
                    if !inline_quote_prefix.is_empty() {
                        spans.push(Span::styled(
                            inline_quote_prefix.clone(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    spans.extend(row_spans);
                    if b.streaming && is_last {
                        spans.push(Span::styled("▍", Style::default().fg(Color::Cyan)));
                    }
                    if let Some(span) = delivery_span(b.delivery) {
                        spans.push(Span::raw("  "));
                        spans.push(span);
                    }
                    Line::from(spans)
                } else {
                    let mut spans = vec![
                        gutter.clone(),
                        Span::raw(BODY_INDENT.to_string()),
                    ];
                    spans.extend(row_spans);
                    if b.streaming && is_last {
                        spans.push(Span::styled("▍", Style::default().fg(Color::Cyan)));
                    }
                    Line::from(spans)
                };
                total_rows = total_rows.saturating_add(wrapped_rows(&rendered, width));
                out.push(rendered);
            }
            if collapsed {
                let remaining = body_row_count.saturating_sub(COLLAPSED_BODY_LINES);
                let hint = Line::from(vec![
                    gutter.clone(),
                    Span::raw(BODY_INDENT.to_string()),
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

/// Width threshold under which the quote block collapses to a single inline
/// `↩ @actor` prefix instead of hanging on its own line.
const QUOTE_LINE_MIN_WIDTH: u16 = 60;
const COLLAPSED_BODY_LINES: usize = 5;

/// Indent that aligns hanging quote lines with the column where the actor
/// name starts on the header line (after the `[HH:MM:SS] ` timestamp prefix).
const QUOTE_TS_PAD: &str = "           ";

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
        .find(|b| b.trailing_event_id.as_deref() == Some(parent_id))
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

fn inline_quote_label(p: &ParentPreview) -> String {
    if p.available {
        format!("↩ @{}", p.actor)
    } else {
        "↩ (message unavailable)".into()
    }
}

fn build_quote_line(gutter: &Span<'static>, p: &ParentPreview) -> Line<'static> {
    let label = if p.available {
        format!("↩ @{}: {}", p.actor, p.text)
    } else {
        "↩ (message unavailable)".into()
    };
    Line::from(vec![
        gutter.clone(),
        Span::styled(
            format!("{}{}", QUOTE_TS_PAD, label),
            Style::default().fg(Color::DarkGray),
        ),
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
        BubbleKind::Stream => markdown::render_to_rows(display_text(&bubble.text), Style::default()),
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
    Some((event_id, format!("@{}: {}", actor, preview)))
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

/// Indent applied to non-first body rows so wrapped text aligns under the
/// bubble's actor name. Width = gutter (2) + `[HH:MM:SS] ` (11) - gutter (2)
/// already on the row = 11; we keep a 12-wide pad so a one-row delta in
/// timestamp formatting doesn't desync the column.
const BODY_INDENT: &str = "            ";

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
fn trim_trailing_blank_rows(
    mut rows: Vec<Vec<Span<'static>>>,
) -> Vec<Vec<Span<'static>>> {
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
    use std::collections::HashSet;

    use super::{
        display_text, preview_text, reply_target, reply_target_label, Bubble, BubbleKind,
        DeliveryState, History,
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
            streaming: false,
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
            streaming: false,
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
            streaming: false,
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
            streaming: false,
        });

        let rendered = history.render_lines(80, Some(0), &HashSet::new(), &|id| id.to_string());

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
            streaming: false,
        });

        let rendered = history.render_lines(16, Some(0), &HashSet::new(), &|id| id.to_string());
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_parent".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });
        history.bubbles.push(Bubble {
            actor_id: "Coder".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "这个任务已经完成了".into(),
            ts: Utc::now(),
            reply_to_event_id: Some("evt_parent".into()),
            trailing_event_id: Some("evt_reply".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });
        history
    }

    #[test]
    fn reply_target_label_omits_event_id() {
        let bubble = Bubble {
            actor_id: "bojun.cbj".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello world".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_abc123def456".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        };
        let (id, label) = reply_target_label(&bubble, &|id| id.to_string()).unwrap();
        assert_eq!(id, "evt_abc123def456");
        assert_eq!(label, "@bojun.cbj: hello world");
        assert!(!label.contains("evt_"));
    }

    #[test]
    fn render_lines_emits_quote_line_above_reply_when_wide() {
        let history = parent_and_reply();
        let rendered = history.render_lines(80, None, &HashSet::new(), &|id| id.to_string());
        // parent (1 line) + quote line + reply (1 line) = 3
        assert_eq!(rendered.lines.len(), 3);
        let quote = line_text(&rendered.lines[1]);
        assert!(quote.contains("↩ @bojun.cbj:"), "got: {quote:?}");
        assert!(quote.contains("the original"), "got: {quote:?}");
        assert!(!quote.contains("evt_"), "quote leaked event id: {quote:?}");
        let reply = line_text(&rendered.lines[2]);
        assert!(reply.contains("Coder"));
        assert!(reply.contains("这个任务已经完成了"));
        assert!(!reply.contains("↩"), "reply still has inline arrow: {reply:?}");
    }

    #[test]
    fn render_lines_inlines_quote_when_narrow() {
        let history = parent_and_reply();
        let rendered = history.render_lines(50, None, &HashSet::new(), &|id| id.to_string());
        // parent + reply (no quote line); inline ↩ @actor on the reply header
        assert_eq!(rendered.lines.len(), 2);
        let reply = line_text(&rendered.lines[1]);
        assert!(reply.contains("↩ @bojun.cbj"), "got: {reply:?}");
        assert!(!reply.contains("evt_"), "reply leaked event id: {reply:?}");
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
            reply_to_event_id: Some("evt_long_gone".into()),
            trailing_event_id: Some("evt_reply".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });
        let rendered = history.render_lines(80, None, &HashSet::new(), &|id| id.to_string());
        assert_eq!(rendered.lines.len(), 2);
        let quote = line_text(&rendered.lines[0]);
        assert!(quote.contains("(message unavailable)"), "got: {quote:?}");
        assert!(!quote.contains("evt_"));
    }

    #[test]
    fn render_lines_selected_range_includes_quote_line() {
        let history = parent_and_reply();
        // Select the reply (index 1). Range should cover both quote line + body.
        let rendered = history.render_lines(80, Some(1), &HashSet::new(), &|id| id.to_string());
        // quote line is at row 1, body at row 2.
        assert_eq!(rendered.selected_row_range, Some((1, 2)));
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_long".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });

        let rendered = history.render_lines(80, Some(0), &HashSet::new(), &|id| id.to_string());

        assert!(history.bubble_is_collapsible(0));
        assert_eq!(rendered.lines.len(), 6);
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_long".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });

        let mut expanded = HashSet::new();
        expanded.insert(0);
        let rendered = history.render_lines(80, Some(0), &expanded, &|id| id.to_string());

        assert_eq!(rendered.lines.len(), 7);
        let last = line_text(rendered.lines.last().unwrap());
        assert!(last.contains('7'));
    }

    fn make_event(id: &str, actor: &str, turn: Option<&str>, text: &str) -> Event {
        Event {
            id: id.into(),
            kind: "content.add".into(),
            actor_id: actor.into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: turn.map(|t| t.into()),
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": text }),
            relations: vec![],
            _meta: None,
        }
    }

    #[test]
    fn append_stream_delta_creates_streaming_bubble() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "Hel", Utc::now());
        assert_eq!(history.bubbles.len(), 1);
        let b = &history.bubbles[0];
        assert!(b.streaming);
        assert_eq!(b.actor_id, "Coder");
        assert_eq!(b.turn_id.as_deref(), Some("turn_1"));
        assert_eq!(b.text, "Hel");
    }

    #[test]
    fn append_stream_delta_appends_to_matching_turn() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "Hel", Utc::now());
        history.append_stream_delta("Coder", "turn_1", "lo", Utc::now());
        assert_eq!(history.bubbles.len(), 1);
        assert_eq!(history.bubbles[0].text, "Hello");
    }

    #[test]
    fn append_stream_delta_keeps_separate_bubble_per_turn() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "first", Utc::now());
        history.append_stream_delta("Coder", "turn_2", "second", Utc::now());
        assert_eq!(history.bubbles.len(), 2);
        assert_eq!(history.bubbles[0].text, "first");
        assert_eq!(history.bubbles[1].text, "second");
    }

    #[test]
    fn content_add_event_finalizes_streaming_bubble() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "partial...", Utc::now());
        // Authoritative event arrives with the full text. The streaming bubble
        // should flip to non-streaming and adopt the canonical text + event id.
        let ev = make_event("evt_99", "Coder", Some("turn_1"), "Hello world");
        history.push_event(&ev);
        assert_eq!(history.bubbles.len(), 1);
        let b = &history.bubbles[0];
        assert!(!b.streaming);
        assert_eq!(b.text, "Hello world");
        assert_eq!(b.trailing_event_id.as_deref(), Some("evt_99"));
    }

    #[test]
    fn cancelled_turn_close_renders_system_divider() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "in flight", Utc::now());
        // turn.close with status=cancelled should: (a) flip the streaming
        // bubble off, (b) push a system divider naming who cancelled.
        let close = Event {
            id: "evt_close".into(),
            kind: "turn.close".into(),
            actor_id: "Coder".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: Some("turn_1".into()),
            seq: 2,
            occurred_at: Utc::now(),
            payload: json!({
                "status": "cancelled",
                "stopReason": "user_cancelled",
                "_meta": { "cancelledBy": "bojun.cbj" }
            }),
            relations: vec![],
            _meta: None,
        };
        history.push_event(&close);
        assert_eq!(history.bubbles.len(), 2);
        assert!(!history.bubbles[0].streaming);
        let divider = &history.bubbles[1];
        assert_eq!(divider.kind, BubbleKind::System);
        assert!(divider.text.contains("cancelled by @bojun.cbj"));
    }

    #[test]
    fn non_cancelled_turn_close_is_still_suppressed() {
        // Sanity: regular turn.close still produces no bubble (the closing
        // chunk arrives separately as content.add).
        let mut history = History::default();
        let close = Event {
            id: "evt_close".into(),
            kind: "turn.close".into(),
            actor_id: "Coder".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: Some("turn_1".into()),
            seq: 2,
            occurred_at: Utc::now(),
            payload: json!({ "status": "closed" }),
            relations: vec![],
            _meta: None,
        };
        history.push_event(&close);
        assert!(history.bubbles.is_empty());
    }

    #[test]
    fn streaming_turns_lists_in_flight_bubbles_only() {
        let mut history = History::default();
        history.append_stream_delta("Coder", "turn_1", "live", Utc::now());
        // Add a non-streaming bubble — should not appear in the list.
        history.bubbles.push(Bubble {
            actor_id: "OpenCode".into(),
            turn_id: Some("turn_old".into()),
            kind: BubbleKind::Stream,
            text: "done".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_done".into()),
            delivery: DeliveryState::NotApplicable,
            streaming: false,
        });
        let live = history.streaming_turns();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0, "Coder");
        assert_eq!(live[0].1, "turn_1");
    }
}
