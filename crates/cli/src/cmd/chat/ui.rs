use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::announcement;
use super::app::{App, Mode};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Horizontal split: optional left sidebar (28) | chat | optional right
    // announcement panel (PANEL_WIDTH). Each side is taken only when the
    // corresponding state is present, so a screen with neither uses the full
    // width for chat. The announcement column is omitted on narrow terminals
    // (chat falls below 40 cols) so the chat pane keeps a usable width.
    let want_announcement = app.history.current_announcement.is_some();
    let sidebar_w: u16 = if app.sidebar.is_some() { 28 } else { 0 };
    let chat_min: u16 = 40;
    let announce_w: u16 = if want_announcement
        && area.width >= sidebar_w + chat_min + announcement::PANEL_WIDTH
    {
        announcement::PANEL_WIDTH
    } else {
        0
    };

    let mut h_constraints: Vec<Constraint> = Vec::new();
    if sidebar_w > 0 {
        h_constraints.push(Constraint::Length(sidebar_w));
    }
    h_constraints.push(Constraint::Min(20));
    if announce_w > 0 {
        h_constraints.push(Constraint::Length(announce_w));
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(h_constraints)
        .split(area);

    let mut col_idx = 0usize;
    let sidebar_area = if sidebar_w > 0 {
        let r = cols[col_idx];
        col_idx += 1;
        Some(r)
    } else {
        None
    };
    let main_area = cols[col_idx];
    col_idx += 1;
    let announcement_area = if announce_w > 0 {
        Some(cols[col_idx])
    } else {
        None
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

    if let (Some(rect), Some(a)) = (announcement_area, app.history.current_announcement.as_ref()) {
        let display = |id: &str| app.display_name_for(id);
        announcement::render(f, a, &display, rect);
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
    let kind_for = |id: &str| app.actor_kinds.get(id).cloned();
    let rendered = app.history.render_lines(
        inner.width,
        app.selected_history_idx,
        &app.expanded_history,
        &display_for,
        &kind_for,
    );
    let visible = inner.height.max(1);
    let max_scroll = rendered.total_rows.saturating_sub(visible);
    if app.auto_follow {
        app.scroll = max_scroll;
    } else if let Some((start, end)) = rendered.selected_row_range {
        let selected_height = end.saturating_sub(start).saturating_add(1);
        let visible_bottom = app.scroll.saturating_add(visible.saturating_sub(1));
        if selected_height >= visible {
            app.scroll = start;
        } else if start < app.scroll {
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
fn render_streaming_bar(f: &mut Frame, app: &App, area: Rect, in_flight: &[InFlightRow]) {
    let now = chrono::Utc::now();
    // Frame is a function of wall time so any redraw advances it (the chat
    // event loop wakes every ~100ms via `poll`, which is the spinner cadence).
    const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let frame_idx =
        ((now.timestamp_millis().max(0) as u64 / 100) % SPINNER_FRAMES.len() as u64) as usize;
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        format!(" {} ", SPINNER_FRAMES[frame_idx]),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    let mut first = true;
    for row in in_flight {
        if !first {
            spans.push(Span::styled("   ", Style::default().fg(Color::DarkGray)));
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
    let layout = input_layout(app, inner.width);
    f.render_widget(
        Paragraph::new(layout.lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
    f.set_cursor_position((
        inner.x.saturating_add(layout.cursor_x),
        inner.y.saturating_add(layout.cursor_y),
    ));
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
    input_layout(app, inner_width).lines.len().max(1).min(6) as u16 + 2
}

struct InputLayout {
    lines: Vec<Line<'static>>,
    cursor_x: u16,
    cursor_y: u16,
}

fn input_layout(app: &App, width: u16) -> InputLayout {
    let input = app.input.display_text();
    let cursor_char_idx = app.input.display_cursor_char_index();
    let content_width = width.saturating_sub(2).max(1) as usize;

    if input.contains('\n') {
        let window = visible_multiline_window(&input, cursor_char_idx, content_width, 6);
        return InputLayout {
            lines: render_input_lines(&window.rows),
            cursor_x: if window.cursor_y == 0 {
                2u16.saturating_add(window.cursor_x)
            } else {
                window.cursor_x
            },
            cursor_y: window.cursor_y,
        };
    }

    let window = visible_input_window(&input, cursor_char_idx, content_width);
    InputLayout {
        lines: render_input_lines(&[window.text]),
        cursor_x: 2u16.saturating_add(window.cursor_x),
        cursor_y: 0,
    }
}

fn render_input_lines(rows: &[String]) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return vec![Line::from("> ")];
    }
    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            if idx == 0 {
                Line::from(format!("> {row}"))
            } else {
                Line::from(row.clone())
            }
        })
        .collect()
}

struct InputWindow {
    text: String,
    cursor_x: u16,
}

fn visible_input_window(input: &str, cursor_char_idx: usize, max_width: usize) -> InputWindow {
    if max_width == 0 {
        return InputWindow {
            text: String::new(),
            cursor_x: 0,
        };
    }
    if input.width() <= max_width {
        return InputWindow {
            text: input.to_string(),
            cursor_x: width_of_chars(input, cursor_char_idx)
                .min(u16::MAX as usize) as u16,
        };
    }

    let marker = "<";
    let marker_width = marker.width();
    if max_width <= marker_width {
        return InputWindow {
            text: marker.chars().take(max_width).collect(),
            cursor_x: marker_width.min(max_width).min(u16::MAX as usize) as u16,
        };
    }

    let budget = max_width - marker_width;
    let chars: Vec<char> = input.chars().collect();
    let widths: Vec<usize> = chars.iter().map(|ch| char_display_width(*ch)).collect();
    let total = chars.len();
    let cursor_char_idx = cursor_char_idx.min(total);
    let mut cumulative = Vec::with_capacity(total + 1);
    cumulative.push(0);
    for width in &widths {
        cumulative.push(cumulative.last().copied().unwrap_or(0) + width);
    }

    let total_width = cumulative[total];
    let cursor_width = cumulative[cursor_char_idx];
    let target_left_width = cursor_width.saturating_sub(budget / 2);
    let mut start_char = cumulative.partition_point(|width| *width <= target_left_width);
    start_char = start_char.saturating_sub(1).min(total);

    let mut end_char = start_char;
    while end_char < total && cumulative[end_char + 1] - cumulative[start_char] <= budget {
        end_char += 1;
    }
    while end_char == total
        && start_char > 0
        && total_width - cumulative[start_char - 1] <= budget
    {
        start_char -= 1;
    }

    let body: String = chars[start_char..end_char].iter().collect();
    let visible_width = cursor_width.saturating_sub(cumulative[start_char]);
    if start_char > 0 {
        InputWindow {
            text: format!("{marker}{body}"),
            cursor_x: marker_width
                .saturating_add(visible_width)
                .min(u16::MAX as usize) as u16,
        }
    } else {
        InputWindow {
            text: body.clone(),
            cursor_x: visible_width.min(u16::MAX as usize) as u16,
        }
    }
}

struct MultilineWindow {
    rows: Vec<String>,
    cursor_x: u16,
    cursor_y: u16,
}

fn visible_multiline_window(
    input: &str,
    cursor_char_idx: usize,
    row_width: usize,
    max_rows: usize,
) -> MultilineWindow {
    let wrapped = wrap_input_rows(input, cursor_char_idx, row_width.max(1));
    let visible_rows = max_rows.max(1);
    let max_start = wrapped.rows.len().saturating_sub(visible_rows);
    let start_row = wrapped
        .cursor_row
        .saturating_add(1)
        .saturating_sub(visible_rows)
        .min(max_start);
    let end_row = (start_row + visible_rows).min(wrapped.rows.len());
    MultilineWindow {
        rows: wrapped.rows[start_row..end_row].to_vec(),
        cursor_x: wrapped.cursor_x.min(u16::MAX as usize) as u16,
        cursor_y: wrapped
            .cursor_row
            .saturating_sub(start_row)
            .min(u16::MAX as usize) as u16,
    }
}

struct WrappedRows {
    rows: Vec<String>,
    cursor_row: usize,
    cursor_x: usize,
}

fn wrap_input_rows(input: &str, cursor_char_idx: usize, row_width: usize) -> WrappedRows {
    let row_width = row_width.max(1);
    let mut rows = vec![String::new()];
    let mut current_width = 0usize;
    let mut cursor_row = 0usize;
    let mut cursor_x = 0usize;
    let mut seen = 0usize;

    for ch in input.chars() {
        if seen == cursor_char_idx {
            cursor_row = rows.len() - 1;
            cursor_x = current_width;
        }
        seen += 1;
        if ch == '\n' {
            rows.push(String::new());
            current_width = 0;
            continue;
        }
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if current_width > 0 && current_width + ch_width > row_width {
            rows.push(String::new());
            current_width = 0;
        }
        rows.last_mut().expect("rows is never empty").push(ch);
        current_width += ch_width;
    }

    if seen == cursor_char_idx {
        cursor_row = rows.len() - 1;
        cursor_x = current_width;
    }

    WrappedRows {
        rows,
        cursor_row,
        cursor_x,
    }
}

fn width_of_chars(input: &str, char_count: usize) -> usize {
    input
        .chars()
        .take(char_count)
        .map(char_display_width)
        .sum()
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

#[cfg(test)]
mod tests {
    use super::{visible_input_window, visible_multiline_window};

    #[test]
    fn visible_input_window_keeps_cursor_visible() {
        let visible = visible_input_window("hello", 5, 8);
        assert_eq!(visible.text, "hello");
        assert_eq!(visible.cursor_x, 5);

        let visible = visible_input_window("abcdefghij", 3, 6);
        assert_eq!(visible.text, "<bcdef");
        assert_eq!(visible.cursor_x, 3);
    }

    #[test]
    fn visible_input_window_respects_wide_char_width() {
        let visible = visible_input_window("你好世界ab", 2, 5);
        assert_eq!(visible.text, "<好世");
        assert_eq!(visible.cursor_x, 3);
    }

    #[test]
    fn visible_multiline_window_keeps_cursor_in_view() {
        let input = "1\n2\n3\n4\n5\n6\n7";
        let window = visible_multiline_window(input, input.chars().count(), 20, 6);
        assert_eq!(window.rows, vec!["2", "3", "4", "5", "6", "7"]);
        assert_eq!(window.cursor_y, 5);
        assert_eq!(window.cursor_x, 1);
    }
}
