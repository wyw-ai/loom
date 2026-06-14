import {
  useState,
} from "react";
import { AvatarStack } from "@/components/agent/AvatarStack";
import { ThreadPanel } from "@/components/chat/ThreadPanel";
import { EmptyState } from "@/components/shared/EmptyState";
import { channelMentionAgentActors } from "@/lib/channel-utils";
import { displayName } from "@/lib/format-utils";
import { metadataText, threadParticipants, threadReplyCount } from "@/lib/message-utils";
import { cn, formatTime, shortId } from "@/lib/utils";
import { Hash, Search, Split } from "lucide-react";
import type { Actor, Channel, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ThreadActivityStats, ThreadWithChannel } from "@/lib/types";

export function ThreadsView({
  actors,
  channels,
  messages,
  machines,
  runs,
  threadMessages,
  threadStatsById,
  threads,
  activeChannelId,
  activeThread,
  activeThreadTask,
  currentActorId,
  threadDraft,
  setThreadDraft,
  onSelectThread,
  onCloseThread,
  onSendThreadMessage,
  onToggleReaction,
  onOpenAgentSettings,
  busy,
  disabled,
}: {
  actors: Record<string, Actor>;
  channels: Channel[];
  messages: Message[];
  machines: MachineInfo[];
  runs: Record<string, Run>;
  threadMessages: Message[];
  threadStatsById: Record<string, ThreadActivityStats>;
  threads: ThreadWithChannel[];
  activeChannelId: string | null;
  activeThread: Thread | null;
  activeThreadTask: Task | null;
  currentActorId: string | null;
  threadDraft: string;
  setThreadDraft: (value: string) => void;
  onSelectThread: (thread: Thread) => void;
  onCloseThread: () => void;
  onSendThreadMessage: () => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
  busy: string | null;
  disabled: boolean;
}) {
  const [query, setQuery] = useState("");
  const activeChannel = activeChannelId
    ? channels.find((channel) => channel.id === activeChannelId) ?? null
    : null;
  const filteredThreads = threads.filter((thread) => {
    const text = `${thread.title} ${thread.channel.title}`.toLowerCase();
    return text.includes(query.trim().toLowerCase());
  });
  const rootMessagesById = new Map(messages.map((message) => [message.id, message]));
  return (
    <section className="flex min-h-0 flex-1 flex-col bg-white">
      <div className="flex h-[96px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
        <div>
          <h1 className="text-[22px] font-bold text-[#111827]">All Threads</h1>
          <p className="mt-1 text-sm text-[#485063]">
            Track and resolve conversations across all channels.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <label className="search-pill h-10 w-[250px]">
            <Search size={16} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search threads"
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#8a93a5]"
            />
          </label>
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(420px,1fr)_460px] bg-[#fbfbfd]">
        <div className="min-h-0 overflow-y-auto border-r border-[#e2e6ef] p-4 soft-scrollbar">
          <div className="mb-4 flex items-center justify-between gap-2 text-xs font-semibold text-[#667085]">
            <span>{filteredThreads.length} threads</span>
            <span>Newest activity first</span>
          </div>
          <div className="space-y-2">
            {filteredThreads.map((thread) => (
              <ThreadListCard
                key={thread.id}
                actors={actors}
                rootMessage={rootMessagesById.get(thread.rootMessageId) ?? null}
                replyCount={
                  activeThread?.id === thread.id
                    ? threadMessages.length
                    : threadStatsById[thread.id]?.replyCount
                }
                selected={activeThread?.id === thread.id}
                thread={thread}
                threadStats={threadStatsById[thread.id]}
                onSelect={() => onSelectThread(thread)}
              />
            ))}
            {filteredThreads.length === 0 && <EmptyState icon={Split} text="No threads." />}
          </div>
        </div>
        <ThreadPanel
          actors={actors}
          channel={activeChannel}
          channelMessages={messages}
          currentActorId={currentActorId}
          disabled={disabled}
          draft={threadDraft}
          mentionAgents={
            activeChannel ? channelMentionAgentActors(activeChannel, actors) : []
          }
          machines={machines}
          runs={runs}
          messages={threadMessages}
          setDraft={setThreadDraft}
          task={activeThreadTask}
          thread={activeThread}
          busy={busy}
          className="flex border-l border-[#e2e6ef] xl:flex"
          onClose={onCloseThread}
          onSend={onSendThreadMessage}
          onToggleReaction={onToggleReaction}
          onOpenAgentSettings={onOpenAgentSettings}
        />
      </div>
    </section>
  );
}


export function ThreadListCard({
  actors,
  rootMessage,
  replyCount,
  selected,
  thread,
  threadStats,
  onSelect,
}: {
  actors: Record<string, Actor>;
  rootMessage: Message | null;
  replyCount?: number;
  selected: boolean;
  thread: ThreadWithChannel;
  threadStats?: ThreadActivityStats;
  onSelect: () => void;
}) {
  const starter = rootMessage ? actors[rootMessage.authorActorId] : null;
  const participants = threadParticipants(thread, actors, starter ?? undefined, threadStats);
  const effectiveReplyCount = replyCount ?? threadReplyCount(thread, threadStats);
  const preview = rootMessage
    ? rootMessage.body || metadataText(rootMessage)
    : "Open the conversation to review the latest replies.";
  return (
    <button
      type="button"
      className={cn("thread-list-card", selected && "thread-list-card-active")}
      onClick={onSelect}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-[#596174]">
            <Hash size={13} />
            <span className="truncate">{thread.channel.title}</span>
          </div>
          <div className="truncate text-base font-bold text-[#111827]">{thread.title}</div>
        </div>
        <span className="shrink-0 text-xs font-medium text-[#667085]">
          {rootMessage ? formatTime(rootMessage.createdAt) : "Thread"}
        </span>
      </div>
      <p className="mt-2 line-clamp-2 text-sm leading-5 text-[#485063]">
        {starter ? `${displayName(starter)}: ` : ""}
        {preview}
      </p>
      <div className="mt-3 flex items-center gap-2">
        <AvatarStack actors={participants} max={5} small />
        <span className="rounded-full bg-[#f1efff] px-2 py-1 text-xs font-bold text-[#5843d7]">
          {typeof effectiveReplyCount === "number"
            ? `${Math.max(0, effectiveReplyCount)}${threadStats?.hasMoreReplies ? "+" : ""} ${
                effectiveReplyCount === 1 ? "reply" : "replies"
              }`
            : participants.length > 0
              ? `${participants.length} ${
                  participants.length === 1 ? "participant" : "participants"
                }`
              : "Thread"}
        </span>
        <span className="rounded-full border border-[#dfe3ec] bg-white px-2 py-1 text-xs font-semibold text-[#667085]">
          {shortId(thread.rootMessageId, 5)}
        </span>
      </div>
    </button>
  );
}


