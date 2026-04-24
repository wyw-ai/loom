use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Mode};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // When the Discord-style sidebar is open, peel off a fixed-width left
    // column and render the existing chat layout into the remaining area.
    let (sidebar_area, main_area) = if app.sidebar.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(20)])
            .split(area);
        (Some(cols[0]), cols[1])
    } else {
        (None, area)
    };

    // Inline dropdown sizing: 1 border + N items + 1 border, capped at 8 rows.
    // Only one of slash/at menus can be open at a time (the input can't begin
    // with both `/` and `@`), so a single rect serves either.
    let dropdown_h: u16 = if let Some(p) = app.slash_menu.as_ref() {
        let n = p.filtered().len() as u16;
        n.saturating_add(2).clamp(3, 8)
    } else if let Some(p) = app.at_menu.as_ref() {
        let n = p.filtered().len() as u16;
        n.saturating_add(2).clamp(3, 10)
    } else {
        0
    };

    // In-flight status: 1 dim row above the input, only when at least one
    // agent has an open turn in this scope. Sourced from `open_turns` (driven
    // by `turn.opened` / `turn.closed`) so the bar appears even before the
    // agent has emitted a single `turn/stream.update` delta — that's the
    // window during which Esc-cancel needs to be discoverable.
    let streaming_bubbles = app.history.streaming_turns();
    let in_flight = if let Some(scope) = app.current_scope() {
        in_flight_turns(app, &scope, &streaming_bubbles)
    } else {
        Vec::new()
    };
    let streaming_h: u16 = if in_flight.is_empty() { 0 } else { 1 };

    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(1), // title
        Constraint::Min(3),    // history
    ];
    if streaming_h > 0 {
        constraints.push(Constraint::Length(streaming_h));
    }
    if dropdown_h > 0 {
        constraints.push(Constraint::Length(dropdown_h));
    }
    constraints.push(Constraint::Length(input_height(app, main_area.width))); // input
    constraints.push(Constraint::Length(1)); // status

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(main_area);

    render_title(f, app, outer[0]);
    render_history(f, app, outer[1]);

    let mut idx = 2usize;
    if streaming_h > 0 {
        render_streaming_bar(f, app, outer[idx], &in_flight);
        idx += 1;
    }
    if dropdown_h > 0 {
        if let Some(p) = app.slash_menu.as_mut() {
            p.render_inline(f, outer[idx]);
        } else if let Some(p) = app.at_menu.as_mut() {
            p.render_inline(f, outer[idx]);
        }
        idx += 1;
    }
    render_input(f, app, outer[idx]);
    render_status(f, app, outer[idx + 1]);

    if let Some(sidebar_rect) = sidebar_area {
        if let Some(s) = app.sidebar.as_mut() {
            s.render(f, sidebar_rect);
        }
    }

    if let (Mode::Picker(_), Some(p)) = (&app.mode, app.picker.as_mut()) {
        p.render(f, area);
    }

    // Prompt overlay sits on top of everything else (including the sidebar).
    if let Some(p) = app.prompt.as_ref() {
        p.render(f, area);
    }
}

fn render_title(f: &mut Frame, app: &App, area: Rect) {
    use crate::cmd::chat::sidebar::SidebarFocus;
    let display = app.display_name_for(&app.actor_id);
    // Tail hint depends on which sidebar pane has focus — so the operator
    // sees the actionable hotkeys for the pane they're currently driving.
    let tail = match app.sidebar.as_ref().map(|s| s.focus) {
        Some(SidebarFocus::Members) => {
            "(i=invite · I=invite by id · x=revoke · Tab=next pane · Ctrl-B=close · Ctrl-C=quit)"
        }
        Some(_) => {
            "(↑/↓ · Enter=open · n=new · r=rename · d=delete · Tab=next pane · Ctrl-B=close · Ctrl-C=quit)"
        }
        None => "(/ commands · r=reply selected · Ctrl-R=picker · Ctrl-B=channels · Ctrl-C=quit)",
    };
    let (thread_label, thread_style) = if app.has_scope() {
        let prefix = match app.scope_kind {
            proto::types::ScopeKind::Channel => "#",
            proto::types::ScopeKind::Thread => "🧵",
        };
        (
            format!("{}{}", prefix, app.thread_id),
            Style::default().fg(Color::Cyan),
        )
    } else {
        (
            "(no scope — pick a channel/thread in the sidebar)".to_string(),
            Style::default().fg(Color::DarkGray),
        )
    };
    let line = Line::from(vec![
        Span::styled(
            " Joi ",
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(thread_label, thread_style),
        Span::raw("  "),
        Span::styled(
            format!("as {}", display),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(tail, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_history(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(
            " History  (↑/↓=select · ←/→=collapse/expand · r=reply · Ctrl-R=picker · PgUp/PgDn=scroll) ",
        )
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let display_for = |id: &str| app.display_name_for(id);
    let rendered = app
        .history
        .render_lines(
            inner.width,
            app.selected_history_idx,
            &app.expanded_history,
            &display_for,
        );
    let visible = inner.height.max(1);
    let max_scroll = rendered.total_rows.saturating_sub(visible);
    if app.auto_follow {
        app.scroll = max_scroll;
    } else if let Some((start, end)) = rendered.selected_row_range {
        let visible_bottom = app.scroll.saturating_add(visible.saturating_sub(1));
        if start < app.scroll {
            app.scroll = start;
        } else if end > visible_bottom {
            app.scroll = end.saturating_sub(visible.saturating_sub(1));
        }
    }
    if app.scroll >= max_scroll {
        app.scroll = max_scroll;
        if app.selected_history_idx.is_none() {
            app.auto_follow = true;
        }
    }
    let para = Paragraph::new(rendered.lines)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(para, inner);
}

/// Single-row dim bar that lists every agent with an open turn in scope.
/// Hint at the end tells the user how to stop them: Esc cancels the only
/// one (or the selected bubble), `/cancel @agent` for explicit.
fn render_streaming_bar(
    f: &mut Frame,
    app: &App,
    area: Rect,
    in_flight: &[InFlightRow],
) {
    let now = chrono::Utc::now();
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        " ⠋ ",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];
    let mut first = true;
    for row in in_flight {
        if !first {
            spans.push(Span::styled(
                "   ",
                Style::default().fg(Color::DarkGray),
            ));
        }
        first = false;
        let elapsed = (now - row.started).num_seconds().max(0);
        let display = app.display_name_for(&row.actor);
        let suffix = if row.streaming { "" } else { " (waiting)" };
        spans.push(Span::styled(
            format!("@{display} · {elapsed}s{suffix}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::styled(
        if in_flight.len() == 1 {
            "    Esc to cancel".to_string()
        } else {
            "    Esc cancels selected · /cancel @agent for one".to_string()
        },
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// One row in the in-flight bar. `streaming=true` when the agent has emitted
/// at least one stream delta (so we know it's actively producing output);
/// `false` when only `turn.opened` has fired (queued or thinking, no output yet).
pub(super) struct InFlightRow {
    pub actor: String,
    pub started: chrono::DateTime<chrono::Utc>,
    pub streaming: bool,
}

/// Merge open-turn registry entries with streaming-bubble metadata into the
/// rows the in-flight bar renders. Open turns without a corresponding
/// streaming bubble still surface — that's the whole point of this fix —
/// but get a `(waiting)` suffix so the operator knows the agent hasn't
/// started typing yet.
pub(super) fn in_flight_turns(
    app: &App,
    scope: &proto::types::ScopeRef,
    streaming_bubbles: &[(String, String, chrono::DateTime<chrono::Utc>)],
) -> Vec<InFlightRow> {
    use std::collections::HashMap;
    let stream_by_turn: HashMap<&str, &chrono::DateTime<chrono::Utc>> = streaming_bubbles
        .iter()
        .map(|(_, turn_id, started)| (turn_id.as_str(), started))
        .collect();
    let mut rows: Vec<InFlightRow> = app
        .open_turns_in_scope(scope)
        .into_iter()
        .map(|t| InFlightRow {
            actor: t.actor_id.clone(),
            started: stream_by_turn
                .get(t.turn_id.as_str())
                .map(|&&dt| dt)
                .unwrap_or(t.opened_at),
            streaming: stream_by_turn.contains_key(t.turn_id.as_str()),
        })
        .collect();
    rows.sort_by_key(|r| r.started);
    rows
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let (title, border_color) = if !app.has_scope() {
        (
            " Message (pick a channel or thread to start chatting) ".to_string(),
            Color::DarkGray,
        )
    } else {
        let title = match app.reply_target.as_ref() {
            Some(target) => format!(
                " Message (reply → {} · Enter=send · Alt+Enter/Ctrl+J=new line · Esc=clear · /… for slash) ",
                target.preview
            ),
            None => {
                " Message (Enter=send · Alt+Enter/Ctrl+J=new line · /… for slash) ".to_string()
            }
        };
        (title, Color::Cyan)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(
        Paragraph::new(input_display_text(app, inner.width))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![Span::styled(
        format!(" {}", app.status),
        Style::default().fg(Color::DarkGray),
    )]);
    f.render_widget(Paragraph::new(line), area);
}

fn input_height(app: &App, width: u16) -> u16 {
    let inner_width = width.saturating_sub(2).max(1);
    let rows = Paragraph::new(input_display_text(app, inner_width))
        .wrap(Wrap { trim: false })
        .line_count(inner_width)
        .max(1)
        .min(6);
    rows.saturating_add(2) as u16
}

fn input_display_text(app: &App, width: u16) -> String {
    let input = app.input.display_text();
    let visible = if input.contains('\n') {
        input
    } else {
        visible_input_tail(&input, width.saturating_sub(3) as usize)
    };
    format!("> {visible}_")
}

fn visible_input_tail(input: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if input.width() <= max_width {
        return input.to_string();
    }

    let marker = "<";
    let marker_width = marker.width();
    if max_width <= marker_width {
        return marker.chars().take(max_width).collect();
    }

    let budget = max_width - marker_width;
    let mut width = 0usize;
    let mut start = input.len();

    for (idx, ch) in input.char_indices().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > budget {
            break;
        }
        width += ch_width;
        start = idx;
    }

    format!("{}{}", marker, &input[start..])
}

#[cfg(test)]
mod tests {
    use super::visible_input_tail;

    #[test]
    fn visible_input_tail_shows_end_of_long_input() {
        assert_eq!(visible_input_tail("hello", 8), "hello");
        assert_eq!(visible_input_tail("abcdefghij", 6), "<fghij");
    }
}
