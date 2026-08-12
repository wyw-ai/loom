//! Workspace-resident Loom bootstrap for provider-native instruction discovery.
//!
//! Loom writes a concise generated block into `{workspace}/AGENTS.md`. The block
//! contains stable actor/channel facts and pointers to Loom tooling. Dynamic
//! thread/turn/task facts stay out of this file.

use std::io;
use std::path::Path;

const BEGIN_MARKER: &str = "<!-- BEGIN loom -->";
const END_MARKER: &str = "<!-- END loom -->";
const CLAUDE_BRIDGE_BEGIN_MARKER: &str = "<!-- BEGIN loom claude bridge -->";
const CLAUDE_BRIDGE_END_MARKER: &str = "<!-- END loom claude bridge -->";
const CLAUDE_BRIDGE_BLOCK: &str = "<!-- BEGIN loom claude bridge -->\n\
@AGENTS.md\n\
<!-- END loom claude bridge -->";

#[derive(Debug, Clone, Default)]
pub struct AgentsMdContext {
    pub actor_id: String,
    pub actor_display_name: String,
    pub channel_id: String,
    pub channel_title: String,
    pub channel_topic: String,
    pub workspace: String,
    pub members: Vec<AgentsMdMember>,
    pub agent_instructions: Option<String>,
    /// Channel-level instructions from the channel's `instructions` field.
    pub channel_instructions: Option<String>,
    /// Thread-level instructions from the thread's `instructions` field.
    /// Only set when the current scope is a thread.
    pub thread_instructions: Option<String>,
    pub wake_policy: AgentsMdWakePolicy,
}

#[derive(Debug, Clone, Default)]
pub struct AgentsMdMember {
    pub actor_id: String,
    pub display_name: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct AgentsMdWakePolicy {
    pub coalesce: bool,
    pub debounce_ms: u64,
    pub reply_reminder: String,
    pub busy_policy: String,
    pub context_token_budget: Option<u64>,
}

impl Default for AgentsMdWakePolicy {
    fn default() -> Self {
        Self {
            coalesce: true,
            debounce_ms: 0,
            reply_reminder: "first-turn".into(),
            busy_policy: "queue".into(),
            context_token_budget: None,
        }
    }
}

/// Create or refresh the Loom block inside `{workspace}/AGENTS.md`.
///
/// Content outside the Loom markers is never changed. If the existing Loom
/// block already matches the newly generated block, the file is left untouched.
pub fn ensure_agents_md(workspace: &Path, context: &AgentsMdContext) -> io::Result<()> {
    let path = workspace.join("AGENTS.md");
    let block = loom_block(context);
    let new_content = match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let updated = update_block(&existing, &block);
            if updated == existing {
                return Ok(());
            }
            updated
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut out = String::with_capacity(block.len() + 1);
            out.push_str(&block);
            out.push('\n');
            out
        }
        Err(e) => return Err(e),
    };
    std::fs::write(&path, new_content)
}

/// Remove only Loom's generated block, preserving project-owned instructions.
pub fn remove_agents_md(workspace: &Path) -> io::Result<()> {
    let path = workspace.join("AGENTS.md");
    let existing = match std::fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let Some((begin, end_past)) = marker_span(&existing) else {
        return Ok(());
    };
    let mut after = &existing[end_past..];
    if begin == 0 {
        after = after
            .strip_prefix("\r\n\r\n")
            .or_else(|| after.strip_prefix("\n\n"))
            .or_else(|| after.strip_prefix("\r\n"))
            .or_else(|| after.strip_prefix('\n'))
            .unwrap_or(after);
    }
    let mut content = String::with_capacity(existing.len());
    content.push_str(&existing[..begin]);
    content.push_str(after);
    if content.is_empty() {
        std::fs::remove_file(path)
    } else {
        std::fs::write(path, content)
    }
}

/// Ensure Claude Code discovers the authoritative workspace `AGENTS.md` through
/// its native project-memory file. The bridge contains only an import, so Loom
/// operating rules still have one source of truth. Project-owned `CLAUDE.md`
/// content outside the bridge markers is preserved.
pub fn ensure_claude_md_bridge(workspace: &Path) -> io::Result<()> {
    let path = workspace.join("CLAUDE.md");
    let new_content = match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let updated = update_marked_block(
                &existing,
                CLAUDE_BRIDGE_BLOCK,
                CLAUDE_BRIDGE_BEGIN_MARKER,
                CLAUDE_BRIDGE_END_MARKER,
            );
            if updated == existing {
                return Ok(());
            }
            updated
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            format!("{CLAUDE_BRIDGE_BLOCK}\n")
        }
        Err(error) => return Err(error),
    };
    std::fs::write(path, new_content)
}

/// Remove only Loom's Claude bridge and preserve project-owned `CLAUDE.md`
/// instructions. A file containing only the bridge is removed entirely.
pub fn remove_claude_md_bridge(workspace: &Path) -> io::Result<()> {
    remove_marked_block(
        &workspace.join("CLAUDE.md"),
        CLAUDE_BRIDGE_BEGIN_MARKER,
        CLAUDE_BRIDGE_END_MARKER,
    )
}

fn update_block(existing: &str, new_block: &str) -> String {
    update_marked_block(existing, new_block, BEGIN_MARKER, END_MARKER)
}

fn update_marked_block(
    existing: &str,
    new_block: &str,
    begin_marker: &str,
    end_marker: &str,
) -> String {
    if let Some((begin, end_past)) = marker_span_for(existing, begin_marker, end_marker) {
        let mut out = String::with_capacity(existing.len() + new_block.len());
        out.push_str(&existing[..begin]);
        out.push_str(new_block);
        out.push_str(&existing[end_past..]);
        return out;
    }

    let mut out = String::with_capacity(existing.len() + new_block.len() + 2);
    out.push_str(new_block);
    out.push('\n');
    if !existing.is_empty() {
        out.push('\n');
        out.push_str(existing);
    }
    out
}

fn marker_span(existing: &str) -> Option<(usize, usize)> {
    marker_span_for(existing, BEGIN_MARKER, END_MARKER)
}

fn marker_span_for(existing: &str, begin_marker: &str, end_marker: &str) -> Option<(usize, usize)> {
    let begin = existing.find(begin_marker)?;
    let search_from = begin + begin_marker.len();
    let end_rel = existing[search_from..].find(end_marker)?;
    let end = search_from + end_rel;
    Some((begin, end + end_marker.len()))
}

fn remove_marked_block(path: &Path, begin_marker: &str, end_marker: &str) -> io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let Some((begin, end_past)) = marker_span_for(&existing, begin_marker, end_marker) else {
        return Ok(());
    };
    let mut after = &existing[end_past..];
    if begin == 0 {
        after = after
            .strip_prefix("\r\n\r\n")
            .or_else(|| after.strip_prefix("\n\n"))
            .or_else(|| after.strip_prefix("\r\n"))
            .or_else(|| after.strip_prefix('\n'))
            .unwrap_or(after);
    }
    let mut content = String::with_capacity(existing.len());
    content.push_str(&existing[..begin]);
    content.push_str(after);
    if content.is_empty() {
        std::fs::remove_file(path)
    } else {
        std::fs::write(path, content)
    }
}

fn loom_block(context: &AgentsMdContext) -> String {
    let actor_display = optional_field(&context.actor_display_name);
    let channel_title = optional_field(&context.channel_title);
    let channel_topic = optional_field(&context.channel_topic);
    let members = render_members(&context.members);
    let wake_policy = render_wake_policy(&context.wake_policy);
    let mut block = format!(
        "{BEGIN_MARKER}\n\
# Loom runtime bootstrap\n\
\n\
This block is generated by Loom. Keep local project rules outside the Loom\n\
markers; Loom may refresh this block when actor or channel context changes.\n\
\n\
## Actor\n\
\n\
- Actor id: `{actor_id}`\n\
- Display name: {actor_display}\n\
- Workspace: `{workspace}`\n\
\n\
## Channel\n\
\n\
- Channel id: `{channel_id}`\n\
- Title: {channel_title}\n\
- Topic: {channel_topic}\n\
\n\
## Channel members\n\
\n\
{members}\n\
\n\
## Wake intake policy\n\
\n\
{wake_policy}\n\
\n\
## Loom operating rules\n\
\n\
- This file contains stable actor/channel facts. Thread, turn, trigger, task,\n\
  assignment, and inbox facts are dynamic and belong to the current turn input.\n\
- Current runtime values are available in environment variables such as\n\
  `LOOM_ACTOR`, `LOOM_CHANNEL_ID`, `LOOM_SCOPE_ID`, `LOOM_REPLY_TARGET`,\n\
  `LOOM_TRIGGER_MESSAGE_ID`, `LOOM_TRIGGER_ACTOR`, `LOOM_TRIGGER_PRIVATE`,\n\
  and `LOOM_TRIGGER_PRIVATE_TO_FLAGS`.\n\
- Before acting, identify your role for this wake: requester/coordinator,\n\
  participant/contributor, assignee/reviewer, observer, or no-action recipient.\n\
  Use the Loom primitive for that role; do not take over coordination unless\n\
  you own the task, were asked to coordinate, or successfully claimed it.\n\
- A recognizable activity is not yet an executable collaboration contract.\n\
  When several actors are invited to participate but the current conversation\n\
  does not already establish how they will work together, negotiate that shape\n\
  before doing the activity itself. This applies even when the activity's\n\
  subject or desired outcome is obvious.\n\
- Treat this as a brief, visible protocol-negotiation phase. Relevant actors\n\
  should exchange concrete proposals about only the dimensions the activity\n\
  needs: participation, facilitation or state ownership, roles, ordering or\n\
  concurrency, handoff signals, shared-state updates, and completion or stop\n\
  conditions. Derive these from the user's actual request and current context;\n\
  do not import a domain template or assume voting, leadership, or sequential\n\
  turns. Ask the user only for intent that cannot be inferred safely.\n\
- Derive the initial participant set from the actionable request: its routed\n\
  audience plus actors explicitly included or excluded by the requester. Channel\n\
  membership establishes who is available, not who agreed or was invited to\n\
  participate. Do not add the requester, observers, or other members merely\n\
  because they are present in the channel; later expand the set only through an\n\
  explicit proposal and acceptance when the activity actually needs it.\n\
- Use one convergence owner so simultaneous wakes do not produce competing\n\
  protocols. When the current wake is the initiating multi-actor invitation and\n\
  no owner is established yet, attempt to become the negotiation owner with\n\
  `loom --json task claim --source-message \"$LOOM_TRIGGER_MESSAGE_ID\"`. Do not\n\
  claim again when an established owner asks for a proposal, announces the\n\
  contract, or hands off activity work; answer or act in the requested role. A\n\
  successful claimant retains the task id returned by `task claim`, facilitates,\n\
  and explicitly asks other relevant actors for proposals with `message ask`,\n\
  naming the expected respondents and convergence condition; it does not start\n\
  substantive participation yet.\n\
  On a claim conflict, use same-scope task state to identify the established\n\
  owner and do not start the activity or a second negotiation. If the owner has\n\
  already requested a proposal, answer once with `message ask`; otherwise end\n\
  no-reply and let the owner's required solicitation wake you. The owner treats\n\
  proposal replies as negotiation input, not as fresh activities that need a\n\
  new claim, and does not launch while required respondents remain unaccounted\n\
  for under the announced convergence condition. Claiming this role grants\n\
  coordination responsibility, not authority to invent the result.\n\
- Negotiation collection is read-before-write and idempotent. Before reporting\n\
  missing respondents, re-asking, converging, or launching, the owner must read\n\
  the current conversation (for example, `loom --json message read --target\n\
  \"$LOOM_REPLY_TARGET\"`) and rebuild one latest-effective-response entry per\n\
  expected actor. A delayed wake is not evidence that another reply is absent.\n\
  On any duplicate delivery of the same solicitation, a participant that has\n\
  already supplied an effective response must not send it again; inspect the\n\
  conversation and end with `run ignore` instead. A later explicit correction\n\
  supersedes that actor's earlier response.\n\
- Negotiation is complete only when the owner publishes one concise operational\n\
  contract in the shared scope, resolves material disagreements, and identifies\n\
  who owns shared progress. Publish that contract separately from substantive\n\
  work as a no-action `message send --intent notify`, then explicitly route\n\
  exactly the first required actor or actor set with `message ask`. Do not use a\n\
  plain public `message send` to launch a phase: reply inference may wake only\n\
  the current trigger. Inspect the returned message audience and correct any\n\
  mismatch before claiming the phase started. Only then begin participation.\n\
  Afterward, follow the agreed protocol and use explicit Loom handoffs whenever\n\
  another actor must act next. At the agreed stop, the owner completes the\n\
  retained bootstrap task before publishing the final outcome.\n\
- Open or free-form participation still needs an executable routing policy.\n\
  Explicitly wake every actor currently eligible to act, or select and record\n\
  one first actor and route later turns through visible handoffs. Public text is\n\
  shared context, not proof that the intended actors were woken. When the owner\n\
  contributes before handing off, send that contribution with `--intent notify`\n\
  and make the next actor's `message ask` a separate command so reply inference\n\
  cannot create an unintended parallel wake.\n\
- Keep every promised mechanism operational and auditable. Perform and verify\n\
  any selection, private delivery, reminder, timeout, or state transition with\n\
  Loom primitives before saying it happened. Do not announce a clock-based\n\
  fallback unless a reminder or another observable trigger will actually enact\n\
  it, and do not combine a phase announcement with an implicit handoff.\n\
- For decisions, votes, reviews, tallies, next-speaker handoffs, or other\n\
  stateful choices, inspect enough current conversation before answering; do\n\
  not rely only on the latest wake if prior messages determine the choice.\n\
- For check-ins, votes, approvals, reviews, or other collection phases, rebuild\n\
  the participant ledger from the current thread and same-scope pending inbox\n\
  before declaring someone missing, tallying, or re-asking.\n\
- Treat Loom messages, tasks, assignments, artifacts, and reminders as durable\n\
  collaboration facts. Workspace-local files are derived state and should stay\n\
  recoverable from Loom-visible facts.\n\
- When you accept owner/coordinator responsibility for a multi-step workflow,\n\
  create or claim the message-anchored task when possible. Use message routing\n\
  for short handoffs, task/assignment for lifecycle ownership, coordination for\n\
  explicit baton or slot flows, reminders for rechecks, and facts, projections,\n\
  or artifacts for recoverable non-private state.\n\
- Before private or parallel work begins, publish the non-private workflow\n\
  frame that participants need: roles, rules, constraints, order, and stop or\n\
  success conditions. Keep secrets private, but do not make participants infer\n\
  public rules from hidden assignments.\n\
- Assistant text is an internal run transcript. If a message asks you to answer,\n\
  speak, choose, vote, submit a result, or take your turn, execute a Loom CLI\n\
  command before ending; otherwise the answer is not delivered to the thread.\n\
- Loom stores message text literally. For multiline visible messages, pass real\n\
  newline characters to `--text`; do not write escaped `\\n` unless the backslash\n\
  and letter `n` should be shown to readers. In shell, prefer stdin/heredoc for\n\
  multiline text instead of quoted `\\n` sequences.\n\
- To send a file or image into chat, first run\n\
  `loom --json attachment upload --target \"$LOOM_REPLY_TARGET\" --path <path>`,\n\
  then include its returned artifact id on the visible `message send` or\n\
  `message ask` command with `--attachment-id <art_id>`. Uploading alone, saving\n\
  a workspace file, or writing an `artifact://` URI in message text does not\n\
  attach the file and will not make it appear in the chat attachment panel.\n\
- Use `$LOOM_REPLY_TARGET` as the default target for the current workflow. If it\n\
  is a thread target such as `#channel:root`, send or ask on the bare `#channel`\n\
  only when you intentionally want a channel-level update outside that thread.\n\
- Public replies that only inform the thread may use\n\
  `loom --json message send --target \"$LOOM_REPLY_TARGET\" --text \"...\"`.\n\
  If you answer a public ask and the requester/coordinator must collect it or\n\
  continue after your reply, use\n\
  `loom --json message ask @actor_id --target \"$LOOM_REPLY_TARGET\" --text \"...\"`\n\
  so that actor is woken explicitly. Loom may also infer this wake-back for\n\
  agent replies to public asks, but do not rely on inference for handoffs.\n\
  Do not send the same answer once with `message send` and again with\n\
  `message ask`; choose the routed form when a wake-back is needed.\n\
- Requested answers such as joining, voting, choosing, approving, reviewing,\n\
  or completing a step are actionable even when they are short; wake the\n\
  requester/coordinator instead of sending them notify-only.\n\
- Do not use `message ask` for waiting, acknowledgement, no-reply, or status\n\
  messages that require no recipient action. Send them with explicit\n\
  `message send --intent notify` when they are useful, or omit them.\n\
- Final summaries, wrap-ups, and phase results that require no further action\n\
  should use `message send` or `message send --intent notify`, not `message ask`.\n\
- A public phase transition, broadcast, or handoff that asks participants to\n\
  discuss, vote, review, approve, continue, or otherwise act is not complete\n\
  unless it is routed with `message ask` to the exact actor(s) or appropriate\n\
  group. Natural language such as \"everyone please start\" does not wake agents;\n\
  agent CLI may reject notify-only messages that look like action requests.\n\
- If `LOOM_TRIGGER_PRIVATE=1`, private answers use\n\
  `loom --json message send $LOOM_TRIGGER_PRIVATE_TO_FLAGS --text \"...\"`;\n\
  never send a private answer to `$LOOM_REPLY_TARGET`.\n\
- If the current turn input says `Private route for this turn`, use its exact\n\
  dynamic command when answering the private requester(s).\n\
- If you are the requester/coordinator receiving a completed private action,\n\
  vote, target, approval, or other answer, process it and route the next\n\
  required actor; do not answer the submitter again unless you need\n\
  clarification.\n\
- If a private wake requires a hidden follow-up with another actor, keep that\n\
  follow-up private too: use same-scope\n\
  `loom --json message send --private-to @actor_id --target \"$LOOM_REPLY_TARGET\" --text \"...\"`.\n\
- Use public `message ask` from private context only when the requested output\n\
  is explicitly intended for the public thread; keep private facts out.\n\
- Private actions, votes, target choices, and sensitive data stay private even\n\
  when the answer is only one word.\n\
- Public messages should include only information intended for that audience;\n\
  do not add labels, hints, or formatting derived from private state.\n\
- In ordered workflows, do not take over sequencing unless you own it or were\n\
  explicitly delegated. Participants should wake the requester/coordinator with\n\
  their completion. For dynamic eligibility, permissions, lifecycle,\n\
  membership, or other mutable state, the coordinator should read current state\n\
  and route the next eligible actor.\n\
- If you are the requester/coordinator receiving a completed public answer,\n\
  process it and wake the next required actor; do not ask the submitter again\n\
  unless you need clarification.\n\
- In multi-party decisions, rebuild the latest effective decision for each\n\
  required participant before declaring agreement; crossed or stale replies do\n\
  not count as consensus.\n\
- If same-phase replies conflict or include corrections, use the latest\n\
  explicit final/correction visible to the allowed audience, or ask for\n\
  clarification. After consuming answers, do not end silently: record the\n\
  accepted result, wake the next actor, schedule a reminder, or surface the\n\
  blocker.\n\
- If a private wake asks for a public contribution, publish it with\n\
  `message ask` to the requester/coordinator unless you own or were delegated\n\
  the next handoff; keep private facts out of the public text.\n\
- If a wake is only informational, has `notify` / `notify_only` delivery, or\n\
  explicitly asks for no reply, do not send a receipt; run\n\
  `loom --json run ignore --reason \"no action needed\"`.\n\
- Do not answer acknowledgement-only or waiting messages that do not change\n\
  state; run `loom --json run ignore --reason \"no action needed\"`.\n\
- Use `loom --json message ask @actor_id ...` when another actor must act next;\n\
  plain `message send` is for visible text and should not be used as an\n\
  implicit handoff.\n\
- Prefer same-scope private delivery for hidden prompts that require action in\n\
  an active workflow:\n\
  `loom --json message send --private-to @actor_id --target \"$LOOM_REPLY_TARGET\" --text \"...\"`.\n\
  If you assign hidden or actor-specific information, actually send it this\n\
  way before announcing it as done; if the recipient must act, that private\n\
  message is the wake.\n\
  If the private message is background context only and does not require action\n\
  now, use `--intent notify --delivery-policy notify_only` or omit it, and make\n\
  the later action wake carry enough context to act correctly.\n\
  Global `dm:@actor_id` starts a separate private channel; use it deliberately\n\
  only when leaving the current workflow scope is intended.\n\
- Query fresh state with `loom --json ...` commands before relying on channel,\n\
  thread, task, assignment, or actor state.\n\
- Before ending, make the next required step explicit: send the required reply,\n\
  wake the next actor(s), complete/update the task or assignment, schedule a\n\
  reminder, or run `loom --json run ignore --reason \"...\"`.\n\
- Use the default `loom` skill for scenario routing. Use `loom guide list`,\n\
  `loom guide show <topic>`, and `loom guide search <query>` for detailed\n\
  Loom operating guidance.\n\
",
        actor_id = inline_value(&context.actor_id),
        actor_display = actor_display,
        workspace = inline_value(&context.workspace),
        channel_id = inline_value(&context.channel_id),
        channel_title = channel_title,
        channel_topic = channel_topic,
        members = members,
        wake_policy = wake_policy,
    );

    if let Some(instructions) = context
        .agent_instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str("\n## Stable agent instructions\n\n");
        block.push_str(&sanitize_marker_text(instructions));
        block.push('\n');
    }

    if let Some(instructions) = context
        .channel_instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        block.push_str("\n## Channel instructions\n\n");
        block.push_str(&sanitize_marker_text(instructions));
        block.push('\n');
    }

    // NOTE: Thread instructions are intentionally NOT rendered into the
    // shared AGENTS.md block. Two threads share one workspace, so writing
    // thread-scoped instructions into the shared AGENTS.md would let one
    // thread's instructions overwrite another's (issue #1). Thread
    // instructions are shelved at the projection layer; the data model
    // (Thread.instructions, AgentsMdContext.thread_instructions field) is
    // preserved for a future per-Run provider-scope implementation. See
    // docs/channel-thread-instructions.md and the PR40 fix #1+#9.

    block.push_str(&format!("\n{END_MARKER}"));
    block
}

fn render_wake_policy(policy: &AgentsMdWakePolicy) -> String {
    let mut lines = vec![
        format!(
            "- Coalesce queued same-scope triggers: `{}`",
            policy.coalesce
        ),
        format!("- Dispatch debounce: `{}` ms", policy.debounce_ms),
        format!(
            "- Reply reminder: `{}`",
            inline_value(&policy.reply_reminder)
        ),
        format!(
            "- Human message while busy: `{}`",
            inline_value(&policy.busy_policy)
        ),
    ];
    if let Some(budget) = policy.context_token_budget {
        lines.push(format!("- Context token budget: `{budget}`"));
    }

    if policy.coalesce || policy.debounce_ms > 0 {
        lines.extend([
            "- Strategy: this actor may receive several same-scope inputs in one turn.".to_string(),
            "  Treat the current USER message as a Loom turn inbox: handle wake[],".to_string(),
            "  fold in any pending same-scope inbox items shown there, merge related".to_string(),
            "  state changes for the same workflow, handle independent requests".to_string(),
            "  separately, and use the latest effective state instead of stale".to_string(),
            "  messages blindly. If extra inspection is needed, use".to_string(),
            "  `loom --json inbox list --state pending --no-ack`.".to_string(),
        ]);
    } else {
        lines.extend([
            "- Strategy: this actor is expected to process one focused wake at a time.".to_string(),
            "  Do not proactively drain unrelated unread inbox or channel messages.".to_string(),
            "  Read more history only when needed to validate state or avoid conflict;".to_string(),
            "  if you inspect pending inbox, use `loom --json inbox list --state pending --no-ack`.".to_string(),
            "  This keeps inspection from consuming delivery state.".to_string(),
        ]);
    }

    lines.join("\n")
}

fn render_members(members: &[AgentsMdMember]) -> String {
    if members.is_empty() {
        return "- No channel member list was available when this file was generated.".into();
    }
    members
        .iter()
        .map(|member| {
            let display = optional_field(&member.display_name);
            let kind = optional_field(&member.kind);
            format!(
                "- `{}` - {} ({})",
                inline_value(&member.actor_id),
                display,
                kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn optional_field(value: &str) -> String {
    let value = inline_value(value);
    if value.is_empty() {
        "`unknown`".into()
    } else {
        value
    }
}

fn inline_value(value: &str) -> String {
    sanitize_marker_text(value)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .replace('`', "'")
}

fn sanitize_marker_text(value: &str) -> String {
    value
        .replace(BEGIN_MARKER, "[removed Loom begin marker]")
        .replace(END_MARKER, "[removed Loom end marker]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(actor_id: &str, channel_id: &str) -> AgentsMdContext {
        AgentsMdContext {
            actor_id: actor_id.into(),
            actor_display_name: "Demo Agent".into(),
            channel_id: channel_id.into(),
            channel_title: "Demo Channel".into(),
            channel_topic: "Ship the feature".into(),
            workspace: "C:/tmp/loom/workspace".into(),
            members: vec![
                AgentsMdMember {
                    actor_id: "actor_human_owner".into(),
                    display_name: "Owner".into(),
                    kind: "human".into(),
                },
                AgentsMdMember {
                    actor_id: actor_id.into(),
                    display_name: "Demo Agent".into(),
                    kind: "agent".into(),
                },
            ],
            agent_instructions: Some("Prefer concise answers.".into()),
            channel_instructions: None,
            thread_instructions: None,
            wake_policy: AgentsMdWakePolicy::default(),
        }
    }

    #[test]
    fn first_write_contains_stable_context_and_rules() {
        let out = update_block("", &loom_block(&context("actor_demo", "chan_demo")));
        assert!(out.contains(BEGIN_MARKER));
        assert!(out.contains(END_MARKER));
        assert!(out.contains("Actor id: `actor_demo`"));
        assert!(out.contains("Channel id: `chan_demo`"));
        assert!(out.contains("Demo Channel"));
        assert!(out.contains("actor_human_owner"));
        assert!(out.contains("Wake intake policy"));
        assert!(out.contains("Coalesce queued same-scope triggers: `true`"));
        assert!(out.contains("Strategy: this actor may receive several same-scope inputs"));
        assert!(out.contains("Stable agent instructions"));
        assert!(out.contains("Prefer concise answers."));
        assert!(out.contains("Assistant text is an internal run transcript"));
        assert!(out.contains("pass real"));
        assert!(out.contains("escaped `\\n`"));
        assert!(out.contains("stdin/heredoc"));
        assert!(out.contains("identify your role for this wake"));
        assert!(out.contains("participant/contributor"));
        assert!(out.contains("not yet an executable collaboration contract"));
        assert!(out.contains("subject or desired outcome is obvious"));
        assert!(out.contains("visible protocol-negotiation phase"));
        assert!(out.contains("ordering or"));
        assert!(out.contains("concurrency"));
        assert!(out.contains("do not import a domain template"));
        assert!(out.contains("actionable request: its routed"));
        assert!(out.contains("membership establishes who is available"));
        assert!(out.contains("Do not add the requester, observers"));
        assert!(out.contains("task claim --source-message"));
        assert!(out.contains("initiating multi-actor invitation"));
        assert!(out.contains("Do not"));
        assert!(out.contains("claim again when an established owner asks"));
        assert!(out.contains("explicitly asks other relevant actors"));
        assert!(out.contains("On a claim conflict"));
        assert!(out.contains("expected respondents and convergence"));
        assert!(out.contains("proposal replies as negotiation input"));
        assert!(out.contains("required respondents remain unaccounted"));
        assert!(out.contains("read-before-write and idempotent"));
        assert!(out.contains("latest-effective-response entry"));
        assert!(out.contains("duplicate delivery of the same solicitation"));
        assert!(out.contains("A delayed wake is not evidence"));
        assert!(out.contains("do not start the"));
        assert!(out.contains("publishes one concise operational"));
        assert!(out.contains("Publish that contract separately"));
        assert!(out.contains("message send --intent notify"));
        assert!(out.contains("exactly the first required actor"));
        assert!(out.contains("Inspect the returned message audience"));
        assert!(out.contains("Open or free-form participation"));
        assert!(out.contains("Public text is"));
        assert!(out.contains("an unintended parallel wake"));
        assert!(out.contains("retained bootstrap task before publishing"));
        assert!(out.contains("operational and auditable"));
        assert!(out.contains("before saying it happened"));
        assert!(out.contains("clock-based"));
        assert!(out.contains("durable"));
        assert!(out.contains("Workspace-local files are derived state"));
        assert!(out.contains("message-anchored task"));
        assert!(out.contains("coordination for"));
        assert!(out.contains("recoverable non-private state"));
        assert!(out.contains("ordered workflows"));
        assert!(out.contains("dynamic eligibility"));
        assert!(out.contains("route the next eligible actor"));
        assert!(out.contains("acknowledgement-only"));
        assert!(out.contains("Final summaries"));
        assert!(out.contains("do not rely on inference for handoffs"));
        assert!(out.contains("Do not send the same answer"));
        assert!(out.contains("private wake asks for a public contribution"));
        assert!(out.contains("actually send it this"));
        assert!(out.contains("latest effective decision"));
        assert!(out.contains("loom guide show <topic>"));
        assert!(!out.contains("### Read-only CLI"));
        assert!(!out.contains("### Write CLI"));
    }

    #[test]
    fn collaboration_bootstrap_is_domain_neutral() {
        let out = loom_block(&context("actor_demo", "chan_demo"));
        for domain_term in [
            "counting game",
            "number counting",
            "turtle soup",
            "lateral thinking puzzle",
            "werewolf game",
            "Mafia game",
            "报数",
            "海龟汤",
            "狼人杀",
        ] {
            assert!(
                !out.to_ascii_lowercase().contains(domain_term),
                "generated collaboration contract must not embed scenario-specific rules: {domain_term}"
            );
        }
    }

    #[test]
    fn preserves_user_content_outside_markers() {
        let existing = format!(
            "# My project rules\n\nStyle: tabs.\n\n{BEGIN_MARKER}\nold loom content\n{END_MARKER}\n\n## Postscript\n\nMore notes.\n"
        );
        let out = update_block(&existing, &loom_block(&context("actor_x", "chan_x")));
        assert!(out.contains("My project rules"));
        assert!(out.contains("Style: tabs."));
        assert!(out.contains("Postscript"));
        assert!(out.contains("More notes."));
        assert!(out.contains("actor_x"));
        assert!(out.contains("chan_x"));
        assert!(!out.contains("old loom content"));
    }

    #[test]
    fn prepends_block_when_no_markers() {
        let existing = "# Existing file\n\nHello.\n";
        let out = update_block(existing, &loom_block(&context("actor_y", "chan_y")));
        assert!(out.starts_with(BEGIN_MARKER));
        assert!(out.contains("actor_y"));
        assert!(out.contains("chan_y"));
        assert!(out.contains("# Existing file"));
    }

    #[test]
    fn idempotent_across_repeated_writes() {
        let once = update_block("", &loom_block(&context("actor_z", "chan_z")));
        let twice = update_block(&once, &loom_block(&context("actor_z", "chan_z")));
        assert_eq!(once, twice);
    }

    #[test]
    fn remove_preserves_project_owned_instructions() {
        let root =
            std::env::temp_dir().join(format!("loom-agents-md-remove-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("workspace");
        let path = root.join("AGENTS.md");
        std::fs::write(&path, "project before\n\nproject after\n").expect("project agents");
        ensure_agents_md(&root, &context("actor_z", "chan_z")).expect("inject loom block");

        remove_agents_md(&root).expect("remove loom block");

        let content = std::fs::read_to_string(path).expect("preserved agents");
        assert!(content.contains("project before"));
        assert!(content.contains("project after"));
        assert!(!content.contains(BEGIN_MARKER));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn claude_bridge_imports_agents_md_and_preserves_project_content() {
        let root = std::env::temp_dir().join(format!("loom-claude-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("workspace");
        let path = root.join("CLAUDE.md");
        std::fs::write(&path, "# Project Claude rules\n\nKeep this.\n").expect("project claude");

        ensure_claude_md_bridge(&root).expect("inject Claude bridge");
        ensure_claude_md_bridge(&root).expect("idempotent Claude bridge");

        let content = std::fs::read_to_string(&path).expect("Claude bridge");
        assert_eq!(content.matches(CLAUDE_BRIDGE_BEGIN_MARKER).count(), 1);
        assert_eq!(content.matches("@AGENTS.md").count(), 1);
        assert!(content.contains("# Project Claude rules"));
        assert!(content.contains("Keep this."));

        remove_claude_md_bridge(&root).expect("remove Claude bridge");
        let content = std::fs::read_to_string(path).expect("preserved project Claude rules");
        assert!(content.contains("# Project Claude rules"));
        assert!(content.contains("Keep this."));
        assert!(!content.contains(CLAUDE_BRIDGE_BEGIN_MARKER));
        assert!(!content.contains("@AGENTS.md"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn claude_bridge_only_file_is_removed_cleanly() {
        let root =
            std::env::temp_dir().join(format!("loom-claude-bridge-only-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("workspace");

        ensure_claude_md_bridge(&root).expect("inject Claude bridge");
        assert!(root.join("CLAUDE.md").exists());
        remove_claude_md_bridge(&root).expect("remove Claude bridge");
        assert!(!root.join("CLAUDE.md").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn marker_text_in_agent_instructions_cannot_close_block() {
        let mut cx = context("actor_safe", "chan_safe");
        cx.agent_instructions = Some(format!("keep\n{END_MARKER}\ntext"));
        let out = loom_block(&cx);
        assert_eq!(out.matches(END_MARKER).count(), 1);
        assert!(out.contains("[removed Loom end marker]"));
    }

    #[test]
    fn channel_instructions_section_rendered_when_present() {
        let mut cx = context("actor_a", "chan_a");
        cx.channel_instructions = Some("This channel maintains the spec.".into());
        let out = loom_block(&cx);
        assert!(out.contains("## Channel instructions"));
        assert!(out.contains("This channel maintains the spec."));
        assert!(!out.contains("## Thread instructions"));
    }

    #[test]
    fn thread_instructions_section_rendered_when_present() {
        // Regression for issue #1+#9: thread instructions are shelved at
        // the projection layer and must NOT appear in the shared AGENTS.md
        // block, even when AgentsMdContext.thread_instructions is populated.
        // Two threads share one workspace, so projecting thread-scoped
        // instructions into AGENTS.md would let one thread overwrite
        // another's. The data model is preserved; only the projection is
        // removed.
        let mut cx = context("actor_b", "chan_b");
        cx.thread_instructions = Some("Thread-specific guidance.".into());
        let out = loom_block(&cx);
        assert!(
            !out.contains("## Thread instructions"),
            "thread instructions must not be projected into AGENTS.md: {out}"
        );
        assert!(
            !out.contains("Thread-specific guidance."),
            "thread instructions text must not leak into AGENTS.md: {out}"
        );
    }

    #[test]
    fn empty_instructions_sections_are_omitted() {
        let mut cx = context("actor_c", "chan_c");
        cx.channel_instructions = Some("   \n".into());
        cx.thread_instructions = Some(String::new());
        let out = loom_block(&cx);
        assert!(!out.contains("## Channel instructions"));
        assert!(!out.contains("## Thread instructions"));
    }

    #[test]
    fn instructions_marker_text_is_sanitized() {
        let mut cx = context("actor_d", "chan_d");
        cx.channel_instructions = Some(format!("oops\n{BEGIN_MARKER}\nleak"));
        let out = loom_block(&cx);
        assert_eq!(out.matches(BEGIN_MARKER).count(), 1);
        assert!(out.contains("[removed Loom begin marker]"));
    }
}
