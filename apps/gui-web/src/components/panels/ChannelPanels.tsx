import {
  useState,
} from "react";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { AvatarStack } from "@/components/agent/AvatarStack";
import { EmptyState } from "@/components/shared/EmptyState";
import { MutedLine } from "@/components/shared/MutedLine";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { actorKindLabel, channelPanelDetail, channelPanelTitle, getActorRunContext, memberPresence, runStatusDotClass, runStatusFullLabel } from "@/lib/agent-utils";
import { canAddChannelMember, canRemoveChannelMember, channelTaskGroups, taskProgressPercent } from "@/lib/channel-utils";
import { displayName, fallbackActor, formatShortDateTime, shortActorAlias, statusDotClass, taskStatusBadgeClass } from "@/lib/format-utils";
import { metadataText, threadLastReplyLabel, threadParticipants, threadReplyCount } from "@/lib/message-utils";
import { cn, formatTime, shortId } from "@/lib/utils";
import { Check, Clock, Hash, Loader2, Plus, Search, Split, UserPlus, Users, X } from "lucide-react";
import type { Actor, Channel, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ChannelMemberPanelItem, ChannelMemberPresence, ChannelPanelTab, ThreadActivityStats } from "@/lib/types";

export function ChannelPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
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
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
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
}) {
  return (
    <ChannelDetailPanel
      actors={actors}
      memberCandidates={memberCandidates}
      channel={channel}
      channelMessages={channelMessages}
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
    />
  );
}


export function ChannelDetailPanel({
  actors,
  memberCandidates,
  channel,
  channelMessages,
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
}: {
  actors: Record<string, Actor>;
  memberCandidates: Actor[];
  channel: Channel | null;
  channelMessages: Message[];
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
            filteredAvailableMembers={filteredAvailableMembers}
            memberQuery={memberQuery}
            offlineMembers={offlineMembers}
            onlineMembers={onlineMembers}
            runs={runs}
            setMemberQuery={setMemberQuery}
            onInviteMember={onInviteMember}
            onRemoveMember={onRemoveMember}
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
  filteredAvailableMembers,
  memberQuery,
  offlineMembers,
  onlineMembers,
  runs,
  setMemberQuery,
  onInviteMember,
  onRemoveMember,
}: {
  availableMembers: Actor[];
  busy: string | null;
  channel: Channel;
  filteredAvailableMembers: Actor[];
  memberQuery: string;
  offlineMembers: ChannelMemberPanelItem[];
  onlineMembers: ChannelMemberPanelItem[];
  runs: Record<string, Run>;
  setMemberQuery: (value: string) => void;
  onInviteMember: (channelId: string, actorId: string) => void;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  return (
    <div className="space-y-4">
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        items={onlineMembers}
        runs={runs}
        title="在线"
        onRemoveMember={onRemoveMember}
      />
      <ChannelMemberGroup
        busy={busy}
        channel={channel}
        items={offlineMembers}
        runs={runs}
        title="离线"
        onRemoveMember={onRemoveMember}
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
    </div>
  );
}


export function ChannelMemberGroup({
  busy,
  channel,
  items,
  runs,
  title,
  onRemoveMember,
}: {
  busy: string | null;
  channel: Channel;
  items: ChannelMemberPanelItem[];
  runs: Record<string, Run>;
  title: string;
  onRemoveMember: (channelId: string, actorId: string) => void;
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
            presence={presence}
            runs={runs}
            onRemoveMember={onRemoveMember}
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
  presence,
  runs,
  onRemoveMember,
}: {
  actor: Actor;
  busy: string | null;
  channel: Channel;
  presence: ChannelMemberPresence;
  runs: Record<string, Run>;
  onRemoveMember: (channelId: string, actorId: string) => void;
}) {
  const revokeBusy = busy === `channel:revoke:${channel.id}:${actor.id}`;
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


