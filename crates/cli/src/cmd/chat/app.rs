use std::collections::{HashMap, HashSet};

use proto::types::Event;

use super::history::History;
use super::picker::{Picker, PickerItem};
use super::prompt::PromptModal;
use super::sidebar::Sidebar;

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
    /// Pick an actor to invite into a specific channel. The picker is
    /// pre-filtered to actors that are NOT already members; the channel
    /// id is carried so the consumer can issue `channel/invite` once a
    /// row is chosen.
    InviteActor { channel_id: String },
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
    pub selected_history_idx: Option<usize>,
    pub scroll: u16,
    pub auto_follow: bool,
    pub should_quit: bool,
    pub status: String,
    /// Set once when the WebSocket reader closes; stops the notif select arm
    /// from spinning and re-pushing "server connection closed".
    pub disconnected: bool,
    /// Discord-style channel/thread browser. `None` when hidden (default).
    pub sidebar: Option<Sidebar>,
    /// Modal text input or confirm dialog (used by sidebar CRUD).
    pub prompt: Option<PromptModal>,
    /// Filled by the sidebar when the user picks a different thread; the
    /// chat main loop drains this and re-binds its scope subscription.
    pub pending_thread_switch: Option<String>,
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
            selected_history_idx: None,
            scroll: 0,
            auto_follow: true,
            should_quit: false,
            status: String::new(),
            disconnected: false,
            sidebar: None,
            prompt: None,
            pending_thread_switch: None,
        }
    }

    pub fn toggle_sidebar(&mut self) {
        if self.sidebar.is_some() {
            self.sidebar = None;
        } else {
            self.sidebar = Some(
                Sidebar::new(Some(self.thread_id.clone())).with_me(self.actor_id.clone()),
            );
        }
    }

    pub fn close_prompt(&mut self) {
        self.prompt = None;
    }

    /// Reset the visible message stream when the chat re-binds to a different
    /// thread. Scroll/auto-follow/reply-target/at-menu/slash-menu all become
    /// stale across threads; clear them in one place.
    pub fn reset_for_new_thread(&mut self, new_thread_id: String) {
        self.thread_id = new_thread_id;
        self.history = History::default();
        self.scroll = 0;
        self.auto_follow = true;
        self.reply_target = None;
        self.selected_history_idx = None;
        self.input.clear();
        self.slash_menu = None;
        self.at_menu = None;
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
        if self.auto_follow {
            self.selected_history_idx = self.history.newest_replyable_index();
        }
    }

    /// Rebuild the inline @-mention dropdown from the current input. Visible
    /// whenever input starts with `@`; filters by what comes after `@` and
    /// before the first space (so once the user types `@actor_x ` and starts
    /// the message body, the menu disappears and Enter sends a handoff).
    ///
    /// Non-members of the current private channel stay listed (so the
    /// auto-invite-then-handoff confirm flow remains reachable from `@`),
    /// but get a `(not in #foo)` hint so the operator isn't surprised by
    /// the confirm modal that pops up on Enter.
    pub fn update_at_menu(&mut self) {
        if let Some(rest) = self.input.strip_prefix('@') {
            let token = rest.split_whitespace().next().unwrap_or("");
            let still_typing_target = !rest.contains(' ');
            if !still_typing_target {
                self.at_menu = None;
                return;
            }
            let chan_ctx = self.current_channel_membership_ctx();
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
                    // Status (running/stopped) wins over the membership hint
                    // when both apply — agent runtime state is what the
                    // operator actually needs to see at a glance.
                    if let Some(status) = self.agent_statuses.get(id) {
                        it = it.with_status(status.clone());
                    } else if let Some((title, members)) = chan_ctx.as_ref() {
                        if !members.contains(id.as_str()) {
                            it = it.with_hint(format!("not in #{}", title));
                        }
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

    /// Resolve the current chat thread's owning channel via the sidebar
    /// cache, returning `(channel_title, member_actor_ids)` only when the
    /// channel is **private** — public channels need no membership hint
    /// since every actor can post.
    fn current_channel_membership_ctx(&self) -> Option<(String, std::collections::HashSet<String>)> {
        use proto::types::ChannelVisibility;
        let s = self.sidebar.as_ref()?;
        let channel_id = s.threads_by_channel.iter().find_map(|(ch, threads)| {
            threads
                .iter()
                .find(|t| t.id == self.thread_id)
                .map(|_| ch.clone())
        })?;
        let ch = s.channels.iter().find(|c| c.id == channel_id)?;
        if !matches!(ch.visibility, ChannelVisibility::Private) {
            return None;
        }
        // Prefer the resolved members_by_channel cache (richer); fall back to
        // Channel.members which always rides on the wire.
        let members: std::collections::HashSet<String> =
            if let Some(rows) = s.members_by_channel.get(&channel_id) {
                rows.iter().map(|r| r.actor_id.clone()).collect()
            } else {
                ch.members.iter().cloned().collect()
            };
        Some((ch.title.clone(), members))
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

    pub fn open_invite_picker(&mut self, channel_id: String, items: Vec<PickerItem>) {
        if items.is_empty() {
            self.set_status(
                "no actors to invite (every known actor is already a member or in `actor/list`)",
            );
            return;
        }
        let picker = Picker::new("Invite actor…", items);
        self.picker = Some(picker);
        self.mode = Mode::Picker(PickerKind::InviteActor { channel_id });
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

    pub fn clear_history_selection(&mut self) {
        self.selected_history_idx = None;
    }

    pub fn select_older_history(&mut self) {
        match self
            .history
            .older_replyable_index(self.selected_history_idx)
        {
            Some(idx) => {
                self.selected_history_idx = Some(idx);
                self.auto_follow = false;
            }
            None if self.selected_history_idx.is_none() => {
                self.set_status("no replyable events in this thread");
            }
            None => {}
        }
    }

    pub fn select_newer_history(&mut self) {
        match self.selected_history_idx {
            Some(current) => {
                if let Some(idx) = self.history.newer_replyable_index(current) {
                    self.selected_history_idx = Some(idx);
                } else {
                    self.selected_history_idx = None;
                    self.jump_to_bottom();
                }
            }
            None => self.jump_to_bottom(),
        }
    }

    pub fn selected_history_target(&self) -> Option<(String, String)> {
        let idx = self.selected_history_idx?;
        self.history
            .reply_target_at(idx, &|id| self.display_name_for(id))
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
        PickerItem::new("/invite", "Invite an actor into the current channel"),
        PickerItem::new("/members", "List members of the current channel"),
        PickerItem::new("/quit", "Leave the chat"),
    ]
}

#[cfg(test)]
mod tests {
    use super::App;
    use crate::cmd::chat::history::{Bubble, BubbleKind, DeliveryState};
    use chrono::Utc;

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
    fn at_menu_marks_non_members_of_private_channel_with_hint() {
        use crate::cmd::chat::sidebar::{MemberRow, Sidebar};
        use proto::types::{ActorKind, Channel, ChannelVisibility, Thread};

        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        // Two registered agents: alpha is a channel member, beta is not.
        app.agent_ids.insert("actor_agent_alpha".into());
        app.agent_ids.insert("actor_agent_beta".into());
        app.display_for
            .insert("actor_agent_alpha".into(), "Alpha".into());
        app.display_for
            .insert("actor_agent_beta".into(), "Beta".into());

        let mut sidebar = Sidebar::new(Some("thread_demo".into()));
        sidebar.replace_channels(vec![Channel {
            id: "ch_design".into(),
            title: "design".into(),
            visibility: ChannelVisibility::Private,
            members: vec!["actor_human_current".into(), "actor_agent_alpha".into()],
            _meta: None,
        }]);
        sidebar.replace_threads(
            "ch_design",
            vec![Thread {
                id: "thread_demo".into(),
                channel_id: "ch_design".into(),
                title: "demo".into(),
                root_event_id: None,
                _meta: None,
            }],
        );
        sidebar.replace_members(
            "ch_design",
            vec![MemberRow {
                actor_id: "actor_agent_alpha".into(),
                display: "Alpha".into(),
                kind: ActorKind::Agent,
            }],
        );
        app.sidebar = Some(sidebar);

        app.input = "@".into();
        app.update_at_menu();

        let picker = app.at_menu.expect("expected @-menu");
        let by_id: std::collections::HashMap<String, _> = picker
            .items
            .into_iter()
            .map(|it| (it.id.clone(), it))
            .collect();
        // Alpha is a member: no hint
        assert_eq!(by_id["actor_agent_alpha"].hint, None);
        // Beta is NOT a member: hint mentions the channel title
        assert_eq!(
            by_id["actor_agent_beta"].hint.as_deref(),
            Some("not in #design")
        );
    }

    #[test]
    fn at_menu_skips_membership_hint_for_public_channels() {
        use crate::cmd::chat::sidebar::Sidebar;
        use proto::types::{Channel, ChannelVisibility, Thread};

        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.agent_ids.insert("actor_agent_loner".into());
        app.display_for
            .insert("actor_agent_loner".into(), "Loner".into());

        let mut sidebar = Sidebar::new(Some("thread_demo".into()));
        sidebar.replace_channels(vec![Channel {
            id: "ch_lobby".into(),
            title: "lobby".into(),
            visibility: ChannelVisibility::Public,
            members: vec![],
            _meta: None,
        }]);
        sidebar.replace_threads(
            "ch_lobby",
            vec![Thread {
                id: "thread_demo".into(),
                channel_id: "ch_lobby".into(),
                title: "demo".into(),
                root_event_id: None,
                _meta: None,
            }],
        );
        app.sidebar = Some(sidebar);

        app.input = "@".into();
        app.update_at_menu();

        let picker = app.at_menu.expect("expected @-menu");
        // Public channel = no membership hint, even though loner is not in
        // Channel.members (the field is best-effort for public channels).
        assert_eq!(picker.items[0].hint, None);
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

    #[test]
    fn history_selection_moves_across_replyable_bubbles() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.history.push_system("system");
        app.history.bubbles.push(Bubble {
            actor_id: "actor_agent_alpha".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "older".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });
        app.history.bubbles.push(Bubble {
            actor_id: "system".into(),
            turn_id: None,
            kind: BubbleKind::System,
            text: "system".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: None,
            delivery: DeliveryState::NotApplicable,
        });
        app.history.bubbles.push(Bubble {
            actor_id: "actor_agent_beta".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "newer".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_2".into()),
            delivery: DeliveryState::NotApplicable,
        });

        app.select_older_history();
        assert_eq!(app.selected_history_idx, Some(3));

        app.select_older_history();
        assert_eq!(app.selected_history_idx, Some(1));

        app.select_newer_history();
        assert_eq!(app.selected_history_idx, Some(3));

        app.select_newer_history();
        assert_eq!(app.selected_history_idx, None);
        assert!(app.auto_follow);
    }
}
