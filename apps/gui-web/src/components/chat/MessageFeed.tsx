import { Fragment, type ReactNode } from "react";
import type { Actor, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";
import { isHiddenProtocolMessage, isWorkflowMessage, canUseAsThreadRoot } from "@/lib/message-utils";
import { groupMessagesByDate } from "@/lib/message-utils";
import { useStickToBottomScroll } from "@/hooks/useStickToBottomScroll";
import { MessageRow } from "@/components/chat/MessageRow";

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
}) {
  const workflowSourceIds = new Set(
    messages.filter(isWorkflowMessage).map((message) => message.id),
  );
  const visibleMessages = messages.filter((message) => !isHiddenProtocolMessage(message));
  const messageGroups = groupMessagesByDate(visibleMessages);
  const messageListKey = visibleMessages
    .map((message) => `${message.id}:${message.createdAt}:${message.body.length}`)
    .join("|");
  const feedScroll = useStickToBottomScroll({
    contentKey: messageListKey,
    itemCount: visibleMessages.length,
    scrollKey: feedKey,
  });

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
    <div
      ref={feedScroll.ref}
      className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-2 soft-scrollbar"
      onScroll={feedScroll.onScroll}
    >
      <div className="mx-auto flex max-w-4xl flex-col gap-2">
        {messageGroups.map((group) => (
          <Fragment key={group.key}>
            <div className="date-divider">
              <span />
              <div>{group.label}</div>
              <span />
            </div>
            {group.messages.map((message) => {
              const threadSummary =
                channelThreads.find((thread) => thread.rootMessageId === message.id) ?? null;
              const sourceTask =
                message.scope.kind === "channel"
                  ? tasksBySourceMessageId[message.id] ?? null
                  : null;
              return (
                <MessageRow
                  key={message.id}
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
                  threadStats={
                    threadSummary ? threadStatsById[threadSummary.id] : undefined
                  }
                  sourceTask={sourceTask}
                  currentActorId={currentActorId}
                  busy={busy}
                />
              );
            })}
          </Fragment>
        ))}
      </div>
    </div>
  );
}
