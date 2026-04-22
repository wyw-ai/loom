use std::collections::BTreeMap;

use proto::types::{Actor, ActorKind, Channel, ChannelVisibility, Thread};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarFocus {
    Channels,
    Threads,
    Members,
}

/// Render-friendly snapshot of a channel member. Built from `actor/list` +
/// `channel/members` so the pane can show a display name and an actor-kind
/// glyph without re-resolving on every redraw.
#[derive(Debug, Clone)]
pub struct MemberRow {
    pub actor_id: String,
    pub display: String,
    pub kind: ActorKind,
}

impl MemberRow {
    pub fn from_actor(a: &Actor) -> Self {
        let display = if a.display_name.is_empty() {
            a.id.clone()
        } else {
            a.display_name.clone()
        };
        Self {
            actor_id: a.id.clone(),
            display,
            kind: a.kind,
        }
    }
}

/// Discord-style left rail showing channels (top), threads of the focused
/// channel (middle), and the focused channel's members (bottom). Owns its
/// own selection cursors; the main App treats it as ambient state, not a
/// modal.
#[derive(Debug)]
pub struct Sidebar {
    pub channels: Vec<Channel>,
    pub threads_by_channel: BTreeMap<String, Vec<Thread>>,
    /// Cached member rows per channel id. The Members pane reads from this
    /// cache; events.rs is responsible for keeping it fresh via
    /// `replace_members` (initial / refresh) and `add_member`/`remove_member`
    /// (notification deltas).
    pub members_by_channel: BTreeMap<String, Vec<MemberRow>>,
    pub selected_channel_idx: usize,
    pub selected_thread_idx: usize,
    pub selected_member_idx: usize,
    pub focus: SidebarFocus,
    pub channels_state: ListState,
    pub threads_state: ListState,
    pub members_state: ListState,
    pub current_thread_id: Option<String>,
    /// When the chat is bound to a channel's common area (not a thread), this
    /// holds the bound channel id. Used to mark the bound channel row with a
    /// green dot in the Channels pane.
    pub current_channel_id: Option<String>,
    /// Caller's own actor id; used to mark "you" in the Members pane and to
    /// guard the local revoke shortcut against self-removal foot-guns.
    pub me_actor_id: Option<String>,
}

impl Sidebar {
    pub fn new(current_thread_id: Option<String>) -> Self {
        Self {
            channels: Vec::new(),
            threads_by_channel: BTreeMap::new(),
            members_by_channel: BTreeMap::new(),
            selected_channel_idx: 0,
            selected_thread_idx: 0,
            selected_member_idx: 0,
            focus: SidebarFocus::Channels,
            channels_state: ListState::default(),
            threads_state: ListState::default(),
            members_state: ListState::default(),
            current_thread_id,
            current_channel_id: None,
            me_actor_id: None,
        }
    }

    pub fn with_current_channel(mut self, channel_id: Option<String>) -> Self {
        self.current_channel_id = channel_id;
        self
    }

    pub fn with_me(mut self, me: String) -> Self {
        self.me_actor_id = Some(me);
        self
    }

    pub fn replace_channels(&mut self, mut channels: Vec<Channel>) {
        channels.sort_by(|a, b| a.title.cmp(&b.title));
        self.channels = channels;
        if self.selected_channel_idx >= self.channels.len() {
            self.selected_channel_idx = self.channels.len().saturating_sub(1);
        }
        self.sync_state();
    }

    /// Auto-select the channel containing the current thread (when known) so
    /// the operator opens the sidebar with their context already focused.
    pub fn focus_channel_of_current_thread(&mut self) {
        let Some(tid) = self.current_thread_id.clone() else {
            return;
        };
        // Find the channel id from the cached threads first; otherwise just
        // leave selection where it is.
        let mut owning_channel: Option<String> = None;
        for (ch_id, threads) in &self.threads_by_channel {
            if threads.iter().any(|t| t.id == tid) {
                owning_channel = Some(ch_id.clone());
                break;
            }
        }
        if let Some(ch_id) = owning_channel {
            if let Some(idx) = self.channels.iter().position(|c| c.id == ch_id) {
                self.selected_channel_idx = idx;
                if let Some(threads) = self.threads_by_channel.get(&ch_id) {
                    if let Some(t_idx) = threads.iter().position(|t| t.id == tid) {
                        self.selected_thread_idx = t_idx;
                    }
                }
                self.sync_state();
            }
        }
    }

    pub fn replace_threads(&mut self, channel_id: &str, mut threads: Vec<Thread>) {
        threads.sort_by(|a, b| a.title.cmp(&b.title));
        if let Some(current_ch) = self.selected_channel().map(|c| c.id.clone()) {
            if current_ch == channel_id && self.selected_thread_idx >= threads.len() {
                self.selected_thread_idx = threads.len().saturating_sub(1);
            }
        }
        self.threads_by_channel
            .insert(channel_id.to_string(), threads);
        self.sync_state();
    }

    pub fn add_channel(&mut self, ch: Channel) {
        self.channels.push(ch);
        self.channels.sort_by(|a, b| a.title.cmp(&b.title));
        self.sync_state();
    }

    pub fn remove_channel(&mut self, channel_id: &str) {
        self.channels.retain(|c| c.id != channel_id);
        self.threads_by_channel.remove(channel_id);
        if self.selected_channel_idx >= self.channels.len() {
            self.selected_channel_idx = self.channels.len().saturating_sub(1);
        }
        self.selected_thread_idx = 0;
        self.sync_state();
    }

    pub fn rename_channel(&mut self, ch: Channel) {
        for c in &mut self.channels {
            if c.id == ch.id {
                c.title = ch.title.clone();
            }
        }
        self.channels.sort_by(|a, b| a.title.cmp(&b.title));
        // Selection cursor may have moved after re-sort; keep it pointing at
        // the renamed channel.
        if let Some(idx) = self.channels.iter().position(|c| c.id == ch.id) {
            self.selected_channel_idx = idx;
        }
        self.sync_state();
    }

    pub fn add_thread(&mut self, t: Thread) {
        let entry = self
            .threads_by_channel
            .entry(t.channel_id.clone())
            .or_default();
        entry.push(t);
        entry.sort_by(|a, b| a.title.cmp(&b.title));
        self.sync_state();
    }

    pub fn remove_thread(&mut self, channel_id: &str, thread_id: &str) {
        if let Some(threads) = self.threads_by_channel.get_mut(channel_id) {
            threads.retain(|t| t.id != thread_id);
            if self.selected_thread_idx >= threads.len() {
                self.selected_thread_idx = threads.len().saturating_sub(1);
            }
        }
        self.sync_state();
    }

    pub fn replace_members(&mut self, channel_id: &str, mut members: Vec<MemberRow>) {
        members.sort_by(|a, b| a.display.to_lowercase().cmp(&b.display.to_lowercase()));
        if let Some(current_ch) = self.selected_channel().map(|c| c.id.clone()) {
            if current_ch == channel_id && self.selected_member_idx >= members.len() {
                self.selected_member_idx = members.len().saturating_sub(1);
            }
        }
        self.members_by_channel
            .insert(channel_id.to_string(), members);
        self.sync_state();
    }

    pub fn add_member(&mut self, channel_id: &str, row: MemberRow) {
        let entry = self
            .members_by_channel
            .entry(channel_id.to_string())
            .or_default();
        if entry.iter().any(|m| m.actor_id == row.actor_id) {
            return;
        }
        entry.push(row);
        entry.sort_by(|a, b| a.display.to_lowercase().cmp(&b.display.to_lowercase()));
        // Patch the Channel.members list so list/member-counts stay accurate
        // without an extra round-trip.
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel_id) {
            if !ch.members.iter().any(|m| {
                self.members_by_channel
                    .get(channel_id)
                    .map(|rows| rows.last().map(|r| r.actor_id.as_str()) == Some(m.as_str()))
                    .unwrap_or(false)
            }) {
                // Push the latest add_member's actor_id only if missing.
                let new_id = self
                    .members_by_channel
                    .get(channel_id)
                    .and_then(|rows| rows.last().map(|r| r.actor_id.clone()));
                if let Some(id) = new_id {
                    if !ch.members.contains(&id) {
                        ch.members.push(id);
                    }
                }
            }
        }
        self.sync_state();
    }

    pub fn remove_member(&mut self, channel_id: &str, actor_id: &str) {
        let current_ch = self.selected_channel().map(|c| c.id.clone());
        if let Some(rows) = self.members_by_channel.get_mut(channel_id) {
            rows.retain(|m| m.actor_id != actor_id);
            if current_ch.as_deref() == Some(channel_id) && self.selected_member_idx >= rows.len() {
                self.selected_member_idx = rows.len().saturating_sub(1);
            }
        }
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel_id) {
            ch.members.retain(|m| m != actor_id);
        }
        self.sync_state();
    }

    pub fn rename_thread(&mut self, t: Thread) {
        if let Some(threads) = self.threads_by_channel.get_mut(&t.channel_id) {
            for it in threads.iter_mut() {
                if it.id == t.id {
                    it.title = t.title.clone();
                }
            }
            threads.sort_by(|a, b| a.title.cmp(&b.title));
            if let Some(idx) = threads.iter().position(|x| x.id == t.id) {
                self.selected_thread_idx = idx;
            }
        }
        self.sync_state();
    }

    pub fn selected_channel(&self) -> Option<&Channel> {
        self.channels.get(self.selected_channel_idx)
    }

    pub fn selected_thread(&self) -> Option<&Thread> {
        let ch = self.selected_channel()?;
        self.threads_by_channel
            .get(&ch.id)?
            .get(self.selected_thread_idx)
    }

    pub fn current_threads(&self) -> Option<&Vec<Thread>> {
        let ch = self.selected_channel()?;
        self.threads_by_channel.get(&ch.id)
    }

    pub fn current_members(&self) -> Option<&Vec<MemberRow>> {
        let ch = self.selected_channel()?;
        self.members_by_channel.get(&ch.id)
    }

    pub fn selected_member(&self) -> Option<&MemberRow> {
        self.current_members()?.get(self.selected_member_idx)
    }

    pub fn move_up(&mut self) {
        match self.focus {
            SidebarFocus::Channels => {
                if !self.channels.is_empty() && self.selected_channel_idx > 0 {
                    self.selected_channel_idx -= 1;
                    self.selected_thread_idx = 0;
                    self.selected_member_idx = 0;
                }
            }
            SidebarFocus::Threads => {
                if self.selected_thread_idx > 0 {
                    self.selected_thread_idx -= 1;
                }
            }
            SidebarFocus::Members => {
                if self.selected_member_idx > 0 {
                    self.selected_member_idx -= 1;
                }
            }
        }
        self.sync_state();
    }

    pub fn move_down(&mut self) {
        match self.focus {
            SidebarFocus::Channels => {
                if !self.channels.is_empty() && self.selected_channel_idx + 1 < self.channels.len()
                {
                    self.selected_channel_idx += 1;
                    self.selected_thread_idx = 0;
                    self.selected_member_idx = 0;
                }
            }
            SidebarFocus::Threads => {
                if let Some(threads) = self.current_threads() {
                    if self.selected_thread_idx + 1 < threads.len() {
                        self.selected_thread_idx += 1;
                    }
                }
            }
            SidebarFocus::Members => {
                if let Some(members) = self.current_members() {
                    if self.selected_member_idx + 1 < members.len() {
                        self.selected_member_idx += 1;
                    }
                }
            }
        }
        self.sync_state();
    }

    pub fn next_pane(&mut self) {
        self.focus = match self.focus {
            SidebarFocus::Channels => SidebarFocus::Threads,
            SidebarFocus::Threads => SidebarFocus::Members,
            SidebarFocus::Members => SidebarFocus::Channels,
        };
    }

    pub fn sync_state(&mut self) {
        if self.channels.is_empty() {
            self.channels_state.select(None);
        } else {
            self.channels_state.select(Some(self.selected_channel_idx));
        }
        let thread_len = self
            .selected_channel()
            .and_then(|ch| self.threads_by_channel.get(&ch.id))
            .map(|v| v.len())
            .unwrap_or(0);
        if thread_len == 0 {
            self.threads_state.select(None);
        } else {
            self.threads_state.select(Some(self.selected_thread_idx));
        }
        let member_len = self
            .selected_channel()
            .and_then(|ch| self.members_by_channel.get(&ch.id))
            .map(|v| v.len())
            .unwrap_or(0);
        if member_len == 0 {
            self.members_state.select(None);
        } else {
            self.members_state.select(Some(self.selected_member_idx));
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Browser  (Tab=switch · Ctrl+B=hide) ");
        let inner = block.inner(area);
        f.render_widget(block, area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(35),
                Constraint::Min(3),
            ])
            .split(inner);
        self.render_channels(f, layout[0]);
        self.render_threads(f, layout[1]);
        self.render_members(f, layout[2]);
    }

    fn render_channels(&mut self, f: &mut Frame, area: Rect) {
        let focused = matches!(self.focus, SidebarFocus::Channels);
        let title = if focused {
            " ▸ Channels  [Enter=threads · c=enter · n/r/d] "
        } else {
            "   Channels "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style(focused));
        if self.channels.is_empty() {
            let para = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                "(no channels — press n)",
                Style::default().fg(Color::DarkGray),
            )))
            .block(block);
            f.render_widget(para, area);
            return;
        }
        let items: Vec<ListItem> = self
            .channels
            .iter()
            .map(|c| {
                let owning = self
                    .current_thread_id
                    .as_ref()
                    .and_then(|tid| owning_channel_id(self, tid));
                let bound_via_thread = owning.as_deref() == Some(c.id.as_str());
                let bound_via_channel = self.current_channel_id.as_deref() == Some(c.id.as_str());
                let mark = if bound_via_thread || bound_via_channel {
                    "●"
                } else {
                    " "
                };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::default().fg(Color::Green)),
                    Span::raw(" #"),
                    Span::raw(c.title.clone()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(highlight_style(focused))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.channels_state);
    }

    fn render_threads(&mut self, f: &mut Frame, area: Rect) {
        let focused = matches!(self.focus, SidebarFocus::Threads);
        let title = if focused {
            " ▸ Threads  [Enter=open · n/r/d] "
        } else {
            "   Threads "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style(focused));
        let threads = self.current_threads().cloned().unwrap_or_default();
        if threads.is_empty() {
            let msg = if self.channels.is_empty() {
                "(no channel selected)"
            } else {
                "(no threads — press n)"
            };
            let para = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(Color::DarkGray),
            )))
            .block(block);
            f.render_widget(para, area);
            return;
        }
        let current = self.current_thread_id.clone();
        let items: Vec<ListItem> = threads
            .iter()
            .map(|t| {
                let is_current = current.as_deref() == Some(t.id.as_str());
                let prefix = if is_current { "●" } else { " " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Green)),
                    Span::raw(" "),
                    Span::raw(t.title.clone()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(highlight_style(focused))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.threads_state);
    }

    fn render_members(&mut self, f: &mut Frame, area: Rect) {
        let focused = matches!(self.focus, SidebarFocus::Members);
        let title = if focused {
            " ▸ Members  [i=invite · I=by-id · x=remove] "
        } else {
            "   Members "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style(focused));
        let visibility = self.selected_channel().map(|c| c.visibility);
        let members = self.current_members().cloned().unwrap_or_default();
        if members.is_empty() {
            let msg = if self.channels.is_empty() {
                "(no channel selected)"
            } else {
                match visibility {
                    Some(ChannelVisibility::Public) => "(public — no members tracked)",
                    Some(ChannelVisibility::Private) | None => "(empty — press i to invite)",
                }
            };
            let para = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(Color::DarkGray),
            )))
            .block(block);
            f.render_widget(para, area);
            return;
        }
        let me = self.me_actor_id.clone();
        let items: Vec<ListItem> = members
            .iter()
            .map(|m| {
                let is_me = me.as_deref() == Some(m.actor_id.as_str());
                let prefix = if is_me { "●" } else { " " };
                let (kind_glyph, kind_style) = match m.kind {
                    ActorKind::Agent => ("@", Style::default().fg(Color::Cyan)),
                    ActorKind::Service => ("$", Style::default().fg(Color::Yellow)),
                    ActorKind::Human => ("", Style::default().fg(Color::White)),
                };
                let mut spans = vec![
                    Span::styled(prefix, Style::default().fg(Color::Green)),
                    Span::raw(" "),
                ];
                if !kind_glyph.is_empty() {
                    spans.push(Span::styled(kind_glyph, kind_style));
                }
                spans.push(Span::raw(m.display.clone()));
                if is_me {
                    spans.push(Span::styled(
                        "  (you)",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(highlight_style(focused))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.members_state);
    }
}

fn owning_channel_id(sidebar: &Sidebar, thread_id: &str) -> Option<String> {
    sidebar.threads_by_channel.iter().find_map(|(ch, threads)| {
        threads
            .iter()
            .find(|t| t.id == thread_id)
            .map(|_| ch.clone())
    })
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn highlight_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(Color::Magenta)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::types::{Channel, Thread};

    fn ch(id: &str, title: &str) -> Channel {
        Channel {
            id: id.into(),
            title: title.into(),
            visibility: ChannelVisibility::Public,
            members: Vec::new(),
            _meta: None,
        }
    }
    fn th(id: &str, channel_id: &str, title: &str) -> Thread {
        Thread {
            id: id.into(),
            channel_id: channel_id.into(),
            title: title.into(),
            root_event_id: None,
            _meta: None,
        }
    }
    fn mr(actor_id: &str, display: &str, kind: ActorKind) -> MemberRow {
        MemberRow {
            actor_id: actor_id.into(),
            display: display.into(),
            kind,
        }
    }

    #[test]
    fn move_within_pane_respects_bounds_and_resets_thread_idx_on_channel_change() {
        let mut s = Sidebar::new(None);
        s.replace_channels(vec![ch("c1", "alpha"), ch("c2", "beta")]);
        s.replace_threads("c1", vec![th("t1", "c1", "x"), th("t2", "c1", "y")]);
        s.replace_threads("c2", vec![th("t3", "c2", "z")]);

        // start at channels[0] / threads[0]
        assert_eq!(s.selected_channel().unwrap().id, "c1");
        assert_eq!(s.selected_thread_idx, 0);

        // move down on threads pane
        s.next_pane();
        s.move_down();
        assert_eq!(s.selected_thread_idx, 1);
        // bound: another move_down stays at 1 (only 2 threads)
        s.move_down();
        assert_eq!(s.selected_thread_idx, 1);

        // back to channels and switch to c2 — thread idx must reset.
        // next_pane cycles Threads → Members → Channels.
        s.next_pane();
        s.next_pane();
        s.move_down();
        assert_eq!(s.selected_channel().unwrap().id, "c2");
        assert_eq!(s.selected_thread_idx, 0);
    }

    #[test]
    fn add_remove_thread_keeps_selection_in_range() {
        let mut s = Sidebar::new(None);
        s.replace_channels(vec![ch("c1", "alpha")]);
        s.replace_threads("c1", vec![th("t1", "c1", "x"), th("t2", "c1", "y")]);
        s.next_pane();
        s.move_down();
        assert_eq!(s.selected_thread_idx, 1);

        s.remove_thread("c1", "t2");
        // selection moves up to the still-existing item
        assert_eq!(s.selected_thread_idx, 0);
    }

    #[test]
    fn rename_channel_keeps_cursor_on_renamed_item() {
        let mut s = Sidebar::new(None);
        s.replace_channels(vec![ch("c1", "alpha"), ch("c2", "zeta")]);
        s.move_down();
        assert_eq!(s.selected_channel().unwrap().id, "c2");

        s.rename_channel(ch("c2", "aardvark"));
        assert_eq!(s.selected_channel().unwrap().id, "c2");
        assert_eq!(s.selected_channel().unwrap().title, "aardvark");
        // re-sorted: aardvark, alpha
        assert_eq!(s.selected_channel_idx, 0);
    }

    #[test]
    fn next_pane_cycles_channels_threads_members() {
        let mut s = Sidebar::new(None);
        assert_eq!(s.focus, SidebarFocus::Channels);
        s.next_pane();
        assert_eq!(s.focus, SidebarFocus::Threads);
        s.next_pane();
        assert_eq!(s.focus, SidebarFocus::Members);
        s.next_pane();
        assert_eq!(s.focus, SidebarFocus::Channels);
    }

    #[test]
    fn members_pane_navigation_and_replace_keep_cursor_in_range() {
        let mut s = Sidebar::new(None);
        s.replace_channels(vec![ch("c1", "alpha")]);
        s.replace_members(
            "c1",
            vec![
                mr("a_alice", "alice", ActorKind::Human),
                mr("a_bob", "bob", ActorKind::Human),
                mr("a_zoe", "zoe", ActorKind::Agent),
            ],
        );
        // jump to Members pane
        s.next_pane();
        s.next_pane();
        assert_eq!(s.focus, SidebarFocus::Members);
        s.move_down();
        s.move_down();
        assert_eq!(s.selected_member_idx, 2);
        // bound: another move_down stays at 2 (only 3 members)
        s.move_down();
        assert_eq!(s.selected_member_idx, 2);
        // remove the selected one — cursor must collapse to last in range
        s.remove_member("c1", "a_zoe");
        assert_eq!(s.selected_member_idx, 1);
        assert_eq!(
            s.selected_member().map(|m| m.actor_id.as_str()),
            Some("a_bob")
        );
        // adding back doesn't move the cursor
        s.add_member("c1", mr("a_zoe", "zoe", ActorKind::Agent));
        assert_eq!(s.selected_member_idx, 1);
    }

    #[test]
    fn focus_channel_of_current_thread_picks_owning_channel() {
        let mut s = Sidebar::new(Some("t3".into()));
        s.replace_channels(vec![ch("c1", "alpha"), ch("c2", "beta")]);
        s.replace_threads("c1", vec![th("t1", "c1", "x")]);
        s.replace_threads("c2", vec![th("t2", "c2", "y"), th("t3", "c2", "z")]);
        s.focus_channel_of_current_thread();
        assert_eq!(s.selected_channel().unwrap().id, "c2");
        assert_eq!(s.selected_thread().unwrap().id, "t3");
    }
}
