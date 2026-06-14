import { Fragment } from "react";
import type { Actor, Channel, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import { isHiddenProtocolMessage } from "@/lib/message-utils";
import { groupMessagesByDate } from "@/lib/message-utils";
import { displayName } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { useStickToBottomScroll } from "@/hooks/useStickToBottomScroll";
import { X, Split, MessageSquare } from "lucide-react";
import { EmptyState } from "@/components/shared/EmptyState";
import { MutedLine } from "@/components/shared/MutedLine";
import { TaskStateBadge } from "@/components/chat/TaskStateBadge";
import { ThreadConversationMessage } from "@/components/chat/ThreadConversationMessage";
import { ThreadComposer } from "@/components/chat/ThreadComposer";

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
  onSend: () => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
}) {
  const rootMessage = thread
    ? channelMessages.find((message) => message.id === thread.rootMessageId) ?? null
    : null;
  const replyMessages = messages.filter(
    (message) =>
      !isHiddenProtocolMessage(message) &&
      (!rootMessage || message.id !== rootMessage.id),
  );
  const replyGroups = groupMessagesByDate(replyMessages);
  const starter = rootMessage ? actors[rootMessage.authorActorId] : undefined;
  const threadScrollKey = thread?.id ?? "thread:none";
  const threadContentKey = [
    rootMessage
      ? `${rootMessage.id}:${rootMessage.createdAt}:${rootMessage.body.length}`
      : "root:none",
    ...replyMessages.map(
      (message) => `${message.id}:${message.createdAt}:${message.body.length}`,
    ),
  ].join("|");
  const threadScroll = useStickToBottomScroll({
    contentKey: threadContentKey,
    itemCount: replyMessages.length + (rootMessage ? 1 : 0),
    scrollKey: threadScrollKey,
  });
  return (
    <aside
      className={cn(
        "min-h-0 min-w-0 flex-col bg-white",
        className ?? "hidden border-l border-[#e2e6ef] xl:flex",
      )}
    >
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
            <button className="composer-icon" type="button" title="Close" onClick={onClose}>
              <X size={16} />
            </button>
          </div>
        </div>
      </div>

      <div
        ref={threadScroll.ref}
        className="min-h-0 flex-1 overflow-y-auto bg-white soft-scrollbar"
        onScroll={threadScroll.onScroll}
      >
        {!thread ? (
          <div className="p-4">
            <EmptyState icon={Split} text="Select a thread." />
          </div>
        ) : (
          <div>
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

            <section className="bg-white px-5 py-2">
              {replyMessages.length === 0 ? (
                <div className="py-8">
                  <EmptyState icon={MessageSquare} text="No replies in this thread." />
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  {replyGroups.map((group) => (
                    <Fragment key={group.key}>
                      <div className="date-divider px-0">
                        <span />
                        <div>{group.label}</div>
                        <span />
                      </div>
                      {group.messages.map((message) => (
                        <ThreadConversationMessage
                          key={message.id}
                          actor={actors[message.authorActorId]}
                          actors={actors}
                          busy={busy}
                          currentActorId={currentActorId}
                          machines={machines}
                          runs={runs}
                          message={message}
                          onOpenAgentSettings={onOpenAgentSettings}
                          onToggleReaction={onToggleReaction}
                        />
                      ))}
                    </Fragment>
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
      </div>

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
