import { useCallback, useMemo } from "react";
import type { Actor, Channel, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import { isHiddenProtocolMessage } from "@/lib/message-utils";
import { groupMessagesByDate } from "@/lib/message-utils";
import { displayName } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { X, Split, MessageSquare, FolderOpen } from "lucide-react";
import { EmptyState } from "@/components/shared/EmptyState";
import { MutedLine } from "@/components/shared/MutedLine";
import { TaskStateBadge } from "@/components/chat/TaskStateBadge";
import { ThreadConversationMessage } from "@/components/chat/ThreadConversationMessage";
import { ThreadComposer } from "@/components/chat/ThreadComposer";
import { ScopeTokenSummary } from "@/components/layout/ScopeTokenSummary";
import { FeedScrollManager } from "@/components/chat/FeedScrollManager";
import type { FeedItem } from "@/components/chat/MessageFeed";

type ReplyItem = FeedItem;

export function ThreadPanel({
  actors,
  channel,
  channelMessages,
  currentActorId,
  disabled,
  draft,
  mentionAgents,
  machines,
  runs,
  messages,
  setDraft,
  task,
  thread,
  busy,
  className,
  onClose,
  onSend,
  onToggleReaction,
  onOpenAgentSettings,
  onOpenFolder,
  scopeId,
}: {
  actors: Record<string, Actor>;
  channel: Channel | null;
  channelMessages: Message[];
  currentActorId: string | null;
  disabled: boolean;
  draft: string;
  mentionAgents: Actor[];
  machines: MachineInfo[];
  runs: Record<string, Run>;
  messages: Message[];
  setDraft: (value: string) => void;
  task: Task | null;
  thread: Thread | null;
  busy: string | null;
  className?: string;
  onClose: () => void;
  onSend: (attachments?: import("@/lib/attachment-utils").PendingAttachment[]) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
  onOpenFolder?: () => void;
  scopeId?: string | null;
}) {
  const rootMessage = thread
    ? channelMessages.find((message) => message.id === thread.rootMessageId) ?? null
    : null;
  const starter = rootMessage ? actors[rootMessage.authorActorId] : undefined;

  const replyMessages = useMemo(
    () =>
      messages.filter(
        (message) =>
          !isHiddenProtocolMessage(message) &&
          (!rootMessage || message.id !== rootMessage.id),
      ),
    [messages, rootMessage],
  );

  // Flatten grouped replies for Virtuoso
  const replyItems = useMemo<ReplyItem[]>(() => {
    const groups = groupMessagesByDate(replyMessages);
    const items: ReplyItem[] = [];
    for (const group of groups) {
      items.push({ kind: "date-divider", key: `div-${group.key}`, label: group.label });
      for (const message of group.messages) {
        items.push({ kind: "message", message });
      }
    }
    return items;
  }, [replyMessages]);

  const feedKey = thread ? `thread-${thread.id}` : "thread-empty";

  const renderItem = useCallback(
    (index: number) => {
      const item = replyItems[index];
      if (!item) return null;
      if (item.kind === "date-divider") {
        return (
          <div className={`date-divider px-0 ${index === 0 ? "date-divider-first" : ""}`}>
            <span />
            <div>{item.label}</div>
            <span />
          </div>
        );
      }
      return (
        <ThreadConversationMessage
          actor={actors[item.message.authorActorId]}
          actors={actors}
          busy={busy}
          currentActorId={currentActorId}
          machines={machines}
          runs={runs}
          message={item.message}
          onOpenAgentSettings={onOpenAgentSettings}
          onToggleReaction={onToggleReaction}
        />
      );
    },
    [replyItems, actors, busy, currentActorId, machines, runs, onOpenAgentSettings, onToggleReaction],
  );

  const headerRenderer = useCallback(
    () => (
      <section className="border-b border-[#edf0f5] bg-white px-5 py-4">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[#667085]">
          <Split size={13} />
          Original message
        </div>
        {rootMessage ? (
          <ThreadConversationMessage
            actor={starter}
            actors={actors}
            busy={busy}
            currentActorId={currentActorId}
            machines={machines}
            runs={runs}
            message={rootMessage}
            onOpenAgentSettings={onOpenAgentSettings}
            onToggleReaction={onToggleReaction}
            root
          />
        ) : (
          <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-4 py-6">
            <MutedLine>Original message unavailable.</MutedLine>
          </div>
        )}
      </section>
    ),
    [rootMessage, starter, actors, busy, currentActorId, machines, runs, onOpenAgentSettings, onToggleReaction],
  );

  return (
    <aside
      className={cn(
        "min-h-0 min-w-0 flex-col bg-white",
        className ?? "hidden border-l border-[#e2e6ef] xl:flex",
      )}
    >
      {/* Header */}
      <div className="flex min-h-[86px] shrink-0 items-center border-b border-[#e2e6ef] bg-white px-5 py-3">
        <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="min-w-0 truncate text-lg font-bold text-[#111827]">
              {thread?.title ?? "Thread"}
            </div>
            <div className="mt-0.5 truncate text-sm text-[#485063]">
              {thread
                ? starter
                  ? `Started by ${displayName(starter)} in #${channel?.title ?? "channel"}`
                  : `#${channel?.title ?? "channel"}`
                : "Select a thread"}
            </div>
            {task && (
              <div className="mt-2 flex">
                <TaskStateBadge task={task} />
              </div>
            )}
          </div>
          <div className="flex items-center gap-1">
            {/* L1/L2 thread token summary (AC-T2) - silent-hidden when null */}
            <ScopeTokenSummary scopeId={scopeId} actors={actors} />
            {onOpenFolder && thread && (
              <button
                className="composer-icon"
                type="button"
                title="View thread attachments"
                onClick={onOpenFolder}
              >
                <FolderOpen size={15} />
              </button>
            )}
            <button className="composer-icon" type="button" title="Close" onClick={onClose}>
              <X size={16} />
            </button>
          </div>
        </div>
      </div>

      {/* Body - root message header + virtualized replies via FeedScrollManager */}
      {!thread ? (
        <div className="flex min-h-0 flex-1 items-center justify-center p-4">
          <EmptyState icon={Split} text="Select a thread." />
        </div>
      ) : replyItems.length === 0 && !rootMessage ? (
        <div className="flex min-h-0 flex-1 items-center justify-center p-4">
          <EmptyState icon={MessageSquare} text="No replies in this thread." />
        </div>
      ) : (
        <FeedScrollManager
          feedKey={feedKey}
          feedItems={replyItems}
          renderItem={renderItem}
          headerRenderer={headerRenderer}
        />
      )}

      <ThreadComposer
        draft={draft}
        setDraft={setDraft}
        disabled={disabled || !thread}
        busy={busy === "thread:message:send"}
        mentionAgents={mentionAgents}
        onSend={onSend}
      />
    </aside>
  );
}
