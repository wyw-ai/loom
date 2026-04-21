use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Mode};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

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

    let constraints: Vec<Constraint> = if dropdown_h > 0 {
        vec![
            Constraint::Length(1),          // title
            Constraint::Min(3),             // history
            Constraint::Length(dropdown_h), // inline dropdown
            Constraint::Length(3),          // input
            Constraint::Length(1),          // status
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ]
    };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_title(f, app, outer[0]);
    render_history(f, app, outer[1]);

    if dropdown_h > 0 {
        if let Some(p) = app.slash_menu.as_mut() {
            p.render_inline(f, outer[2]);
        } else if let Some(p) = app.at_menu.as_mut() {
            p.render_inline(f, outer[2]);
        }
        render_input(f, app, outer[3]);
        render_status(f, app, outer[4]);
    } else {
        render_input(f, app, outer[2]);
        render_status(f, app, outer[3]);
    }

    if let (Mode::Picker(_), Some(p)) = (&app.mode, app.picker.as_mut()) {
        p.render(f, area);
    }
}

fn render_title(f: &mut Frame, app: &App, area: Rect) {
    let display = app.display_name_for(&app.actor_id);
    let line =
        Line::from(vec![
        Span::styled(
            " Joi ",
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("#{}", app.thread_id),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(format!("as {}", display), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(
            "(/ for commands · Ctrl-R=reply · ↑/↓/PgUp/PgDn/scroll wheel · Home/End · Ctrl-C=quit)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_history(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let display_for = |id: &str| app.display_name_for(id);
    let lines = app.history.render_lines(&display_for);
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total = para.line_count(inner.width).min(u16::MAX as usize) as u16;
    let visible = inner.height;
    let max_scroll = total.saturating_sub(visible);
    if app.auto_follow {
        app.scroll = max_scroll;
    } else if app.scroll >= max_scroll {
        app.scroll = max_scroll;
        app.auto_follow = true;
    }
    let para = para.scroll((app.scroll, 0));
    f.render_widget(para, inner);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.reply_target.as_ref() {
        Some(target) => format!(
            " Message (reply → {} · Enter=send · Esc=clear · /… for slash) ",
            target.preview
        ),
        None => " Message (Enter to send · /…  for slash) ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    let visible = visible_input_tail(&app.input, inner.width.saturating_sub(3) as usize);
    let line = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(visible),
        Span::styled("_", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![Span::styled(
        format!(" {}", app.status),
        Style::default().fg(Color::DarkGray),
    )]);
    f.render_widget(Paragraph::new(line), area);
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
