use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use proto::types::{Message, RunStatus, ScopeKind, ScopeRef};

use super::draft::DraftInput;
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
    /// Pick a target actor for a directed message.
    DirectTarget,
    /// Pick a pending action.request to respond to.
    Action,
    /// Pick an existing message as the next reply target.
    Reply,
    /// Pick an actor to invite into a specific channel. The picker is
    /// pre-filtered to actors that are NOT already members; the channel
    /// id is carried so the consumer can issue `channel/invite` once a
    /// row is chosen.
    InviteActor { channel_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub message_id: String,
    pub preview: String,
}

/// Light snapshot of an open agent run the client has observed via
/// `run.updated` and not yet seen in a terminal status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTurn {
    pub turn_id: String,
    pub actor_id: String,
    pub scope: ScopeRef,
    pub opened_at: DateTime<Utc>,
}

// ── Agent status display (V4) ──────────────────────────────────────────

/// Snapshot of a run received via `run.updated`, for agent status aggregation.
/// Separate from `OpenTurn` (which serves cancel / in-flight bar) so terminal
/// runs can be retained briefly for display without affecting cancel behaviour.
#[derive(Debug, Clone)]
pub struct CachedRun {
    pub actor_id: String,
    pub status: RunStatus,
    pub start_reason: Option<String>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    /// `meta.toolName` — used for the WaitingTool label.
    pub tool_name: Option<String>,
    /// `meta.error` — used for the Failed label.
    pub error: Option<String>,
    /// `meta.noReplyReason` — fallback for the Failed label.
    pub no_reply_reason: Option<String>,
}

/// Computed agent status info for the Members pane and @-mention picker.
#[derive(Debug, Clone)]
pub struct AgentStatusInfo {
    pub label: String,             // "Thinking · user mention"
    pub status: Option<RunStatus>, // for color dot
    pub is_stale: bool,            // timed out
}

// ── Timeout thresholds (seconds) per run status ────────────────────────
const TIMEOUT_QUEUED_SECS: i64 = 300;
const TIMEOUT_PREPARING_CONTEXT_SECS: i64 = 180;
const TIMEOUT_RUNNING_SECS: i64 = 600;
const TIMEOUT_WAITING_TOOL_SECS: i64 = 120;
/// Terminal runs linger in agent_statuses for this long after closing.
const TTL_TERMINAL_SECS: i64 = 30;

pub struct App {
    pub history: History,
    pub input: DraftInput,
    pub mode: Mode,
    pub picker: Option<Picker>,
    /// Inline slash-command dropdown above the input box. Visible whenever
    /// the input starts with `/`. Independent of `mode` (which is reserved
    /// for full-screen modal pickers).
    pub slash_menu: Option<Picker>,
    /// Inline @-mention dropdown above the input box. Visible whenever the
    /// input starts with `@`. Selecting an entry rewrites the leading
    /// `@<filter>` token to `@<actor_id> ` so the user can append a
    /// directed message before pressing Enter.
    pub at_menu: Option<Picker>,
    pub actor_id: String,
    /// Id of the currently bound scope. Empty = no scope bound (launched
    /// `loom chat` without `--in`/`--channel`). When non-empty, `scope_kind`
    /// disambiguates whether this is a thread or a channel scope.
    pub thread_id: String,
    /// Kind of the currently bound scope. `Thread` by default for
    /// back-compat with `--in <thread_id>`; flipped to `Channel` when the
    /// operator binds the chat to a channel's common area.
    pub scope_kind: ScopeKind,
    pub display_for: HashMap<String, String>,
    /// Best-effort kind ("agent" / "human" / "service") per actor id; used
    /// only as a hint label in the @-mention picker.
    pub actor_kinds: HashMap<String, String>,
    /// Agent actors from `actor/list`. `@...` is a fast directed-send shortcut and
    /// should not surface every historical human actor.
    pub agent_ids: HashSet<String>,
    /// Runtime status per agent, computed from `run_cache` via
    /// `recompute_agent_statuses()`. Members pane and @-mention picker
    /// consume this map.
    pub agent_statuses: HashMap<String, AgentStatusInfo>,
    /// Raw run snapshots keyed by run_id. Populated by `apply_run_updated`;
    /// consumed by `recompute_agent_statuses()`. Terminal runs age out
    /// naturally during recomputation (TTL check).
    pub run_cache: HashMap<String, CachedRun>,
    pub reply_target: Option<ReplyTarget>,
    /// Runs the server has told us are open and not yet terminal. Keyed by run
    /// id; populated from `run.updated` notifications.
    /// Used by Esc-cancel and the in-flight status bar.
    pub open_turns: HashMap<String, OpenTurn>,
    pub selected_history_idx: Option<usize>,
    pub expanded_history: HashSet<usize>,
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
    /// Filled by the sidebar when the user picks a different scope
    /// (thread OR channel common area); the chat main loop drains this
    /// and re-binds its scope subscription.
    pub pending_scope_switch: Option<ScopeRef>,
    pub next_paste_token_id: u64,
}

impl App {
    pub fn new(
        actor_id: String,
        scope_id: String,
        scope_kind: ScopeKind,
        self_display: String,
    ) -> Self {
        let mut display_for = HashMap::new();
        display_for.insert(actor_id.clone(), self_display);
        display_for.insert("system".to_string(), "system".to_string());
        Self {
            history: History::default(),
            input: DraftInput::default(),
            mode: Mode::Normal,
            picker: None,
            slash_menu: None,
            at_menu: None,
            actor_id,
            thread_id: scope_id,
            scope_kind,
            display_for,
            actor_kinds: HashMap::new(),
            agent_ids: HashSet::new(),
            agent_statuses: HashMap::new(),
            run_cache: HashMap::new(),
            reply_target: None,
            open_turns: HashMap::new(),
            selected_history_idx: None,
            expanded_history: HashSet::new(),
            scroll: 0,
            auto_follow: true,
            should_quit: false,
            status: String::new(),
            disconnected: false,
            sidebar: None,
            prompt: None,
            pending_scope_switch: None,
            next_paste_token_id: 1,
        }
    }

    /// `true` when any scope (thread OR channel) is bound.
    pub fn has_scope(&self) -> bool {
        !self.thread_id.is_empty()
    }

    /// `true` when the bound scope is specifically a thread. Several
    /// sidebar/prompt helpers only make sense inside a thread; this gate
    /// keeps channel-common-area chat out of their control flow.
    pub fn has_thread(&self) -> bool {
        self.has_scope() && matches!(self.scope_kind, ScopeKind::Thread)
    }

    /// Snapshot the current scope, if any.
    pub fn current_scope(&self) -> Option<ScopeRef> {
        if !self.has_scope() {
            return None;
        }
        Some(ScopeRef {
            kind: self.scope_kind.clone(),
            id: self.thread_id.clone(),
        })
    }

    pub fn toggle_sidebar(&mut self) {
        if self.sidebar.is_some() {
            self.sidebar = None;
        } else {
            // Pass `None` when no thread is bound — the sidebar uses this
            // to skip the "auto-focus the owning channel" logic and just
            // lands on the first channel in the list. Channel-bound mode
            // feeds `current_channel_id` so the Channels pane still marks
            // the bound channel with a green dot.
            let current_thread = if self.has_thread() {
                Some(self.thread_id.clone())
            } else {
                None
            };
            let current_channel =
                if matches!(self.scope_kind, ScopeKind::Channel) && self.has_scope() {
                    Some(self.thread_id.clone())
                } else {
                    None
                };
            self.sidebar = Some(
                Sidebar::new(current_thread)
                    .with_me(self.actor_id.clone())
                    .with_current_channel(current_channel),
            );
        }
    }

    pub fn close_prompt(&mut self) {
        self.prompt = None;
    }

    /// Reset the visible message stream when the chat re-binds to a different
    /// scope (thread or channel). Scroll/auto-follow/reply-target/at-menu/
    /// slash-menu all become stale across scopes; clear them in one place.
    pub fn reset_for_new_scope(&mut self, new_scope: &ScopeRef) {
        self.thread_id = new_scope.id.clone();
        self.scope_kind = new_scope.kind.clone();
        self.history = History::default();
        self.scroll = 0;
        self.auto_follow = true;
        self.reply_target = None;
        // Drop open-turn entries that don't belong to the new scope. We keep
        // the rest because cancel cares about scope-bound turns; an entry
        // for a different scope would still be valid but never actionable
        // from this view, so dropping is fine and avoids unbounded growth
        // for operators that bounce between many scopes.
        self.open_turns.retain(|_, t| &t.scope == new_scope);
        self.selected_history_idx = None;
        self.expanded_history.clear();
        self.input.clear();
        self.slash_menu = None;
        self.at_menu = None;
    }

    /// Open turns currently registered for `scope`, sorted by open time so the
    /// most recent is last.
    pub fn open_turns_in_scope(&self, scope: &ScopeRef) -> Vec<&OpenTurn> {
        let mut v: Vec<&OpenTurn> = self
            .open_turns
            .values()
            .filter(|t| &t.scope == scope)
            .collect();
        v.sort_by_key(|t| t.opened_at);
        v
    }

    // ── Agent status computation (V4) ──────────────────────────────────

    /// Rebuild `agent_statuses` from `run_cache`. Groups runs by actor,
    /// picks the highest-priority non-terminal run for each, computes a
    /// status label, and writes into `agent_statuses`. Terminal runs
    /// linger for `TTL_TERMINAL_SECS` after closing so the user can see
    /// "Failed" / "Canceled" briefly.
    pub fn recompute_agent_statuses(&mut self) {
        use std::collections::HashMap;
        let now = Utc::now();

        // 1. Prune expired terminal runs from run_cache.
        self.run_cache.retain(|_id, cr| {
            if let Some(closed) = cr.closed_at {
                (now - closed).num_seconds() < TTL_TERMINAL_SECS
            } else {
                true // still active
            }
        });

        // 2. Group active (or recently terminal) runs by actor.
        let mut by_actor: HashMap<String, Vec<&CachedRun>> = HashMap::new();
        for cr in self.run_cache.values() {
            by_actor.entry(cr.actor_id.clone()).or_default().push(cr);
        }

        // 3. Clear and recompute.
        self.agent_statuses.clear();
        for (actor_id, runs) in &by_actor {
            // Pick the highest-priority run.
            let Some(best) = runs.iter().max_by_key(|r| run_priority(r.status)) else {
                continue;
            };

            let elapsed = (now - best.opened_at).num_seconds();
            let timeout = timeout_for(best.status);
            let is_stale = timeout > 0 && elapsed > timeout;

            let label = compute_run_status_label(best, is_stale);
            self.agent_statuses.insert(
                actor_id.clone(),
                AgentStatusInfo {
                    label,
                    status: Some(best.status),
                    is_stale,
                },
            );
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

    pub fn ingest_message(&mut self, message: &Message) {
        self.history.push_message(message);
        if self.auto_follow {
            self.selected_history_idx = self.history.newest_replyable_index();
        }
    }

    /// Rebuild the inline @-mention dropdown from the current input. Visible
    /// whenever input starts with `@`; filters by what comes after `@` and
    /// before the first space (so once the user types `@actor_x ` and starts
    /// the message body, the menu disappears and Enter sends a directed message).
    ///
    /// In a private channel the picker is restricted to actors the operator
    /// can actually message without first inviting: members of the
    /// current channel ∪ "public area" actors (today: union of all Public channel
    /// memberships). Non-members are deliberately *not* listed — explicit
    /// invitation goes through the dedicated invite picker.
    /// TODO: once the dedicated lobby channel concept lands (in-flight on
    /// another branch), narrow "public area" from "any Public channel" to that
    /// single lobby channel.
    pub fn update_at_menu(&mut self) {
        let input = self.input.display_text();
        if let Some(rest) = input.strip_prefix('@') {
            let token = rest.split_whitespace().next().unwrap_or("");
            let still_typing_target = !rest.contains(' ');
            if !still_typing_target {
                self.at_menu = None;
                return;
            }
            let chan_ctx = self.current_channel_membership_ctx();
            let public_actors = self.publicly_addressable_actors();
            let mut items: Vec<PickerItem> = self
                .agent_ids
                .iter()
                .filter(|id| id.as_str() != self.actor_id && id.as_str() != "system")
                .filter(|id| match chan_ctx.as_ref() {
                    // Public channel: every known agent is addressable.
                    None => true,
                    // Private channel: keep the current channel's members
                    // and anyone reachable via the public area.
                    Some((_, members)) => {
                        members.contains(id.as_str()) || public_actors.contains(id.as_str())
                    }
                })
                .map(|id| {
                    let display = self
                        .display_for
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| id.clone());
                    let mut it = PickerItem::new(id.clone(), display);
                    if let Some(info) = self.agent_statuses.get(id) {
                        it = it.with_status(info.label.clone());
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

    /// Union of member sets across all Public channels currently visible in
    /// the sidebar — actors the operator can address from anywhere because
    /// they live in the shared "public area". Empty when there's no sidebar yet
    /// (first frame after launch); the caller's filter falls back to "no
    /// public actors" which is the safe default.
    fn publicly_addressable_actors(&self) -> std::collections::HashSet<String> {
        use proto::types::ChannelVisibility;
        let mut out = std::collections::HashSet::new();
        let Some(s) = self.sidebar.as_ref() else {
            return out;
        };
        for ch in &s.channels {
            if !matches!(ch.visibility, ChannelVisibility::Public) {
                continue;
            }
            // Prefer the resolved members_by_channel cache when populated;
            // fall back to Channel.members which always rides on the wire.
            if let Some(rows) = s.members_by_channel.get(&ch.id) {
                for r in rows {
                    out.insert(r.actor_id.clone());
                }
            } else {
                for m in &ch.members {
                    out.insert(m.clone());
                }
            }
        }
        out
    }

    /// Resolve the current chat thread's owning channel via the sidebar
    /// cache, returning `(channel_title, member_actor_ids)` only when the
    /// channel is **private** — public channels need no membership hint
    /// since every actor can post.
    fn current_channel_membership_ctx(
        &self,
    ) -> Option<(String, std::collections::HashSet<String>)> {
        use proto::types::ChannelVisibility;
        let s = self.sidebar.as_ref()?;
        // Channel-bound: the scope id IS the channel id.
        let channel_id = if matches!(self.scope_kind, ScopeKind::Channel) && self.has_scope() {
            self.thread_id.clone()
        } else {
            s.threads_by_channel.iter().find_map(|(ch, threads)| {
                threads
                    .iter()
                    .find(|t| t.id == self.thread_id)
                    .map(|_| ch.clone())
            })?
        };
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
    /// after the slash so e.g. `/as` highlights `/ask`.
    pub fn update_slash_menu(&mut self) {
        let input = self.input.display_text();
        if let Some(rest) = input.strip_prefix('/') {
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

    pub fn open_direct_picker(&mut self, items: Vec<PickerItem>) {
        let picker = Picker::new("Send to…", items);
        self.picker = Some(picker);
        self.mode = Mode::Picker(PickerKind::DirectTarget);
    }

    /// Number of `action.request` events the local actor still owes a
    /// response to. Sourced from `History::pending_action_requests`; surfaced
    /// in the status bar so the operator always sees there's something to
    /// resolve, even when scrolled away from the bubble.
    pub fn pending_action_count(&self) -> usize {
        self.history.pending_action_requests().len()
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
            self.set_status("no replyable messages in this thread");
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

    pub fn set_reply_target(&mut self, message_id: String, preview: String) {
        self.reply_target = Some(ReplyTarget {
            message_id: message_id.clone(),
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

    pub fn expand_selected_history(&mut self) {
        let Some(idx) = self.selected_history_idx else {
            return;
        };
        if self.history.bubble_is_collapsible(idx) {
            self.expanded_history.insert(idx);
        }
    }

    pub fn collapse_selected_history(&mut self) {
        let Some(idx) = self.selected_history_idx else {
            return;
        };
        self.expanded_history.remove(&idx);
    }

    pub fn next_paste_token_id(&mut self) -> u64 {
        let id = self.next_paste_token_id;
        self.next_paste_token_id = self.next_paste_token_id.saturating_add(1);
        id
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
                self.set_status("no replyable messages in this thread");
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
        PickerItem::new("/ask", "Send to an agent or human")
            .with_hint("direct the next message to another actor"),
        PickerItem::new("/reply", "Reply to a previous message")
            .with_hint("sets the parent message for the next send"),
        PickerItem::new("/action", "Respond to a pending action.request"),
        PickerItem::new("/agents", "List active agents in the thread"),
        PickerItem::new("/cancel", "Cancel an in-flight agent turn")
            .with_hint("/cancel @agent for a specific one"),
        PickerItem::new("/invite", "Invite an actor into the current channel"),
        PickerItem::new("/members", "List members of the current channel"),
        PickerItem::new("/quit", "Leave the chat"),
    ]
}

// ── Agent status helpers (V4) ──────────────────────────────────────────

/// Priority value for a run status. Higher = more important.
fn run_priority(s: RunStatus) -> u8 {
    match s {
        RunStatus::Running => 4,
        RunStatus::WaitingTool => 3,
        RunStatus::PreparingContext => 2,
        RunStatus::Queued => 1,
        // Terminal states: participate only while still in TTL window
        RunStatus::Failed => 0,
        RunStatus::Canceled => 0,
        RunStatus::Completed => 0,
    }
}

/// Timeout threshold in seconds for the given status. Returns 0 for
/// terminal states that have no timeout concept.
fn timeout_for(s: RunStatus) -> i64 {
    match s {
        RunStatus::Queued => TIMEOUT_QUEUED_SECS,
        RunStatus::PreparingContext => TIMEOUT_PREPARING_CONTEXT_SECS,
        RunStatus::Running => TIMEOUT_RUNNING_SECS,
        RunStatus::WaitingTool => TIMEOUT_WAITING_TOOL_SECS,
        _ => 0,
    }
}

/// Produce an English status label for a cached run. (The GUI side keeps
/// its own labels in `gui-web/src/lib/agent-utils.ts`.)
pub fn compute_run_status_label(cr: &CachedRun, is_stale: bool) -> String {
    let base = match cr.status {
        RunStatus::Queued => "Queued".to_string(),
        RunStatus::PreparingContext => "Preparing".to_string(),
        RunStatus::Running => "Thinking".to_string(),
        RunStatus::WaitingTool => {
            // GUI V3 uses meta.toolName for tool name; fall back to start_reason.
            let tool = cr.tool_name.as_deref().or(cr.start_reason.as_deref());
            if let Some(tool) = tool {
                format!("Waiting · {}", tool)
            } else {
                "Waiting".to_string()
            }
        }
        RunStatus::Failed => {
            // GUI V3 priority: meta.error || meta.noReplyReason || start_reason.
            let reason = cr
                .error
                .as_deref()
                .or(cr.no_reply_reason.as_deref())
                .or(cr.start_reason.as_deref());
            if let Some(reason) = reason {
                format!("Failed · {}", reason)
            } else {
                "Failed".to_string()
            }
        }
        RunStatus::Canceled => "Canceled".to_string(),
        RunStatus::Completed => return String::new(), // never show "completed"
    };

    if is_stale {
        return format!("⚠ {} (stale)", base);
    }

    // Append start_reason for non-terminal states (except waiting_tool / failed
    // which already incorporate a reason above).
    if let Some(reason) = cr.start_reason.as_ref() {
        if !matches!(
            cr.status,
            RunStatus::WaitingTool | RunStatus::Failed | RunStatus::Canceled | RunStatus::Completed
        ) {
            return format!("{} · {}", base, reason);
        }
    }

    base.to_string()
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
            proto::types::ScopeKind::Thread,
            "tester".into(),
        );
        app.display_for
            .insert("actor_agent_opencode".into(), "OpenCode".into());
        app.actor_kinds
            .insert("actor_agent_opencode".into(), "agent".into());
        app.agent_ids.insert("actor_agent_opencode".into());

        app.display_for
            .insert("actor_human_old".into(), "tester".into());
        app.actor_kinds
            .insert("actor_human_old".into(), "human".into());

        app.input = "@".into();
        app.update_at_menu();

        let picker = app.at_menu.expect("expected @-menu");
        let ids: Vec<String> = picker.items.into_iter().map(|it| it.id).collect();
        assert_eq!(ids, vec!["actor_agent_opencode"]);
    }

    #[test]
    fn at_menu_in_private_channel_lists_members_and_public_actors_only() {
        use crate::cmd::chat::sidebar::{MemberRow, Sidebar};
        use proto::types::{ActorKind, Channel, ChannelVisibility, Thread};

        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "tester".into(),
        );
        // Three registered agents:
        //   alpha: member of the current private channel
        //   gamma: member of a separate Public channel ("public area")
        //   beta:  member of nothing visible — should be filtered out
        app.agent_ids.insert("actor_agent_alpha".into());
        app.agent_ids.insert("actor_agent_beta".into());
        app.agent_ids.insert("actor_agent_gamma".into());
        app.display_for
            .insert("actor_agent_alpha".into(), "Alpha".into());
        app.display_for
            .insert("actor_agent_beta".into(), "Beta".into());
        app.display_for
            .insert("actor_agent_gamma".into(), "Gamma".into());

        let mut sidebar = Sidebar::new(Some("thread_demo".into()));
        sidebar.replace_channels(vec![
            Channel {
                id: "ch_design".into(),
                title: "design".into(),
                topic: String::new(),
                visibility: ChannelVisibility::Private,
                members: vec!["actor_human_current".into(), "actor_agent_alpha".into()],
                _meta: None,
            },
            Channel {
                id: "ch_lobby".into(),
                title: "lobby".into(),
                topic: String::new(),
                visibility: ChannelVisibility::Public,
                members: vec!["actor_agent_gamma".into()],
                _meta: None,
            },
        ]);
        sidebar.replace_threads(
            "ch_design",
            vec![Thread {
                id: "thread_demo".into(),
                channel_id: "ch_design".into(),
                title: "demo".into(),
                root_message_id: "evt_root".into(),
                archived_at: None,
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
        let ids: Vec<String> = picker.items.iter().map(|it| it.id.clone()).collect();
        // Alpha (channel member) and Gamma (public area) are listed; Beta is gone.
        assert_eq!(ids, vec!["actor_agent_alpha", "actor_agent_gamma"]);
        // No more `(not in #foo)` hints — non-members don't appear at all.
        assert!(picker.items.iter().all(|it| it.hint.is_none()));
    }

    #[test]
    fn at_menu_skips_membership_hint_for_public_channels() {
        use crate::cmd::chat::sidebar::Sidebar;
        use proto::types::{Channel, ChannelVisibility, Thread};

        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "tester".into(),
        );
        app.agent_ids.insert("actor_agent_loner".into());
        app.display_for
            .insert("actor_agent_loner".into(), "Loner".into());

        let mut sidebar = Sidebar::new(Some("thread_demo".into()));
        sidebar.replace_channels(vec![Channel {
            id: "ch_lobby".into(),
            title: "lobby".into(),
            topic: String::new(),
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
                root_message_id: "evt_root".into(),
                archived_at: None,
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
            proto::types::ScopeKind::Thread,
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
            proto::types::ScopeKind::Thread,
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
            proto::types::ScopeKind::Thread,
            "tester".into(),
        );
        app.history.push_system("system");
        app.history.bubbles.push(Bubble {
            actor_id: "actor_agent_alpha".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "older".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_1".into()),
            delivery: DeliveryState::NotApplicable,
        });
        app.history.bubbles.push(Bubble {
            actor_id: "system".into(),
            turn_id: None,
            kind: BubbleKind::System,
            text: "system".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: None,
            delivery: DeliveryState::NotApplicable,
        });
        app.history.bubbles.push(Bubble {
            actor_id: "actor_agent_beta".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "newer".into(),
            ts: Utc::now(),
            reply_to_source_id: None,
            trailing_source_id: Some("evt_2".into()),
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

    // ── V4 agent-status helpers ──────────────────────────────────────────

    use super::*;

    fn cr(status: RunStatus, opened_ago_secs: i64, closed_ago_secs: Option<i64>) -> CachedRun {
        let now = Utc::now();
        CachedRun {
            actor_id: "a".into(),
            status,
            start_reason: Some("mention".into()),
            opened_at: now - chrono::Duration::seconds(opened_ago_secs),
            closed_at: closed_ago_secs.map(|s| now - chrono::Duration::seconds(s)),
            tool_name: None,
            error: None,
            no_reply_reason: None,
        }
    }

    fn cr_tool(status: RunStatus, tool_name: &str) -> CachedRun {
        CachedRun {
            actor_id: "a".into(),
            status,
            start_reason: Some("mention".into()),
            opened_at: Utc::now(),
            closed_at: None,
            tool_name: Some(tool_name.into()),
            error: None,
            no_reply_reason: None,
        }
    }

    fn cr_failed(
        error: Option<&str>,
        no_reply_reason: Option<&str>,
        start_reason: Option<&str>,
    ) -> CachedRun {
        CachedRun {
            actor_id: "a".into(),
            status: RunStatus::Failed,
            start_reason: start_reason.map(|s| s.into()),
            opened_at: Utc::now(),
            closed_at: Some(Utc::now()),
            tool_name: None,
            error: error.map(|s| s.into()),
            no_reply_reason: no_reply_reason.map(|s| s.into()),
        }
    }

    #[test]
    fn run_priority_ordering() {
        assert!(run_priority(RunStatus::Running) > run_priority(RunStatus::WaitingTool));
        assert!(run_priority(RunStatus::WaitingTool) > run_priority(RunStatus::PreparingContext));
        assert!(run_priority(RunStatus::PreparingContext) > run_priority(RunStatus::Queued));
        assert_eq!(run_priority(RunStatus::Failed), 0);
        assert_eq!(run_priority(RunStatus::Canceled), 0);
        assert_eq!(run_priority(RunStatus::Completed), 0);
    }

    #[test]
    fn timeout_for_each_status() {
        assert_eq!(timeout_for(RunStatus::Queued), 300);
        assert_eq!(timeout_for(RunStatus::PreparingContext), 180);
        assert_eq!(timeout_for(RunStatus::Running), 600);
        assert_eq!(timeout_for(RunStatus::WaitingTool), 120);
        assert_eq!(timeout_for(RunStatus::Failed), 0);
        assert_eq!(timeout_for(RunStatus::Canceled), 0);
        assert_eq!(timeout_for(RunStatus::Completed), 0);
    }

    #[test]
    fn compute_run_status_label_queued() {
        let r = cr(RunStatus::Queued, 0, None);
        assert_eq!(compute_run_status_label(&r, false), "Queued · mention");
    }

    #[test]
    fn compute_run_status_label_preparing_context() {
        let r = cr(RunStatus::PreparingContext, 0, None);
        assert_eq!(compute_run_status_label(&r, false), "Preparing · mention");
    }

    #[test]
    fn compute_run_status_label_running() {
        let r = cr(RunStatus::Running, 0, None);
        assert_eq!(compute_run_status_label(&r, false), "Thinking · mention");
    }

    #[test]
    fn compute_run_status_label_waiting_tool_with_tool_name() {
        let r = cr_tool(RunStatus::WaitingTool, "read_file");
        assert_eq!(compute_run_status_label(&r, false), "Waiting · read_file");
    }

    #[test]
    fn compute_run_status_label_waiting_tool_fallback_to_start_reason() {
        let r = CachedRun {
            actor_id: "a".into(),
            status: RunStatus::WaitingTool,
            start_reason: Some("human_mention".into()),
            opened_at: Utc::now(),
            closed_at: None,
            tool_name: None,
            error: None,
            no_reply_reason: None,
        };
        assert_eq!(
            compute_run_status_label(&r, false),
            "Waiting · human_mention"
        );
    }

    #[test]
    fn compute_run_status_label_waiting_tool_no_reason() {
        let r = CachedRun {
            actor_id: "a".into(),
            status: RunStatus::WaitingTool,
            start_reason: None,
            opened_at: Utc::now(),
            closed_at: None,
            tool_name: None,
            error: None,
            no_reply_reason: None,
        };
        assert_eq!(compute_run_status_label(&r, false), "Waiting");
    }

    #[test]
    fn compute_run_status_label_failed_with_error() {
        let r = cr_failed(Some("connection refused"), None, None);
        assert_eq!(
            compute_run_status_label(&r, false),
            "Failed · connection refused"
        );
    }

    #[test]
    fn compute_run_status_label_failed_with_no_reply_reason() {
        let r = cr_failed(None, Some("rate limited"), None);
        assert_eq!(compute_run_status_label(&r, false), "Failed · rate limited");
    }

    #[test]
    fn compute_run_status_label_failed_with_start_reason_fallback() {
        let r = cr_failed(None, None, Some("timeout"));
        assert_eq!(compute_run_status_label(&r, false), "Failed · timeout");
    }

    #[test]
    fn compute_run_status_label_failed_error_priority_over_no_reply() {
        let r = cr_failed(Some("crash"), Some("rate limited"), None);
        assert_eq!(compute_run_status_label(&r, false), "Failed · crash");
    }

    #[test]
    fn compute_run_status_label_failed_no_reason() {
        let r = cr_failed(None, None, None);
        assert_eq!(compute_run_status_label(&r, false), "Failed");
    }

    #[test]
    fn compute_run_status_label_canceled() {
        let r = cr(RunStatus::Canceled, 0, None);
        assert_eq!(compute_run_status_label(&r, false), "Canceled");
    }

    #[test]
    fn compute_run_status_label_completed_is_empty() {
        let r = cr(RunStatus::Completed, 0, Some(1));
        assert_eq!(compute_run_status_label(&r, false), "");
    }

    #[test]
    fn compute_run_status_label_stale_queued() {
        let r = cr(RunStatus::Queued, 0, None);
        // stale path returns early before appending start_reason
        assert_eq!(compute_run_status_label(&r, true), "⚠ Queued (stale)");
    }

    #[test]
    fn compute_run_status_label_no_start_reason() {
        let r = CachedRun {
            actor_id: "a".into(),
            status: RunStatus::Running,
            start_reason: None,
            opened_at: Utc::now(),
            closed_at: None,
            tool_name: None,
            error: None,
            no_reply_reason: None,
        };
        assert_eq!(compute_run_status_label(&r, false), "Thinking");
    }

    #[test]
    fn recompute_agent_statuses_picks_highest_priority() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );
        let now = Utc::now();

        // Two runs for same actor: running (priority 4) + queued (priority 1)
        app.run_cache.insert(
            "r1".into(),
            CachedRun {
                actor_id: "agent_x".into(),
                status: RunStatus::Queued,
                start_reason: Some("batch".into()),
                opened_at: now,
                closed_at: None,
                tool_name: None,
                error: None,
                no_reply_reason: None,
            },
        );
        app.run_cache.insert(
            "r2".into(),
            CachedRun {
                actor_id: "agent_x".into(),
                status: RunStatus::Running,
                start_reason: Some("mention".into()),
                opened_at: now,
                closed_at: None,
                tool_name: None,
                error: None,
                no_reply_reason: None,
            },
        );

        app.recompute_agent_statuses();

        let info = app
            .agent_statuses
            .get("agent_x")
            .expect("agent_x should have status");
        assert_eq!(info.status, Some(RunStatus::Running));
        assert!(!info.is_stale, "not old enough to be stale");
        assert!(info.label.contains("Thinking"));
    }

    #[test]
    fn recompute_agent_statuses_terminal_run_has_ttl() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );
        let now = Utc::now();

        // Terminal run closed just now — should still appear
        app.run_cache.insert(
            "r1".into(),
            CachedRun {
                actor_id: "agent_y".into(),
                status: RunStatus::Failed,
                start_reason: None,
                opened_at: now - chrono::Duration::seconds(10),
                closed_at: Some(now - chrono::Duration::seconds(5)),
                tool_name: None,
                error: Some("crash".into()),
                no_reply_reason: None,
            },
        );

        app.recompute_agent_statuses();

        let info = app
            .agent_statuses
            .get("agent_y")
            .expect("agent_y should have status within TTL");
        assert_eq!(info.status, Some(RunStatus::Failed));
        assert!(!info.is_stale);
        assert!(info.label.contains("Failed"));
    }

    #[test]
    fn recompute_agent_statuses_terminal_run_expired_ttl() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );
        let now = Utc::now();

        // Terminal run closed 60s ago — beyond 30s TTL
        app.run_cache.insert(
            "r1".into(),
            CachedRun {
                actor_id: "agent_z".into(),
                status: RunStatus::Completed,
                start_reason: None,
                opened_at: now - chrono::Duration::seconds(70),
                closed_at: Some(now - chrono::Duration::seconds(60)),
                tool_name: None,
                error: None,
                no_reply_reason: None,
            },
        );

        app.recompute_agent_statuses();

        assert!(
            app.agent_statuses.get("agent_z").is_none(),
            "agent_z should be evicted after TTL expiry"
        );
    }

    #[test]
    fn recompute_agent_statuses_stale_detection() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );
        let now = Utc::now();

        // Queued run opened 400s ago — exceeds 300s timeout
        app.run_cache.insert(
            "r1".into(),
            CachedRun {
                actor_id: "agent_slow".into(),
                status: RunStatus::Queued,
                start_reason: None,
                opened_at: now - chrono::Duration::seconds(400),
                closed_at: None,
                tool_name: None,
                error: None,
                no_reply_reason: None,
            },
        );

        app.recompute_agent_statuses();

        let info = app
            .agent_statuses
            .get("agent_slow")
            .expect("agent_slow should have status");
        assert!(
            info.is_stale,
            "run open 400s should be stale for Queued (300s)"
        );
        assert!(info.label.contains("⚠"));
    }

    #[test]
    fn recompute_agent_statuses_removes_stale_entries() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );

        // Only stale runs — after recompute status_label is stale-marked but
        // the entry should remain (is_stale is informational, not eviction).
        app.run_cache.insert(
            "r1".into(),
            CachedRun {
                actor_id: "agent_stale".into(),
                status: RunStatus::Queued,
                start_reason: None,
                opened_at: Utc::now() - chrono::Duration::seconds(400),
                closed_at: None,
                tool_name: None,
                error: None,
                no_reply_reason: None,
            },
        );

        app.recompute_agent_statuses();

        let info = app
            .agent_statuses
            .get("agent_stale")
            .expect("stale entries are retained (not evicted)");
        assert!(info.is_stale);
    }

    #[test]
    fn recompute_agent_statuses_idle_actor_no_entry() {
        let mut app = App::new(
            "me".into(),
            "t1".into(),
            proto::types::ScopeKind::Thread,
            "demo".into(),
        );
        app.run_cache.clear();
        app.agent_statuses.clear();

        app.recompute_agent_statuses();

        assert!(
            app.agent_statuses.is_empty(),
            "no status for actors without runs"
        );
    }
}
