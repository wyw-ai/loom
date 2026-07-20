import {
  useEffect,
  useState,
} from "react";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { AvatarStack } from "@/components/agent/AvatarStack";
import { EmptyState } from "@/components/shared/EmptyState";
import { MutedLine } from "@/components/shared/MutedLine";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { actorKindLabel, channelPanelDetail, channelPanelTitle, getActorRunContext, memberPresence, runStatusDotClass, runStatusFullLabel } from "@/lib/agent-utils";
import { canAddChannelMember, canRemoveChannelMember, channelTaskGroups, taskProgressPercent } from "@/lib/channel-utils";
import { displayName, errorText, fallbackActor, findAgentMemberEntry, formatShortDateTime, machineCanRunCommands, shortActorAlias, statusDotClass, taskStatusBadgeClass } from "@/lib/format-utils";
import { metadataString, metadataText, threadLastReplyLabel, threadParticipants, threadReplyCount } from "@/lib/message-utils";
import { cn, formatTime, shortId } from "@/lib/utils";
import { Check, ChevronLeft, Clock, Folder, FolderCog, HardDrive, Hash, Loader2, Plus, RefreshCw, Search, Split, UserPlus, Users, X } from "lucide-react";
import type { Actor, Channel, ChannelMemberConfig, MachineDirListResult, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ChannelMemberPanelItem, ChannelMemberPresence, ChannelPanelTab, ThreadActivityStats } from "@/lib/types";
import * as ipc from "@/ipc/bridge";

export function ChannelPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
  channelMemberConfigs,
  channelTasks,
  channelThreads,
  currentActorId,
  machines,
  runs,
  threadStatsById,
  tab,
  busy,
  onClose,
  onSelectTab,
  onSelectThread,
  onInviteMember,
  onRemoveMember,
  onSaveMemberWorkspace,
  onClearMemberWorkspace,
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
  channelMemberConfigs: Record<string, ChannelMemberConfig>;
  channelTasks: Task[];
  channelThreads: Thread[];
  currentActorId: string | null;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  threadStatsById: Record<string, ThreadActivityStats>;
  tab: ChannelPanelTab;
  busy: string | null;
  onClose: () => void;
  onSelectTab: (tab: ChannelPanelTab) => void;
  onSelectThread: (thread: Thread) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onSaveMemberWorkspace: (channelId: string, actorId: string, workspaceDir: string) => Promise<boolean>;
  onClearMemberWorkspace: (channelId: string, actorId: string) => Promise<boolean>;
}) {
  return (
    <ChannelDetailPanel
      actors={actors}
      memberCandidates={memberCandidates}
      channel={channel}
      channelMessages={channelMessages}
      channelMemberConfigs={channelMemberConfigs}
      channelTasks={channelTasks}
      channelThreads={channelThreads}
      currentActorId={currentActorId}
      machines={machines}
      runs={runs}
      threadStatsById={threadStatsById}
      tab={tab}
      busy={busy}
      className="hidden min-h-0 min-w-0 flex-col bg-[#fbfbfd] xl:flex"
      onClose={onClose}
      onSelectTab={onSelectTab}
      onSelectThread={onSelectThread}
      onInviteMember={onInviteMember}
      onRemoveMember={onRemoveMember}
      onSaveMemberWorkspace={onSaveMemberWorkspace}
      onClearMemberWorkspace={onClearMemberWorkspace}
    />
  );
}


export function ChannelDetailPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
  channelMemberConfigs,
  channelTasks,
  channelThreads,
  currentActorId,
  machines,
  runs,
  threadStatsById,
  tab,
  busy,
  className,
  onClose,
  onSelectTab,
  onSelectThread,
  onInviteMember,
  onRemoveMember,
  onSaveMemberWorkspace,
  onClearMemberWorkspace,
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
  channelMemberConfigs: Record<string, ChannelMemberConfig>;
  channelTasks: Task[];
  channelThreads: Thread[];
  currentActorId: string | null;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  threadStatsById: Record<string, ThreadActivityStats>;
  tab: ChannelPanelTab;
  busy: string | null;
  className?: string;
  onClose: () => void;
  onSelectTab: (tab: ChannelPanelTab) => void;
  onSelectThread: (thread: Thread) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onSaveMemberWorkspace: (channelId: string, actorId: string, workspaceDir: string) => Promise<boolean>;
  onClearMemberWorkspace: (channelId: string, actorId: string) => Promise<boolean>;
}) {
  const [memberQuery, setMemberQuery] = useState("");
  const members = channel
    ? channel.members.map((actorId) => actors[actorId] ?? fallbackActor(actorId))
    : [];
  const availableMembers = channel
    ? memberCandidates.filter((actor) => canAddChannelMember(channel, actor))
    : [];
  const filteredAvailableMembers = availableMembers.filter((actor) => {
    const query = memberQuery.trim().toLowerCase();
    if (!query) return true;
    return `${displayName(actor)} ${actor.id} ${actor.kind}`.toLowerCase().includes(query);
  });
  const rootMessagesById = new Map(channelMessages.map((message) => [message.id, message]));
  const memberRows = members.map((actor) => ({
    actor,
    presence: memberPresence(actor, machines, currentActorId),
  }));
  const onlineMembers = memberRows.filter((item) => item.presence.online);
  const offlineMembers = memberRows.filter((item) => !item.presence.online);
  const panelTitle = channelPanelTitle(tab);
  const panelDetail = channel
    ? `#${channel.title} · ${channelPanelDetail(tab, channelThreads.length, members.length, channelTasks.length)}`
    : "Not connected";
  const tabs: Array<{ id: ChannelPanelTab; label: string; count: number }> = [
    { id: "threads", label: "Threads", count: channelThreads.length },
    { id: "members", label: "Members", count: members.length },
    { id: "tasks", label: "Tasks", count: channelTasks.length },
  ];
  return (
    <aside className={cn("min-h-0 min-w-0 flex-col bg-[#fbfbfd]", className ?? "flex")}>
      <div className="border-b border-[#edf0f5] bg-white p-5">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-[#6784f4] to-[#4d3ed7] text-white shadow-sm">
              {tab === "threads" ? (
                <Split size={24} />
              ) : tab === "members" ? (
                <Users size={24} />
              ) : (
                <Check size={24} />
              )}
            </div>
            <div className="min-w-0">
              <div className="truncate text-lg font-bold text-[#111827]">
                {panelTitle}
              </div>
              <div className="mt-1 truncate text-sm text-[#485063]">
                {channel
                  ? `#${channel.title} · ${panelDetail}`
                  : "Not connected"}
              </div>
            </div>
          </div>
          <button className="composer-icon" type="button" title="Close panel" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="grid h-12 shrink-0 grid-cols-3 border-b border-[#edf0f5] bg-white px-5">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            className={cn(
              "relative text-sm font-semibold capitalize text-[#667085]",
              tab === item.id && "text-[#503ed4]",
            )}
            onClick={() => onSelectTab(item.id)}
          >
            {item.label}
            {item.count > 0 && (
              <span className="ml-1 rounded-full bg-[#f1efff] px-1.5 py-0.5 text-[10px] text-[#5843d7]">
                {item.count}
              </span>
            )}
            {tab === item.id && (
              <span className="absolute inset-x-1 bottom-0 h-0.5 rounded-full bg-[#503ed4]" />
            )}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        {!channel ? (
          <EmptyState icon={Hash} text="Select a channel." />
        ) : tab === "threads" ? (
          <ChannelThreadsPanel
            actors={actors}
            rootMessagesById={rootMessagesById}
            threadStatsById={threadStatsById}
            threads={channelThreads}
            onSelectThread={onSelectThread}
          />
        ) : tab === "members" ? (
          <ChannelMembersPanel
            availableMembers={availableMembers}
            busy={busy}
            channel={channel}
            channelMemberConfigs={channelMemberConfigs}
            filteredAvailableMembers={filteredAvailableMembers}
            memberQuery={memberQuery}
            machines={machines}
            offlineMembers={offlineMembers}
            onlineMembers={onlineMembers}
            runs={runs}
            setMemberQuery={setMemberQuery}
            onInviteMember={onInviteMember}
            onRemoveMember={onRemoveMember}
            onSaveMemberWorkspace={onSaveMemberWorkspace}
            onClearMemberWorkspace={onClearMemberWorkspace}
          />
        ) : (
          <ChannelTasksPanel actors={actors} tasks={channelTasks} />
        )}
      </div>
    </aside>
  );
}


export function ChannelThreadsPanel({
  actors,
  rootMessagesById,
  threadStatsById,
  threads,
  onSelectThread,
}: {
  actors: Record<string, Actor>;
  rootMessagesById: Map<string, Message>;
  threadStatsById: Record<string, ThreadActivityStats>;
  threads: Thread[];
  onSelectThread: (thread: Thread) => void;
}) {
  if (threads.length === 0) {
    return <EmptyState icon={Split} text="No threads in this channel." />;
  }
  return (
    <div className="space-y-3">
      {threads.map((thread) => (
        <ChannelThreadCard
          key={thread.id}
          actors={actors}
          rootMessage={rootMessagesById.get(thread.rootMessageId) ?? null}
          thread={thread}
          threadStats={threadStatsById[thread.id]}
          onSelect={() => onSelectThread(thread)}
        />
      ))}
    </div>
  );
}


export function ChannelThreadCard({
  actors,
  rootMessage,
  thread,
  threadStats,
  onSelect,
}: {
  actors: Record<string, Actor>;
  rootMessage: Message | null;
  thread: Thread;
  threadStats?: ThreadActivityStats;
  onSelect: () => void;
}) {
  const starter = rootMessage ? actors[rootMessage.authorActorId] : undefined;
  const participants = threadParticipants(thread, actors, starter, threadStats);
  const replyCount = threadReplyCount(thread, threadStats);
  const lastReply = threadLastReplyLabel(thread, threadStats);
  const preview = rootMessage
    ? rootMessage.body || metadataText(rootMessage)
    : `Started from ${shortId(thread.rootMessageId)}`;
  const replyLabel =
    typeof replyCount === "number"
      ? `${replyCount}${threadStats?.hasMoreReplies ? "+" : ""} replies`
      : "Thread";
  return (
    <button
      type="button"
      className="w-full rounded-xl border border-[#e2e6ef] bg-white p-4 text-left shadow-[0_1px_2px_rgb(16_24_40_/_0.03)] transition hover:border-[#cdd3e5] hover:shadow-[0_10px_24px_rgb(16_24_40_/_0.07)]"
      onClick={onSelect}
    >
      <div className="flex items-start gap-3">
        <span className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
          <Split size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-start justify-between gap-2">
            <span className="min-w-0">
              <span className="block truncate text-sm font-bold text-[#111827]">
                {thread.title}
              </span>
              <span className="mt-1 line-clamp-2 text-xs leading-5 text-[#596174]">
                {starter ? `${displayName(starter)}: ` : ""}
                {preview}
              </span>
            </span>
            <span className="shrink-0 text-xs font-medium text-[#8a93a5]">
              {rootMessage ? formatTime(rootMessage.createdAt) : shortId(thread.id, 5)}
            </span>
          </span>
          <span className="mt-3 flex min-w-0 items-center gap-2">
            <AvatarStack actors={participants} max={4} small />
            <span className="min-w-0 truncate text-xs font-bold text-[#503ed4]">
              {replyLabel}
            </span>
            {lastReply && (
              <span className="shrink-0 text-xs font-medium text-[#667085]">
                Last {lastReply}
              </span>
            )}
          </span>
        </span>
      </div>
    </button>
  );
}


export function ChannelMembersPanel({
  availableMembers,
  busy,
  channel,
  channelMemberConfigs,
  filteredAvailableMembers,
  memberQuery,
  machines,
  offlineMembers,
  onlineMembers,
  runs,
  setMemberQuery,
  onInviteMember,
  onRemoveMember,
  onSaveMemberWorkspace,
  onClearMemberWorkspace,
}: {
  availableMembers: Actor[];
  busy: string | null;
  channel: Channel;
  channelMemberConfigs: Record<string, ChannelMemberConfig>;
  filteredAvailableMembers: Actor[];
  memberQuery: string;
  machines: MachineInfo[];
  offlineMembers: ChannelMemberPanelItem[];
  onlineMembers: ChannelMemberPanelItem[];
  runs: Record<string, Run>;
  setMemberQuery: (value: string) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onSaveMemberWorkspace: (channelId: string, actorId: string, workspaceDir: string) => Promise<boolean>;
  onClearMemberWorkspace: (channelId: string, actorId: string) => Promise<boolean>;
}) {
  const [editingWorkspaceActor, setEditingWorkspaceActor] = useState<Actor | null>(null);
  return (
    <div className="space-y-4">
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        channelMemberConfigs={channelMemberConfigs}
        items={onlineMembers}
        runs={runs}
        title="Online"
        onRemoveMember={onRemoveMember}
        onEditMemberWorkspace={setEditingWorkspaceActor}
      />
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        channelMemberConfigs={channelMemberConfigs}
        items={offlineMembers}
        runs={runs}
        title="Offline"
        onRemoveMember={onRemoveMember}
        onEditMemberWorkspace={setEditingWorkspaceActor}
      />
      {onlineMembers.length === 0 && offlineMembers.length === 0 && (
        <MutedLine>No explicit members.</MutedLine>
      )}
      <div className="member-picker-card">
        <div className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[#667085]">
          <UserPlus size={13} />
          Add member
        </div>
        <label className="mb-3 flex h-9 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-[#667085]">
          <Search size={14} />
          <input
            value={memberQuery}
            onChange={(event) => setMemberQuery(event.target.value)}
            placeholder="Search people and agents"
            className="min-w-0 flex-1 bg-transparent text-sm text-[#303849] outline-none placeholder:text-[#98a2b3]"
          />
        </label>
        <div className="space-y-2">
          {filteredAvailableMembers.length === 0 ? (
            <MutedLine>
              {availableMembers.length === 0
                ? "No candidates available."
                : "No matching candidates."}
            </MutedLine>
          ) : (
            filteredAvailableMembers.slice(0, 8).map((actor) => {
              const inviteBusy = busy === `channel:invite:${channel.id}:${actor.id}`;
              return (
                <div key={actor.id} className="member-candidate-row">
                  <ActorAvatar actor={actor} fallback={actor.id} small />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-semibold text-[#303849]">
                      {displayName(actor)}
                    </div>
                    <div className="truncate text-xs text-[#667085]">
                      {actorKindLabel(actor)} · {shortActorAlias(actor.id)}
                    </div>
                  </div>
                  <Button
                    size="sm"
                    className="h-8 rounded-lg bg-[#503ed4] px-3 text-white hover:bg-[#4635c5]"
                    disabled={inviteBusy}
                    onClick={() => onInviteMember(channel.id, actor.id)}
                  >
                    {inviteBusy ? (
                      <Loader2 className="animate-spin" size={13} />
                    ) : (
                      <Plus size={13} />
                    )}
                    Add
                  </Button>
                </div>
              );
            })
          )}
        </div>
      </div>
      {editingWorkspaceActor && (
        <ChannelMemberWorkspaceDialog
          key={`${channel.id}:${editingWorkspaceActor.id}`}
          actor={editingWorkspaceActor}
          busy={busy}
          channel={channel}
          config={channelMemberConfigs[editingWorkspaceActor.id] ?? null}
          machines={machines}
          onCancel={() => setEditingWorkspaceActor(null)}
          onSave={onSaveMemberWorkspace}
          onClear={onClearMemberWorkspace}
        />
      )}
    </div>
  );
}


export function ChannelMemberGroup({
  busy,
  channel,
  channelMemberConfigs,
  items,
  runs,
  title,
  onRemoveMember,
  onEditMemberWorkspace,
}: {
  busy: string | null;
  channel: Channel;
  channelMemberConfigs: Record<string, ChannelMemberConfig>;
  items: ChannelMemberPanelItem[];
  runs: Record<string, Run>;
  title: string;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onEditMemberWorkspace: (actor: Actor) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section className="space-y-2">
      <div className="flex items-center gap-2 px-1 text-xs font-bold text-[#596174]">
        <span>{title}</span>
        <span className="rounded-full bg-[#eef0f6] px-1.5 py-0.5 text-[10px] text-[#667085]">
          {items.length}
        </span>
      </div>
      <div className="space-y-2">
        {items.map(({ actor, presence }) => (
          <ChannelMemberRow
            key={actor.id}
            actor={actor}
            busy={busy}
            channel={channel}
            config={channelMemberConfigs[actor.id] ?? null}
            presence={presence}
            runs={runs}
            onRemoveMember={onRemoveMember}
            onEditMemberWorkspace={onEditMemberWorkspace}
          />
        ))}
      </div>
    </section>
  );
}


export function ChannelMemberRow({
  actor,
  busy,
  channel,
  config,
  presence,
  runs,
  onRemoveMember,
  onEditMemberWorkspace,
}: {
  actor: Actor;
  busy: string | null;
  channel: Channel;
  config: ChannelMemberConfig | null;
  presence: ChannelMemberPresence;
  runs: Record<string, Run>;
  onRemoveMember: (channelId: string, actorId: string) => void;
  onEditMemberWorkspace: (actor: Actor) => void;
}) {
  const revokeBusy = busy === `channel:revoke:${channel.id}:${actor.id}`;
  const workspaceBusy = busy === `channel:member-workspace:${channel.id}:${actor.id}`;
  const hasCustomWorkspace = Boolean(config?.workspaceDir?.trim());
  const ctx = actor.kind === "agent" ? getActorRunContext(runs, actor.id) : null;
  const runLabel = runStatusFullLabel(ctx);

  return (
    <div className="flex items-center gap-3 rounded-xl border border-[#edf0f5] bg-white px-3 py-2.5 shadow-[0_1px_2px_rgb(16_24_40_/_0.03)]">
      <span className="relative shrink-0">
        <ActorAvatar actor={actor} fallback={actor.id} small />
        <span
          className={cn(
            "absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-white",
            ctx ? runStatusDotClass(ctx) : statusDotClass(presence.status),
          )}
        />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <div className="truncate text-sm font-semibold text-[#303849]">
            {displayName(actor)}
          </div>
          <span className="shrink-0 rounded-md bg-[#f5f3ff] px-1.5 py-0.5 text-[10px] font-bold text-[#6652e8]">
            {actorKindLabel(actor)}
          </span>
        </div>
        <div className="truncate text-xs text-[#667085]">
          {runLabel ?? (
            <>
              {presence.label}
              {actor.kind === "agent" && presence.status !== "offline"
                ? ` · ${presence.status}`
                : ""}
            </>
          )}
        </div>
      </div>
      {actor.kind === "agent" && (
        <button
          type="button"
          title={`Workspace: ${hasCustomWorkspace ? "Custom" : "Default"}`}
          disabled={workspaceBusy}
          onClick={() => onEditMemberWorkspace(actor)}
          className={cn(
            "composer-icon h-7 min-w-7 text-[#667085]",
            hasCustomWorkspace && "bg-[#eefaf4] text-[#087443] hover:bg-[#dcf5e8] hover:text-[#05603a]",
          )}
        >
          {workspaceBusy ? <Loader2 className="animate-spin" size={13} /> : <FolderCog size={13} />}
        </button>
      )}
      {canRemoveChannelMember(channel, actor.id) && (
        <button
          type="button"
          title={`Remove ${displayName(actor)}`}
          disabled={revokeBusy}
          onClick={() => onRemoveMember(channel.id, actor.id)}
          className="composer-icon h-7 min-w-7 text-[#667085]"
        >
          {revokeBusy ? <Loader2 className="animate-spin" size={13} /> : <X size={13} />}
        </button>
      )}
    </div>
  );
}

function ChannelMemberWorkspaceDialog({
  actor,
  busy,
  channel,
  config,
  machines,
  onCancel,
  onSave,
  onClear,
}: {
  actor: Actor;
  busy: string | null;
  channel: Channel;
  config: ChannelMemberConfig | null;
  machines: MachineInfo[];
  onCancel: () => void;
  onSave: (channelId: string, actorId: string, workspaceDir: string) => Promise<boolean>;
  onClear: (channelId: string, actorId: string) => Promise<boolean>;
}) {
  const initialPath = config?.workspaceDir?.trim() ?? "";
  const [mode, setMode] = useState<"default" | "custom">(initialPath ? "custom" : "default");
  const [path, setPath] = useState(initialPath);
  const [browser, setBrowser] = useState<MachineDirListResult | null>(null);
  const [browserBusy, setBrowserBusy] = useState(false);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const saving = busy === `channel:member-workspace:${channel.id}:${actor.id}`;
  const customPath = path.trim();
  const canSave = mode === "default" || customPath.length > 0;
  const agentEntry = findAgentMemberEntry(machines, actor.id);
  const browseMachine = agentEntry?.machine ?? machineForActorMeta(actor, machines);
  const canBrowseRemote = Boolean(
    browseMachine &&
      machineCanRunCommands(browseMachine) &&
      browseMachine.capabilities.includes("fs.dir.list"),
  );

  useEffect(() => {
    if (mode !== "default") return;
    setBrowser(null);
    setBrowserError(null);
  }, [mode]);

  async function save() {
    if (!canSave || saving) return;
    const ok =
      mode === "default"
        ? await onClear(channel.id, actor.id)
        : await onSave(channel.id, actor.id, customPath);
    if (ok) onCancel();
  }

  async function browseDirectory(nextPath?: string | null) {
    if (!browseMachine || !canBrowseRemote || browserBusy) return;
    setMode("custom");
    setBrowserBusy(true);
    setBrowserError(null);
    try {
      const result = await ipc.machineDirList({
        machineId: browseMachine.id,
        path: nextPath?.trim() || undefined,
      });
      setBrowser(result);
      setPath(result.path);
    } catch (err) {
      setBrowserError(errorText(err));
    } finally {
      setBrowserBusy(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label={`Workspace for ${displayName(actor)}`}
      onMouseDown={onCancel}
    >
      <div
        className="w-full max-w-2xl rounded-lg border border-[#dfe3ec] bg-white p-4 shadow-soft"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="mb-4 flex min-w-0 items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="truncate text-sm font-bold text-[#111827]">Workspace</div>
            <div className="mt-0.5 truncate text-xs text-[#667085]">
              {displayName(actor)} · #{channel.title}
            </div>
          </div>
          <button className="composer-icon h-7 min-w-7" type="button" title="Close" onClick={onCancel}>
            <X size={13} />
          </button>
        </div>

        <div className="mb-3 grid h-9 grid-cols-2 overflow-hidden rounded-md border border-[#dfe3ec] bg-[#f8f9fc] p-0.5">
          {(["default", "custom"] as const).map((item) => (
            <button
              key={item}
              type="button"
              className={cn(
                "rounded-[5px] text-xs font-bold capitalize text-[#667085]",
                mode === item && "bg-white text-[#303849] shadow-sm",
              )}
              onClick={() => setMode(item)}
            >
              {item}
            </button>
          ))}
        </div>

        {mode === "custom" && (
          <div className="space-y-2">
            <label className="block">
              <span className="mb-1 block text-xs font-bold text-[#596174]">Path</span>
              <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                <Input
                  value={path}
                  onChange={(event) => setPath(event.target.value)}
                  placeholder="F:\\code\\project"
                  className="font-mono text-xs"
                  autoFocus
                />
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  disabled={!canBrowseRemote || browserBusy}
                  title={
                    canBrowseRemote
                      ? "Browse remote folders"
                      : "Remote folder browsing is unavailable"
                  }
                  onClick={() => void browseDirectory(customPath || undefined)}
                >
                  {browserBusy ? <Loader2 className="animate-spin" size={13} /> : <Folder size={13} />}
                  Browse
                </Button>
              </div>
            </label>
            {canBrowseRemote ? (
              <RemoteDirectoryBrowser
                browser={browser}
                busy={browserBusy}
                error={browserError}
                onOpenPath={(nextPath) => void browseDirectory(nextPath)}
              />
            ) : (
              <div className="rounded-md border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2 text-xs text-[#667085]">
                Remote browsing is available after the target daemon supports fs.dir.list.
              </div>
            )}
          </div>
        )}

        <div className="mt-4 flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onCancel}>
            Cancel
          </Button>
          <Button size="sm" disabled={!canSave || saving} onClick={save}>
            {saving ? <Loader2 className="animate-spin" size={13} /> : null}
            Save
          </Button>
        </div>
      </div>
    </div>
  );
}

function machineForActorMeta(actor: Actor, machines: MachineInfo[]): MachineInfo | null {
  const machineId = metadataString(actor._meta, ["machineId", "machine_id"]);
  if (!machineId) return null;
  return machines.find((machine) => machine.id === machineId) ?? null;
}

function RemoteDirectoryBrowser({
  browser,
  busy,
  error,
  onOpenPath,
}: {
  browser: MachineDirListResult | null;
  busy: boolean;
  error: string | null;
  onOpenPath: (path?: string | null) => void;
}) {
  const entries = browser?.entries ?? [];
  return (
    <div className="rounded-lg border border-[#edf0f5] bg-[#fbfbfd] p-2">
      <div className="mb-2 flex min-w-0 items-center gap-2">
        <div className="min-w-0 flex-1 truncate rounded-md bg-white px-2 py-1.5 font-mono text-xs text-[#303849]">
          {browser?.path ?? "No remote folder loaded"}
        </div>
        <button
          type="button"
          title="Refresh"
          disabled={busy || !browser}
          className="composer-icon h-8 min-w-8 text-[#667085]"
          onClick={() => onOpenPath(browser?.path)}
        >
          {busy ? <Loader2 className="animate-spin" size={14} /> : <RefreshCw size={14} />}
        </button>
      </div>

      {browser && browser.roots.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-1.5">
          {browser.roots.map((root) => (
            <button
              key={`${root.label}:${root.path}`}
              type="button"
              title={root.path}
              disabled={busy}
              className="inline-flex h-7 max-w-[180px] items-center gap-1 rounded-md border border-[#dfe3ec] bg-white px-2 text-xs font-semibold text-[#596174] hover:border-[#cdd3e5]"
              onClick={() => onOpenPath(root.path)}
            >
              <HardDrive size={12} />
              <span className="truncate">{root.label}</span>
            </button>
          ))}
        </div>
      )}

      <div className="max-h-56 overflow-y-auto rounded-md border border-[#edf0f5] bg-white p-1 soft-scrollbar">
        {browser?.parent && (
          <button
            type="button"
            disabled={busy}
            className="flex h-8 w-full min-w-0 items-center gap-2 rounded px-2 text-left text-xs font-semibold text-[#596174] hover:bg-[#f5f7fb]"
            onClick={() => onOpenPath(browser.parent)}
          >
            <ChevronLeft size={13} />
            <span className="truncate">..</span>
          </button>
        )}
        {!browser && !busy ? (
          <div className="px-2 py-6 text-center text-xs text-[#667085]">Open a remote folder.</div>
        ) : entries.length === 0 && !busy ? (
          <div className="px-2 py-6 text-center text-xs text-[#667085]">No child folders.</div>
        ) : (
          entries.map((entry) => (
            <button
              key={entry.path}
              type="button"
              disabled={busy}
              title={entry.path}
              className="grid h-8 w-full grid-cols-[auto_minmax(0,1fr)] items-center gap-2 rounded px-2 text-left text-xs text-[#303849] hover:bg-[#f5f7fb]"
              onClick={() => onOpenPath(entry.path)}
            >
              <Folder size={13} className="text-[#667085]" />
              <span className="truncate font-mono">{entry.name}</span>
            </button>
          ))
        )}
      </div>

      {browser?.truncated && (
        <div className="mt-1 px-1 text-xs text-[#8a93a5]">Showing the first 500 folders.</div>
      )}
      {error && <div className="mt-2 rounded-md bg-[#fff4f4] px-2 py-1.5 text-xs text-[#b42318]">{error}</div>}
    </div>
  );
}


export function ChannelTasksPanel({
  actors,
  tasks,
}: {
  actors: Record<string, Actor>;
  tasks: Task[];
}) {
  const groups = channelTaskGroups(tasks);
  if (tasks.length === 0) {
    return <EmptyState icon={Check} text="No tasks in this channel." />;
  }
  return (
    <div className="space-y-5">
      {groups.map((group) => (
        <section key={group.id} className="space-y-2">
          <div className="flex items-center gap-2 px-1 text-xs font-bold text-[#596174]">
            <span>{group.title}</span>
            <span className="rounded-full bg-[#eef0f6] px-1.5 py-0.5 text-[10px] text-[#667085]">
              {group.tasks.length}
            </span>
          </div>
          <div className="space-y-2">
            {group.tasks.map((task) => (
              <ChannelTaskCard key={task.id} actors={actors} task={task} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}


export function ChannelTaskCard({
  actors,
  task,
}: {
  actors: Record<string, Actor>;
  task: Task;
}) {
  const ownerId = task.ownerActorId || task.requesterActorId;
  const owner = ownerId ? actors[ownerId] ?? fallbackActor(ownerId) : null;
  const progress = taskProgressPercent(task);
  const done = task.status === "done";
  const active = task.status === "in_progress" || task.status === "waiting_review";
  return (
    <div className="rounded-xl border border-[#e2e6ef] bg-white p-3 shadow-[0_1px_2px_rgb(16_24_40_/_0.03)]">
      <div className="flex items-start gap-3">
        <span
          className={cn(
            "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md border",
            done
              ? "border-[#5b46e8] bg-[#5b46e8] text-white"
              : active
                ? "border-[#5b46e8] bg-[#f4f2ff] text-[#5b46e8]"
                : "border-[#b8bfce] bg-white text-transparent",
          )}
        >
          {done ? <Check size={12} /> : active ? <span className="h-2 w-2 rounded-full bg-current" /> : null}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="line-clamp-2 text-sm font-bold leading-5 text-[#111827]">
                {task.title}
              </div>
              <div className="mt-1 truncate text-xs text-[#667085]">
                Source {shortId(task.sourceMessageId)}
              </div>
            </div>
            <Badge
              variant="outline"
              title={task.id}
              className={cn(
                "shrink-0 whitespace-nowrap font-semibold",
                taskStatusBadgeClass(task.status),
              )}
            >
              Task #{task.number}
            </Badge>
          </div>
          {progress !== null && (
            <div className="mt-3 flex items-center gap-3">
              <span className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-[#e7e9f3]">
                <span
                  className="block h-full rounded-full bg-[#5b46e8]"
                  style={{ width: `${progress}%` }}
                />
              </span>
              <span className="w-9 text-right text-xs font-semibold text-[#667085]">
                {progress}%
              </span>
            </div>
          )}
          <div className="mt-3 flex items-center justify-between gap-2 text-xs text-[#667085]">
            <span className="flex min-w-0 items-center gap-1.5">
              <Clock size={13} />
              <span className="truncate">{formatShortDateTime(task.updatedAt)}</span>
            </span>
            {owner && (
              <span className="flex min-w-0 items-center gap-1.5">
                <ActorAvatar actor={owner} fallback={owner.id} small />
                <span className="max-w-[86px] truncate font-semibold text-[#485063]">
                  {displayName(owner)}
                </span>
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

