mod app;
mod events;
mod history;
mod picker;
mod prompt;
mod sidebar;
mod ui;

use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::client::Client;

/// Launch the chat TUI. `thread_id` may be empty — that's the sentinel for
/// "user invoked `joi chat` without `--in`", in which case `events::run`
/// auto-opens the sidebar so the operator can pick or create a thread from
/// inside the UI instead of having to quit and re-run with `--in`.
pub async fn run(client: Arc<Client>, actor_id: String, thread_id: String) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = events::run(&mut terminal, client, actor_id, thread_id).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}
