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
    /// Target actor id when this bubble represents an explicit handoff
    /// (`/handoff @x` or `@x msg`). The body line is rendered as
    /// `|-> handoff -> {display(target)} ({short_id}): {text}` instead of
    /// the raw text — keeping the target id out of the stored string lets
    /// us look the display name up at render time.
    pub handoff_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BubbleKind {
    /// content.add; subsequent same-actor/turn chunks append in place
    Stream,
    /// Non-content events (handoff, action.response, etc.)
    Static,
    /// `action.request` events targeting the human. Rendered with extra
    /// prominence so the operator notices that the agent is parked waiting
    /// for a decision; counted by `pending_action_requests` until an
    /// `action.response` matches the trailing event id.
    ActionRequest,
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

/// Latest pinned announcement for the current scope. Derived purely by
/// reducing `announcement.set` / `announcement.clear` events as they pass
/// through `push_event` — the canonical state lives in the journal, this
/// is just the cached "current value" the UI panel reads.
#[derive(Debug, Clone)]
pub struct Announcement {
    /// Body text (markdown). Rendered by the announcement panel.
    pub text: String,
    /// Actor that posted/edited the announcement.
    pub actor_id: String,
    /// Server timestamp of the `announcement.set` event.
    pub ts: DateTime<Utc>,
}

#[derive(Default)]
pub struct History {
    pub bubbles: Vec<Bubble>,
    /// Latest announcement, or `None` if none has been set or the most
    /// recent event was an `announcement.clear`. Replaced wholesale by each
    /// new `announcement.set`; cold-start `scope/read` replays naturally
    /// converge on the latest.
    pub current_announcement: Option<Announcement>,
    /// Event ids of `action.request` events that have already been answered
    /// in this scope. Populated from the `RespondsTo` relation on incoming
    /// `action.response` events. Stored as a side set because the response
    /// and request live in different bubbles and their own event ids differ
    /// — comparing `trailing_event_id` directly never matches.
    acked_request_ids: HashSet<String>,
}

pub struct RenderedHistory {
    pub lines: Vec<Line<'static>>,
    pub total_rows: u16,
    pub selected_row_range: Option<(u16, u16)>,
}

impl History {
    pub fn push_event(&mut self, ev: &Event) {
        match ev.kind.as_str() {
            // Explicit handoff (e.g. `/handoff @x msg` or `@x msg`) — no
            // parent message — renders as its own static "↪ handoff → x: …"
            // line so it stands apart from regular chat.
            //
            // Reply-as-handoff (the common case: user picks `r` on someone's
            // message, types a response) gets BOTH `hands_off_to` and
            // `replies_to`; it should look like a normal reply with the dim
            // `↩ @target` quote line above it. Falling through to
            // `append_stream` keeps a single bubble AND lets the sender's
            // pending outgoing bubble flip to Delivered when the echo lands.
            "content.add" if hands_off_target(ev).is_some() && reply_target(ev).is_none() => {
                let target = hands_off_target(ev).unwrap_or_default();
                let msg = handoff_message(ev);
                self.push_handoff(ev, target, msg);
            }
            "content.add" => self.append_stream(ev),
            "action.request" => self.push_action_request(ev, format_action_request(ev)),
            "action.response" => {
                if let Some(target) = responds_to_target(ev) {
                    self.acked_request_ids.insert(target);
                }
                self.push_static(ev, format_action_response(ev));
            }
            // Pinned announcement updates: render in the right-side panel
            // rather than as a chat bubble. We still want them to flow
            // through `push_event` so cold-start `scope/read` replays
            // converge naturally on the latest value.
            "announcement.set" => self.apply_announcement_set(ev),
            "announcement.clear" => self.current_announcement = None,
            // turn.close is normally suppressed; the closing content arrives
            // as a normal content.add event. Cancelled turns are the exception
            // because other channel members should see who pulled the plug.
            "turn.close" => {
                let is_cancelled = ev
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("cancelled"))
                    .unwrap_or(false);
                if is_cancelled {
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
            handoff_target: None,
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
            handoff_target: None,
        });
    }

    fn append_stream(&mut self, ev: &Event) {
        let text = ev
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // If this is our own outgoing bubble we already pushed locally, just
        // flip to Delivered and skip — we'd otherwise duplicate the text.
        // Scan back across all bubbles so a stray notification arriving
        // between push_outgoing and the echo can't strand us in Pending.
        for b in self.bubbles.iter_mut().rev() {
            if b.delivery == DeliveryState::Pending
                && b.trailing_event_id.as_deref() == Some(ev.id.as_str())
            {
                b.delivery = DeliveryState::Delivered;
                return;
            }
        }
        if let Some(b) = self.bubbles.last_mut() {
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
            handoff_target: None,
        });
    }

    /// Reduce an `announcement.set` event into `current_announcement`.
    /// Empty `payload.text` is treated as a clear so callers can either
    /// emit `announcement.clear` or post `set` with `text: ""`.
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
            handoff_target: None,
        });
    }

    fn push_action_request(&mut self, ev: &Event, text: String) {
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::ActionRequest,
            text,
            ts: ev.occurred_at,
            reply_to_event_id: reply_target(ev),
            trailing_event_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
        });
    }

    /// Static bubble for an explicit handoff. Stores only the message body
    /// in `text` and stashes the target id on the bubble; the renderer
    /// formats `|-> handoff -> {display(target)} ({short_id}): {text}` so
    /// the target's display name updates if the actor directory changes.
    fn push_handoff(&mut self, ev: &Event, target: String, message: String) {
        self.bubbles.push(Bubble {
            actor_id: ev.actor_id.clone(),
            turn_id: ev.turn_id.clone(),
            kind: BubbleKind::Static,
            text: message,
            ts: ev.occurred_at,
            reply_to_event_id: reply_target(ev),
            trailing_event_id: Some(ev.id.clone()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: Some(target),
        });
    }

    pub fn pending_action_requests(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for b in &self.bubbles {
            if b.kind == BubbleKind::ActionRequest {
                if let Some(eid) = b.trailing_event_id.as_ref() {
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
                .reply_to_event_id
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

            // Body. Handoff bubbles get a dedicated `handoff -> ...`
            // line; stream bubbles go through the markdown renderer; other
            // static bubbles render their pre-formatted text verbatim.
            let body_rows: Vec<Vec<Span<'static>>> =
                if let (BubbleKind::Static, Some(target)) = (&b.kind, b.handoff_target.as_ref()) {
                    let label = format!(
                        "handoff -> {} ({}): {}",
                        display_for(target),
                        short_actor_ref(target),
                        display_text(&b.text)
                    );
                    label
                        .split('\n')
                        .map(|line| {
                            vec![Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::DarkGray),
                            )]
                        })
                        .collect()
                } else {
                    bubble_body_rows(b)
                };
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

/// Event id this event responds to, e.g. an `action.response` pointing back
/// at the `action.request` it answers. Used to retire the request from the
/// pending set in `pending_action_requests`.
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

fn handoff_message(ev: &Event) -> String {
    // Body lives in `text` for content.add+HandsOffTo; legacy `handoff.offer`
    // events used `message` — keep the fallback so old journals render.
    ev.payload
        .get("text")
        .and_then(|v| v.as_str())
        .or_else(|| ev.payload.get("message").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

/// Compact actor id used in handoff render (e.g. `actor_agent_opencode_v1`
/// → `agent_opencode_v1`, then truncated to 16 chars). Identical strategy
/// to `app::short_actor_ref` — duplicated here to avoid pulling the App
/// module into history's dependency graph.
fn short_actor_ref(id: &str) -> String {
    let compact = id.strip_prefix("actor_").unwrap_or(id);
    if compact.chars().count() > 16 {
        format!("{}…", compact.chars().take(16).collect::<String>())
    } else {
        compact.to_string()
    }
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
                kind: RelationKind::HandsOffTo,
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

        // An action.response carrying the same trailing event id (pushed via
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
            handoff_target: None,
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
            handoff_target: None,
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
            handoff_target: None,
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
            handoff_target: None,
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_parent".into()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
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
            handoff_target: None,
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
            handoff_target: None,
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
        assert!(!quote.contains("evt_"), "quote leaked event id: {quote:?}");
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
            reply_to_event_id: Some("evt_long_gone".into()),
            trailing_event_id: Some("evt_reply".into()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_x".into()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
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
    fn render_lines_handoff_uses_new_format() {
        let mut history = History::default();
        let ev = Event {
            id: "evt_h".into(),
            kind: "content.add".into(),
            actor_id: "actor_human_self".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "please look" }),
            relations: vec![Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_agent_opencode".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };
        history.push_event(&ev);

        let display_for = |id: &str| match id {
            "actor_human_self" => "bojun.cbj".to_string(),
            "actor_agent_opencode" => "Coder".to_string(),
            other => other.to_string(),
        };
        let rendered = history.render_lines(120, None, &HashSet::new(), &display_for, &|_| None);
        // header + body
        assert_eq!(rendered.lines.len(), 2);
        let body = line_text(&rendered.lines[1]);
        assert!(
            body.contains("handoff -> Coder (agent_opencode): please look"),
            "got: {body:?}"
        );
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
            handoff_target: None,
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
            reply_to_event_id: None,
            trailing_event_id: Some("evt_long".into()),
            delivery: DeliveryState::NotApplicable,
            handoff_target: None,
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

    #[test]
    fn cancelled_turn_close_renders_system_divider() {
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
            payload: json!({
                "status": "cancelled",
                "stopReason": "user_cancelled",
                "_meta": { "cancelledBy": "bojun.cbj" }
            }),
            relations: vec![],
            _meta: None,
        };
        history.push_event(&close);
        assert_eq!(history.bubbles.len(), 1);
        let divider = &history.bubbles[0];
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

    fn make_event_with_relations(
        id: &str,
        actor: &str,
        text: &str,
        relations: Vec<Relation>,
    ) -> Event {
        Event {
            id: id.into(),
            kind: "content.add".into(),
            actor_id: actor.into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": text }),
            relations,
            _meta: None,
        }
    }

    #[test]
    fn explicit_handoff_without_reply_renders_static_bubble() {
        // `@target msg` / `/handoff @target msg` — no replies_to, just
        // hands_off_to. The bubble stores the message body (not the
        // formatted line) and stashes the target id for the renderer.
        let mut history = History::default();
        let ev = make_event_with_relations(
            "evt_h",
            "actor_human",
            "do this",
            vec![Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_agent_coder".into(),
                    _meta: None,
                },
                _meta: None,
            }],
        );
        history.push_event(&ev);
        assert_eq!(history.bubbles.len(), 1);
        assert_eq!(history.bubbles[0].kind, BubbleKind::Static);
        assert_eq!(history.bubbles[0].text, "do this");
        assert_eq!(
            history.bubbles[0].handoff_target.as_deref(),
            Some("actor_agent_coder")
        );
    }

    #[test]
    fn reply_with_handoff_finalizes_pending_outgoing_bubble() {
        // The user replies to an agent: the echoed event has BOTH replies_to
        // and hands_off_to. The pending outgoing bubble (pushed locally) must
        // flip to Delivered, and no extra static "↪ handoff" bubble appears.
        let mut history = History::default();
        history.push_outgoing(
            "actor_human",
            "evt_reply",
            Utc::now(),
            "got it".into(),
            Some("evt_parent".into()),
        );
        assert_eq!(history.bubbles[0].delivery, DeliveryState::Pending);

        let ev = make_event_with_relations(
            "evt_reply",
            "actor_human",
            "got it",
            vec![
                Relation {
                    kind: RelationKind::RepliesTo,
                    target: Ref {
                        kind: RefKind::Event,
                        id: "evt_parent".into(),
                        _meta: None,
                    },
                    _meta: None,
                },
                Relation {
                    kind: RelationKind::HandsOffTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: "actor_agent_coder".into(),
                        _meta: None,
                    },
                    _meta: None,
                },
            ],
        );
        history.push_event(&ev);

        // Still a single bubble — flipped to Delivered, no duplicate handoff line.
        assert_eq!(history.bubbles.len(), 1);
        assert_eq!(history.bubbles[0].delivery, DeliveryState::Delivered);
        assert_eq!(history.bubbles[0].text, "got it");
        assert!(!history
            .bubbles
            .iter()
            .any(|b| b.text.starts_with("↪ handoff")));
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
