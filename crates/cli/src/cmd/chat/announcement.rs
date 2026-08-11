//! Right-side pinned-announcement panel.
//!
//! Renders the latest `announcement.set` payload for the current scope as a
//! bordered column on the right edge of the chat layout. The panel is
//! presence-driven: when there is no current announcement, the column is
//! omitted entirely so the chat history reclaims the width.

use chrono::Local;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::history::Announcement;
use super::markdown;

/// Fixed width of the right-side announcement column. Picked to fit a
/// short title + ~28 chars of body before wrapping starts mattering.
pub const PANEL_WIDTH: u16 = 32;

/// Render the announcement panel into `area`. `display_for` resolves the
/// poster's actor id to a friendly name in the footer.
pub fn render(
    f: &mut Frame,
    announcement: &Announcement,
    display_for: &dyn Fn(&str) -> String,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " Announcement ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Body: markdown rows + a footer line "— @actor 04/24 14:30".
    let mut lines: Vec<Line<'static>> =
        markdown::render_to_rows(&announcement.text, Style::default())
            .into_iter()
            .map(Line::from)
            .collect();

    if !lines.is_empty() {
        lines.push(Line::from(Span::raw("")));
    }
    lines.push(footer_line(announcement, display_for));

    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(body, inner);
}

fn footer_line(a: &Announcement, display_for: &dyn Fn(&str) -> String) -> Line<'static> {
    let stamp =
        a.ts.with_timezone(&Local)
            .format("%-m/%-d %H:%M")
            .to_string();
    Line::from(vec![Span::styled(
        format!("— @{} {}", display_for(&a.actor_id), stamp),
        Style::default().fg(Color::DarkGray),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn fixture_announcement(text: &str) -> Announcement {
        Announcement {
            text: text.into(),
            actor_id: "actor_human_self".into(),
            ts: Utc::now(),
        }
    }

    #[test]
    fn render_draws_title_and_body() {
        let backend = TestBackend::new(PANEL_WIDTH, 6);
        let mut term = Terminal::new(backend).unwrap();
        let a = fixture_announcement("hello world");
        term.draw(|f| {
            let area = Rect {
                x: 0,
                y: 0,
                width: PANEL_WIDTH,
                height: 6,
            };
            render(
                f,
                &a,
                &|id| {
                    if id == "actor_human_self" {
                        "tester".into()
                    } else {
                        id.into()
                    }
                },
                area,
            );
        })
        .unwrap();
        let buf = term.backend().buffer();
        let mut joined = String::new();
        for y in 0..6 {
            for x in 0..PANEL_WIDTH {
                joined.push_str(&buf[(x, y)].symbol());
            }
            joined.push('\n');
        }
        assert!(
            joined.contains("Announcement"),
            "title missing in:\n{joined}"
        );
        assert!(joined.contains("hello world"), "body missing in:\n{joined}");
        assert!(joined.contains("@tester"), "footer missing in:\n{joined}");
    }
}
