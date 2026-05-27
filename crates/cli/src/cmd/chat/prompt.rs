use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Discriminator for what an in-flight text prompt produces on submit. The
/// caller pairs this with the entered text to drive the right RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    CreateChannel,
    RenameChannel {
        channel_id: String,
    },
    CreateThread {
        channel_id: String,
    },
    RenameThread {
        thread_id: String,
    },
    /// Free-text actor id fallback when the picker doesn't have the actor
    /// cached (operator just registered an offline agent's spec, or just
    /// knows the id and doesn't want to scroll a picker).
    InviteToChannel {
        channel_id: String,
    },
    /// Danger-zone confirm for cascade channel delete: only Enter-submits
    /// when the typed value matches the channel title verbatim. The expected
    /// value is captured upstream so the modal can render a dim hint and
    /// guard the gate without consulting external state.
    CascadeDeleteChannel {
        channel_id: String,
        expected_title: String,
    },
}

/// Discriminator for confirm-style modals (delete + revoke + auto-invite).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    DeleteChannel {
        channel_id: String,
    },
    DeleteThread {
        thread_id: String,
    },
    /// Confirm revocation of an actor from a channel — mirrors the Delete
    /// confirms in spirit, but membership is cheap to restore so the
    /// guardrail is just a one-keystroke `y`.
    RevokeFromChannel {
        channel_id: String,
        actor_id: String,
        display: String,
    },
    /// `@target ...` to a non-member: confirm we should `channel/invite`
    /// them first, then send the message. The original message text is
    /// stashed here so the consumer doesn't have to re-derive it.
    InviteThenSend {
        channel_id: String,
        actor_id: String,
        message: String,
    },
}

#[derive(Debug)]
pub enum PromptModal {
    Text {
        kind: PromptKind,
        title: String,
        value: String,
    },
    Confirm {
        kind: ConfirmKind,
        title: String,
        message: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PromptOutcome {
    None,
    Cancelled,
    SubmittedText { kind: PromptKind, value: String },
    Confirmed(ConfirmKind),
}

impl PromptModal {
    pub fn text(kind: PromptKind, title: impl Into<String>, initial: impl Into<String>) -> Self {
        PromptModal::Text {
            kind,
            title: title.into(),
            value: initial.into(),
        }
    }

    pub fn confirm(
        kind: ConfirmKind,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        PromptModal::Confirm {
            kind,
            title: title.into(),
            message: message.into(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PromptOutcome {
        match self {
            PromptModal::Text { kind, value, .. } => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    PromptOutcome::Cancelled
                }
                (KeyCode::Enter, _) => {
                    let trimmed = value.trim().to_string();
                    // Danger-zone gate: cascade delete refuses to fire unless
                    // the typed value matches the channel title byte-for-byte.
                    // Stays in `None` (modal stays open) on mismatch so the
                    // operator can correct without re-opening anything.
                    if let PromptKind::CascadeDeleteChannel { expected_title, .. } = kind {
                        if &trimmed != expected_title {
                            return PromptOutcome::None;
                        }
                    } else if trimmed.is_empty() {
                        return PromptOutcome::None;
                    }
                    PromptOutcome::SubmittedText {
                        kind: kind.clone(),
                        value: trimmed,
                    }
                }
                (KeyCode::Backspace, _) => {
                    value.pop();
                    PromptOutcome::None
                }
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                    value.push(c);
                    PromptOutcome::None
                }
                _ => PromptOutcome::None,
            },
            PromptModal::Confirm { kind, .. } => match (key.code, key.modifiers) {
                (KeyCode::Esc, _)
                | (KeyCode::Char('c'), KeyModifiers::CONTROL)
                | (KeyCode::Char('n'), _)
                | (KeyCode::Char('N'), _) => PromptOutcome::Cancelled,
                (KeyCode::Enter, _) | (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) => {
                    PromptOutcome::Confirmed(kind.clone())
                }
                _ => PromptOutcome::None,
            },
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        if let PromptModal::Text { value, .. } = self {
            value.push_str(text);
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(60, 30, area);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(self.title_label());
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        match self {
            PromptModal::Text { kind, value, .. } => {
                let mut body_lines: Vec<Line<'static>> = Vec::new();
                // Cascade-delete prompt: show the expected channel name above
                // the input so the operator can copy it from the dialog itself
                // — no need to glance back at the sidebar.
                if let PromptKind::CascadeDeleteChannel { expected_title, .. } = kind {
                    body_lines.push(Line::from(vec![Span::styled(
                        format!("Type '{expected_title}' to confirm cascade delete:"),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )]));
                    body_lines.push(Line::from(Span::raw("")));
                }
                body_lines.push(Line::from(vec![
                    Span::styled("> ", Style::default().fg(Color::Cyan)),
                    Span::raw(value.clone()),
                    Span::styled("_", Style::default().fg(Color::Cyan)),
                ]));
                f.render_widget(Paragraph::new(body_lines), layout[0]);
                let hint = Line::from(vec![Span::styled(
                    " Enter=submit · Esc=cancel ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )]);
                f.render_widget(Paragraph::new(hint), layout[1]);
            }
            PromptModal::Confirm { message, .. } => {
                let body = Paragraph::new(Line::from(vec![Span::raw(message.clone())]));
                f.render_widget(body, layout[0]);
                let hint = Line::from(vec![Span::styled(
                    " y=confirm · n/Esc=cancel ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )]);
                f.render_widget(Paragraph::new(hint), layout[1]);
            }
        }
    }

    fn title_label(&self) -> String {
        let t = match self {
            PromptModal::Text { title, .. } => title,
            PromptModal::Confirm { title, .. } => title,
        };
        format!(" {} ", t)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_h = (r.height as u32 * percent_y as u32 / 100) as u16;
    let popup_w = (r.width as u32 * percent_x as u32 / 100) as u16;
    let y = r.y + (r.height.saturating_sub(popup_h)) / 2;
    let x = r.x + (r.width.saturating_sub(popup_w)) / 2;
    Rect {
        x,
        y,
        width: popup_w.max(20),
        height: popup_h.max(5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn text_prompt_submits_trimmed_value_on_enter() {
        let mut p = PromptModal::text(PromptKind::CreateChannel, "New channel", "");
        for c in "  hello  ".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        let outcome = p.handle_key(key(KeyCode::Enter));
        assert_eq!(
            outcome,
            PromptOutcome::SubmittedText {
                kind: PromptKind::CreateChannel,
                value: "hello".to_string(),
            }
        );
    }

    #[test]
    fn text_prompt_does_not_submit_blank_input() {
        let mut p = PromptModal::text(PromptKind::CreateChannel, "New channel", "   ");
        let outcome = p.handle_key(key(KeyCode::Enter));
        assert_eq!(outcome, PromptOutcome::None);
    }

    #[test]
    fn text_prompt_backspace_pops_one_char() {
        let mut p = PromptModal::text(PromptKind::CreateChannel, "New channel", "abc");
        p.handle_key(key(KeyCode::Backspace));
        if let PromptModal::Text { value, .. } = &p {
            assert_eq!(value, "ab");
        } else {
            panic!("expected text prompt");
        }
    }

    #[test]
    fn cascade_delete_prompt_only_submits_on_exact_match() {
        let kind = PromptKind::CascadeDeleteChannel {
            channel_id: "chan_x".into(),
            expected_title: "Engineering".into(),
        };
        let mut p = PromptModal::text(kind.clone(), "Type the name", "");
        // Wrong text: stays in None (modal stays open) on Enter.
        for c in "engineering".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(p.handle_key(key(KeyCode::Enter)), PromptOutcome::None);
        // Correct text submits with the typed value, kind preserved.
        let mut p = PromptModal::text(kind.clone(), "Type the name", "");
        for c in "Engineering".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            p.handle_key(key(KeyCode::Enter)),
            PromptOutcome::SubmittedText {
                kind,
                value: "Engineering".into(),
            }
        );
    }

    #[test]
    fn cascade_delete_prompt_esc_still_cancels() {
        let kind = PromptKind::CascadeDeleteChannel {
            channel_id: "chan_x".into(),
            expected_title: "Engineering".into(),
        };
        let mut p = PromptModal::text(kind, "Type the name", "");
        for c in "wrong".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(p.handle_key(key(KeyCode::Esc)), PromptOutcome::Cancelled);
    }

    #[test]
    fn confirm_prompt_y_confirms_n_cancels() {
        let mut p = PromptModal::confirm(
            ConfirmKind::DeleteThread {
                thread_id: "thread_x".into(),
            },
            "Delete thread?",
            "Are you sure?",
        );
        assert_eq!(
            p.handle_key(key(KeyCode::Char('y'))),
            PromptOutcome::Confirmed(ConfirmKind::DeleteThread {
                thread_id: "thread_x".into()
            })
        );
        // Re-init for second test.
        let mut p = PromptModal::confirm(
            ConfirmKind::DeleteThread {
                thread_id: "thread_x".into(),
            },
            "Delete thread?",
            "Are you sure?",
        );
        assert_eq!(
            p.handle_key(key(KeyCode::Char('n'))),
            PromptOutcome::Cancelled
        );
    }
}
