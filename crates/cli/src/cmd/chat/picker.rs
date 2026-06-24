use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub status: Option<String>,
}

impl PickerItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            hint: None,
            status: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }
}

#[derive(Debug)]
pub struct Picker {
    pub title: String,
    pub items: Vec<PickerItem>,
    pub filter: String,
    pub state: ListState,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PickerOutcome {
    None,
    Selected(String),
    Cancelled,
}

impl Picker {
    pub fn new(title: impl Into<String>, items: Vec<PickerItem>) -> Self {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        Self {
            title: title.into(),
            items,
            filter: String::new(),
            state,
        }
    }

    pub fn filtered(&self) -> Vec<&PickerItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let needle = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|it| {
                    it.id.to_lowercase().contains(&needle)
                        || it.label.to_lowercase().contains(&needle)
                })
                .collect()
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PickerOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                PickerOutcome::Cancelled
            }
            (KeyCode::Enter, _) => {
                let filtered = self.filtered();
                if let Some(sel) = self.state.selected() {
                    if let Some(item) = filtered.get(sel) {
                        return PickerOutcome::Selected(item.id.clone());
                    }
                }
                PickerOutcome::None
            }
            (KeyCode::Up, _) => {
                self.move_sel(-1);
                PickerOutcome::None
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                self.move_sel(1);
                PickerOutcome::None
            }
            (KeyCode::Backspace, _) => {
                self.filter.pop();
                self.clamp_selection();
                PickerOutcome::None
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.clamp_selection();
                PickerOutcome::None
            }
            _ => PickerOutcome::None,
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        self.filter.push_str(text);
        self.clamp_selection();
    }

    fn move_sel(&mut self, delta: i32) {
        let len = self.filtered().len();
        if len == 0 {
            self.state.select(None);
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as i32;
        let mut next = cur + delta;
        if next < 0 {
            next = (len as i32) - 1;
        } else if next >= len as i32 {
            next = 0;
        }
        self.state.select(Some(next as usize));
    }

    pub fn clamp_selection(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.state.select(None);
        } else if self.state.selected().map(|s| s >= len).unwrap_or(true) {
            self.state.select(Some(0));
        }
    }

    /// Compact inline render (no filter row, no centering) for the
    /// always-visible slash-command dropdown above the input box.
    pub fn render_inline(&mut self, f: &mut Frame, area: Rect) {
        f.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));
        let items: Vec<ListItem> = self
            .filtered()
            .into_iter()
            .map(|it| {
                let mut spans = vec![
                    Span::styled(
                        format!("{:<10}", it.id),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                ];
                if let Some(status) = &it.status {
                    spans.push(Span::styled("●", Style::default().fg(status_color(status))));
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::raw(it.label.clone()));
                if let Some(status) = &it.status {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        format!("({})", status),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                if let Some(h) = &it.hint {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        format!("({})", h),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::Magenta)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.state);
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(70, 60, area);
        f.render_widget(Clear, popup);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(popup);

        let filter_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} (Esc to cancel) ", self.title))
            .border_style(Style::default().fg(Color::Magenta));
        let filter_text = Paragraph::new(format!("filter: {}_", self.filter)).block(filter_block);
        f.render_widget(filter_text, layout[0]);

        let items: Vec<ListItem> = self
            .filtered()
            .into_iter()
            .map(|it| {
                let mut spans = vec![
                    Span::styled(
                        format!("{:<24}", it.id),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                ];
                if let Some(status) = &it.status {
                    spans.push(Span::styled("●", Style::default().fg(status_color(status))));
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::raw(it.label.clone()));
                if let Some(status) = &it.status {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        format!("({})", status),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                if let Some(h) = &it.hint {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        format!("({})", h),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Magenta)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, layout[1], &mut self.state);
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
        width: popup_w,
        height: popup_h,
    }
}

fn status_color(status: &str) -> Color {
    if status.starts_with("⚠") {
        return Color::Yellow;
    }
    if status.contains("Thinking") {
        return Color::Magenta;
    }
    if status.contains("Waiting") {
        return Color::Rgb(255, 165, 0); // orange
    }
    if status.contains("Preparing") {
        return Color::Blue;
    }
    if status.contains("Queued") {
        return Color::Gray;
    }
    if status.contains("Failed") {
        return Color::Red;
    }
    if status.contains("Canceled") {
        return Color::DarkGray;
    }
    // Legacy fallback for old status strings
    match status {
        "running" => Color::Green,
        "idle" => Color::Yellow,
        "stopped" => Color::DarkGray,
        _ => Color::Cyan,
    }
}
