use std::collections::{HashMap, HashSet};

use proto::types::Event;

use super::history::History;
use super::picker::{Picker, PickerItem};

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Picker(PickerKind),
}

#[derive(Debug, PartialEq, Eq)]
pub enum PickerKind {
    /// Pick a target actor for a handoff. Optional pending message.
    HandoffTarget,
    /// Pick a pending action.request to respond to.
    Action,
    /// Pick an existing event as the next reply target.
    Reply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub event_id: String,
    pub preview: String,
}

pub struct App {
    pub history: History,
    pub input: String,
    pub mode: Mode,
    pub picker: Option<Picker>,
    /// Inline slash-command dropdown above the input box. Visible whenever
    /// the input starts with `/`. Independent of `mode` (which is reserved
    /// for full-screen modal pickers).
    pub slash_menu: Option<Picker>,
    /// Inline @-mention dropdown above the input box. Visible whenever the
    /// input starts with `@`. Selecting an entry rewrites the leading
    /// `@<filter>` token to `@<actor_id> ` so the user can append a
    /// handoff message before pressing Enter.
    pub at_menu: Option<Picker>,
    pub actor_id: String,
    pub thread_id: String,
    pub display_for: HashMap<String, String>,
    /// Best-effort kind ("agent" / "human" / "service") per actor id; used
    /// only as a hint label in the @-mention picker.
    pub actor_kinds: HashMap<String, String>,
    /// Registered agents from `agent/list`. `@...` is a fast handoff shortcut
    /// and should not surface every historical human actor.
    pub agent_ids: HashSet<String>,
    /// Runtime status per registered agent, sourced from `agent/list`.
    pub agent_statuses: HashMap<String, String>,
    pub reply_target: Option<ReplyTarget>,
    pub scroll: u16,
    pub auto_follow: bool,
    pub should_quit: bool,
    pub status: String,
    /// Set once when the WebSocket reader closes; stops the notif select arm
    /// from spinning and re-pushing "server connection closed".
    pub disconnected: bool,
}

impl App {
    pub fn new(actor_id: String, thread_id: String, self_display: String) -> Self {
        let mut display_for = HashMap::new();
        display_for.insert(actor_id.clone(), self_display);
        display_for.insert("system".to_string(), "system".to_string());
        Self {
            history: History::default(),
            input: String::new(),
            mode: Mode::Normal,
            picker: None,
            slash_menu: None,
            at_menu: None,
            actor_id,
            thread_id,
            display_for,
            actor_kinds: HashMap::new(),
            agent_ids: HashSet::new(),
            agent_statuses: HashMap::new(),
            reply_target: None,
            scroll: 0,
            auto_follow: true,
            should_quit: false,
            status: String::new(),
            disconnected: false,
        }
    }

    pub fn display_name_for(&self, id: &str) -> String {
        let base = self
            .display_for
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        if base.is_empty() || id == "system" {
            return if base.is_empty() {
                id.to_string()
            } else {
                base
            };
        }

        let duplicate_count = self
            .display_for
            .iter()
            .filter(|(other_id, other_name)| {
                other_id.as_str() != "system" && other_name.as_str() == base.as_str()
            })
            .count();
        if duplicate_count <= 1 {
            return base;
        }

        if let Some(kind) = self.actor_kinds.get(id) {
            let same_kind_count = self
                .display_for
                .iter()
                .filter(|(other_id, other_name)| {
                    other_id.as_str() != "system"
                        && other_name.as_str() == base.as_str()
                        && self.actor_kinds.get(other_id.as_str()) == Some(kind)
                })
                .count();
            if same_kind_count == 1 {
                return format!("{} ({})", base, kind);
            }
        }

        format!("{} ({})", base, short_actor_ref(id))
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    pub fn ingest_event(&mut self, ev: &Event) {
        self.history.push_event(ev);
    }

    /// Rebuild the inline @-mention dropdown from the current input. Visible
    /// whenever input starts with `@`; filters by what comes after `@` and
    /// before the first space (so once the user types `@actor_x ` and starts
    /// the message body, the menu disappears and Enter sends a handoff).
    pub fn update_at_menu(&mut self) {
        if let Some(rest) = self.input.strip_prefix('@') {
            let token = rest.split_whitespace().next().unwrap_or("");
            let still_typing_target = !rest.contains(' ');
            if !still_typing_target {
                self.at_menu = None;
                return;
            }
            let mut items: Vec<PickerItem> = self
                .agent_ids
                .iter()
                .filter(|id| id.as_str() != self.actor_id && id.as_str() != "system")
                .map(|id| {
                    let display = self
                        .display_for
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| id.clone());
                    let mut it = PickerItem::new(id.clone(), display);
                    if let Some(status) = self.agent_statuses.get(id) {
                        it = it.with_status(status.clone());
                    }
                    it
                })
                .collect();
            items.sort_by(|a, b| a.id.cmp(&b.id));
            if items.is_empty() {
                self.at_menu = None;
                return;
            }
            let mut picker = Picker::new("@-mention", items);
            picker.filter = token.to_string();
            picker.clamp_selection();
            self.at_menu = Some(picker);
        } else {
            self.at_menu = None;
        }
    }

    /// Rebuild the slash-command dropdown from the current input. The menu
    /// is shown whenever input starts with `/`; it filters by what comes
    /// after the slash so e.g. `/ha` highlights `/handoff`.
    pub fn update_slash_menu(&mut self) {
        if let Some(rest) = self.input.strip_prefix('/') {
            // Only show while the user is still typing the command name —
            // once they've typed a space, treat the rest as args, not filter.
            let cmd_token = rest.split_whitespace().next().unwrap_or("");
            let is_typing_cmd = !rest.contains(' ');
            if !is_typing_cmd {
                self.slash_menu = None;
                return;
            }
            let items = slash_command_items();
            // Re-create the picker each time so the highlighted index resets
            // when the filter changes; cheap (4 items).
            let mut picker = Picker::new("Slash commands", items);
            picker.filter = cmd_token.to_string();
            picker.clamp_selection();
            self.slash_menu = Some(picker);
        } else {
            self.slash_menu = None;
        }
    }

    pub fn open_handoff_picker(&mut self, items: Vec<PickerItem>) {
        let picker = Picker::new("Hand off to…", items);
        self.picker = Some(picker);
        self.mode = Mode::Picker(PickerKind::HandoffTarget);
    }

    pub fn open_action_picker(&mut self) {
        let pending = self.history.pending_action_requests();
        if pending.is_empty() {
            self.set_status("no pending action.request in this thread");
            return;
        }
        let items = pending
            .into_iter()
            .map(|(id, label)| PickerItem::new(id, label))
            .collect();
        let picker = Picker::new("Respond to action.request", items);
        self.picker = Some(picker);
        self.mode = Mode::Picker(PickerKind::Action);
    }

    pub fn open_reply_picker(&mut self) {
        let items = self
            .history
            .reply_targets(&|id| self.display_name_for(id))
            .into_iter()
            .map(|(id, label)| PickerItem::new(id, label))
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.set_status("no replyable events in this thread");
            return;
        }
        let picker = Picker::new("Reply to…", items);
        self.picker = Some(picker);
        self.mode = Mode::Picker(PickerKind::Reply);
    }

    pub fn close_picker(&mut self) {
        self.picker = None;
        self.mode = Mode::Normal;
    }

    pub fn set_reply_target(&mut self, event_id: String, preview: String) {
        self.reply_target = Some(ReplyTarget {
            event_id: event_id.clone(),
            preview: preview.clone(),
        });
        self.set_status(format!("reply → {}", preview));
    }

    pub fn clear_reply_target(&mut self) {
        if self.reply_target.take().is_some() {
            self.set_status("reply cleared");
        }
    }

    pub fn scroll_up(&mut self, by: u16) {
        self.scroll = self.scroll.saturating_sub(by);
        self.auto_follow = false;
    }

    pub fn scroll_down(&mut self, by: u16) {
        self.scroll = self.scroll.saturating_add(by);
    }

    pub fn jump_to_top(&mut self) {
        self.scroll = 0;
        self.auto_follow = false;
    }

    pub fn jump_to_bottom(&mut self) {
        self.scroll = u16::MAX;
        self.auto_follow = true;
    }
}

fn short_actor_ref(id: &str) -> String {
    let compact = id.strip_prefix("actor_").unwrap_or(id);
    if compact.len() > 16 {
        format!("{}…", &compact[..16])
    } else {
        compact.to_string()
    }
}

pub fn slash_command_items() -> Vec<PickerItem> {
    vec![
        PickerItem::new("/handoff", "Hand off to an agent or human")
            .with_hint("offer turn to another actor"),
        PickerItem::new("/reply", "Reply to a previous event")
            .with_hint("sets replies_to for the next message"),
        PickerItem::new("/action", "Respond to a pending action.request"),
        PickerItem::new("/agents", "List active agents in the thread"),
        PickerItem::new("/quit", "Leave the chat"),
    ]
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn at_menu_only_lists_registered_agents() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.display_for
            .insert("actor_agent_opencode".into(), "OpenCode".into());
        app.actor_kinds
            .insert("actor_agent_opencode".into(), "agent".into());
        app.agent_ids.insert("actor_agent_opencode".into());

        app.display_for
            .insert("actor_human_old".into(), "bojun.cbj".into());
        app.actor_kinds
            .insert("actor_human_old".into(), "human".into());

        app.input = "@".into();
        app.update_at_menu();

        let picker = app.at_menu.expect("expected @-menu");
        let ids: Vec<String> = picker.items.into_iter().map(|it| it.id).collect();
        assert_eq!(ids, vec!["actor_agent_opencode"]);
    }

    #[test]
    fn duplicate_display_names_are_disambiguated_by_kind_when_possible() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "Alice".into(),
        );
        app.display_for
            .insert("actor_human_peer".into(), "Alice".into());
        app.display_for
            .insert("actor_agent_helper".into(), "Alice".into());
        app.actor_kinds
            .insert("actor_human_current".into(), "human".into());
        app.actor_kinds
            .insert("actor_human_peer".into(), "human".into());
        app.actor_kinds
            .insert("actor_agent_helper".into(), "agent".into());

        assert_eq!(app.display_name_for("actor_agent_helper"), "Alice (agent)");
    }

    #[test]
    fn duplicate_display_names_same_kind_fall_back_to_actor_ref() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "OpenCode".into(),
        );
        app.display_for
            .insert("actor_agent_alpha".into(), "OpenCode".into());
        app.display_for
            .insert("actor_agent_beta".into(), "OpenCode".into());
        app.actor_kinds
            .insert("actor_agent_alpha".into(), "agent".into());
        app.actor_kinds
            .insert("actor_agent_beta".into(), "agent".into());

        assert_eq!(
            app.display_name_for("actor_agent_alpha"),
            "OpenCode (agent_alpha)"
        );
        assert_eq!(
            app.display_name_for("actor_agent_beta"),
            "OpenCode (agent_beta)"
        );
    }
}
