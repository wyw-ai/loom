import { useCallback, useMemo, type ReactNode, Component } from "react";
import type { Actor, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";
import {
  isHiddenProtocolMessage,
  isWorkflowMessage,
  canUseAsThreadRoot,
  groupMessagesByDate,
} from "@/lib/message-utils";
import { MessageRow } from "@/components/chat/MessageRow";
import { FeedScrollManager } from "@/components/chat/FeedScrollManager";
import { cn } from "@/lib/utils";

/** Per-message error boundary: catches single-message render crashes and logs the message id. */
class MessageItemErrorBoundary extends Component<
  { messageId: string; children: ReactNode },
  { hasError: boolean; error: Error | null }
> {
  constructor(props: { messageId: string; children: ReactNode }) {
    super(props);
    this.state = { hasError: false, error: null };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }
  componentDidCatch(error: Error) {
    console.error(
      `[MessageItemErrorBoundary] Crash rendering message ${this.props.messageId}:`,
      error,
    );
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="mx-4 my-1 rounded border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
          ⚠️ Error rendering message {this.props.messageId.slice(0, 10)}…
          <br />
          <span className="opacity-70">{this.state.error?.message}</span>
        </div>
      );
    }
    return this.props.children;
  }
}

export type FeedItem =
  | { kind: "date-divider"; key: string; label: string }
  | { kind: "message"; message: Message };

export function MessageFeed({
  actors,
  allowReply = true,
  allowThreads = true,
  feedKey,
  machines,
  runs,
  messages,
  tasksBySourceMessageId,
  channelThreads,
  threadStatsById,
  emptyText,
  emptyAction,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  onOpenAgentSettings,
  currentActorId,
  busy,
  anchorMessageId,
}: {
  actors: Record<string, Actor>;
  allowReply?: boolean;
  allowThreads?: boolean;
  feedKey: string;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  messages: Message[];
  tasksBySourceMessageId: Record<string, Task>;
  channelThreads: Thread[];
  threadStatsById: Record<string, ThreadActivityStats>;
  emptyText: string;
  emptyAction?: ReactNode;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  onOpenAgentSettings: (actorId: string) => void;
  currentActorId: string | null;
  busy: string | null;
  anchorMessageId?: string | null;
}) {
  const workflowSourceIds = useMemo(
    () => new Set(messages.filter(isWorkflowMessage).map((m) => m.id)),
    [messages],
  );

  // Map rootMessageId -> thread so the per-row lookup in Virtuoso's
  // itemContent is O(1) instead of an O(n) `find` re-run on every visible row.
  const threadByRoot = useMemo(
    () => new Map(channelThreads.map((t) => [t.rootMessageId, t])),
    [channelThreads],
  );

  const visibleMessages = useMemo(
    () => messages.filter((m) => !isHiddenProtocolMessage(m)),
    [messages],
  );

  // Flatten date-grouped messages into a unified item list for Virtuoso
  const feedItems = useMemo<FeedItem[]>(() => {
    const groups = groupMessagesByDate(visibleMessages);
    const items: FeedItem[] = [];
    for (const group of groups) {
      items.push({ kind: "date-divider", key: `div-${group.key}`, label: group.label });
      for (const message of group.messages) {
        items.push({ kind: "message", message });
      }
    }
    return items;
  }, [visibleMessages]);

  const renderItem = useCallback(
    (index: number) => {
      const item = feedItems[index];
      if (!item) return null;
      if (item.kind === "date-divider") {
        return (
          <div className={`date-divider ${index === 0 ? "date-divider-first" : ""}`}>
            <span />
            <div>{item.label}</div>
            <span />
          </div>
        );
      }
      const message = item.message;
      const threadSummary = threadByRoot.get(message.id) ?? null;
      const sourceTask =
        message.scope.kind === "channel"
          ? tasksBySourceMessageId[message.id] ?? null
          : null;
      const isAnchor = message.id === anchorMessageId;
      return (
        <div
          data-search-anchor={isAnchor ? "true" : undefined}
          className={cn(
            isAnchor &&
              "relative z-[1] rounded-lg bg-amber-50/70 ring-2 ring-inset ring-amber-300",
          )}
        >
          <MessageItemErrorBoundary messageId={message.id}>
            <MessageRow
              actor={actors[message.authorActorId]}
              actors={actors}
              machines={machines}
              runs={runs}
              message={message}
              workflowSourceIds={workflowSourceIds}
              onReply={onReply}
              onStartThread={onStartThread}
              onToggleReaction={onToggleReaction}
              onAnswerAction={onAnswerAction}
              onOpenAgentSettings={onOpenAgentSettings}
              canReply={allowReply}
              canStartThread={allowThreads && canUseAsThreadRoot(message)}
              threadSummary={threadSummary}
              threadStats={threadSummary ? threadStatsById[threadSummary.id] : undefined}
              sourceTask={sourceTask}
              currentActorId={currentActorId}
              busy={busy}
            />
          </MessageItemErrorBoundary>
        </div>
      );
    },
    [
      feedItems,
      threadByRoot,
      tasksBySourceMessageId,
      actors,
      machines,
      runs,
      workflowSourceIds,
      onReply,
      onStartThread,
      onToggleReaction,
      onAnswerAction,
      onOpenAgentSettings,
      allowReply,
      allowThreads,
      currentActorId,
      busy,
      threadStatsById,
      anchorMessageId,
    ],
  );

  if (visibleMessages.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-white px-8 text-sm text-muted-foreground">
        <div className="w-full max-w-lg rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-8 py-10 text-center">
          <div className="text-sm font-semibold text-[#667085]">{emptyText}</div>
          {emptyAction}
        </div>
      </div>
    );
  }

  return (
    <FeedScrollManager
      feedKey={feedKey}
      feedItems={feedItems}
      renderItem={renderItem}
      anchorMessageId={anchorMessageId}
    />
  );
}
