import { cn, shortId, formatTime } from "@/lib/utils";
import { displayName, actorName } from "@/lib/format-utils";
import { messageKind, bodyPollFromMessage, actionChoices, metadataText } from "@/lib/message-utils";
import { isWorkflowMessage, isWorkflowResultMessage } from "@/lib/message-utils";
import type { Actor, MachineInfo, Message, Run, Task, Thread } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Reply, Split } from "lucide-react";
import { AgentMessageAvatar } from "@/components/agent/AgentMessageAvatar";
import { TaskStateBadge } from "@/components/chat/TaskStateBadge";
import { MessageMarkdown } from "@/components/chat/MessageMarkdown";
import { AttachmentStack } from "@/components/chat/AttachmentStack";
import { PollCard } from "@/components/chat/PollCard";
import { ReactionPicker } from "@/components/chat/ReactionPicker";
import { ThreadSummaryRow } from "@/components/chat/ThreadSummaryRow";
import { WorkflowEventRow, WorkflowResultRow } from "@/components/chat/WorkflowRows";

export function MessageRow({
  actor,
  actors,
  machines,
  runs,
  message,
  workflowSourceIds,
  onReply,
  onStartThread,
  onToggleReaction,
  onAnswerAction,
  onOpenAgentSettings,
  canReply,
  canStartThread,
  threadSummary,
  threadStats,
  sourceTask,
  currentActorId,
  busy,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  message: Message;
  workflowSourceIds: Set<string>;
  onReply: (message: Message) => void;
  onStartThread: (message: Message) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  onOpenAgentSettings: (actorId: string) => void;
  canReply: boolean;
  canStartThread: boolean;
  threadSummary: Thread | null;
  threadStats?: ThreadActivityStats;
  sourceTask: Task | null;
  currentActorId: string | null;
  busy: string | null;
}) {
  const actionRequest = messageKind(message) === "action.request";
  const bodyPoll = actionRequest ? null : bodyPollFromMessage(message);
  const choices = actionChoices(message);
  const pollChoices = choices.length > 0 ? choices : bodyPoll?.choices ?? [];
  const displayBody = bodyPoll?.question || message.body || metadataText(message);
  const reactions = message.reactions ?? [];
  const attachments = message.attachments ?? [];

  if (isWorkflowMessage(message)) {
    return <WorkflowEventRow actor={actor} actors={actors} message={message} />;
  }
  if (isWorkflowResultMessage(message, workflowSourceIds)) {
    return (
      <WorkflowResultRow
        actor={actor}
        machines={machines}
        runs={runs}
        message={message}
        onOpenAgentSettings={onOpenAgentSettings}
      />
    );
  }

  return (
    <article
      className={cn(
        "group rounded-xl px-4 py-3 transition-colors hover:bg-[#f7f8fb]",
        actionRequest && "border border-amber-300 bg-amber-50",
      )}
    >
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          runs={runs}
          onOpenAgentSettings={onOpenAgentSettings}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs font-medium text-[#667085]">{formatTime(message.createdAt)}</span>
            {sourceTask && <TaskStateBadge task={sourceTask} />}
            {message.parentMessageId && (
              <span className="font-mono text-xs text-muted-foreground">
                reply {shortId(message.parentMessageId)}
              </span>
            )}
          </div>
          <div className="message-markdown mt-1 max-w-none break-words text-[15px] leading-6 text-[#111827]">
            <MessageMarkdown
              actors={actors}
              body={displayBody}
              mentions={displayBody === message.body ? message.mentions : []}
            />
          </div>
          {attachments.length > 0 && (
            <AttachmentStack attachments={attachments} />
          )}
          {pollChoices.length > 0 && (
            <PollCard
              choices={pollChoices}
              disabled={!actionRequest || Boolean(busy?.startsWith(`action:${message.id}:`))}
              onChoose={
                actionRequest
                  ? (choice) => onAnswerAction(message, choice.id, choice.accepted)
                  : undefined
              }
            />
          )}
          {reactions.length > 0 && (
            <div className="mt-3 flex min-h-7 flex-wrap items-center gap-1.5">
              {reactions.map((reaction) => {
                const selected = Boolean(
                  currentActorId && reaction.actorIds.includes(currentActorId),
                );
                return (
                  <button
                    key={reaction.emoji}
                    type="button"
                    className={cn(
                      "reaction-chip",
                      selected
                        ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                        : "border-[#e2e5ed] bg-white text-[#31394a]",
                    )}
                    title={reaction.actorIds
                      .map((actorId) => actorName(actors, actorId))
                      .join(", ")}
                    disabled={busy === `message:reaction:${message.id}:${reaction.emoji}`}
                    onClick={() => onToggleReaction(message, reaction.emoji)}
                  >
                    <span className="text-sm leading-none">{reaction.emoji}</span>
                    <span>{reaction.actorIds.length}</span>
                  </button>
                );
              })}
              <ReactionPicker
                busy={busy}
                compact
                message={message}
                onToggleReaction={onToggleReaction}
              />
            </div>
          )}
          {threadSummary && (
            <ThreadSummaryRow
              actors={actors}
              rootAuthor={actor}
              thread={threadSummary}
              threadStats={threadStats}
              onOpen={() => onStartThread(message)}
            />
          )}
          <div className="mt-2 flex flex-wrap gap-2 opacity-0 transition-opacity group-hover:opacity-100">
            {canReply && (
              <Button variant="ghost" size="sm" onClick={() => onReply(message)}>
                <Reply size={14} />
                Reply
              </Button>
            )}
            {reactions.length === 0 && (
              <ReactionPicker
                busy={busy}
                message={message}
                onToggleReaction={onToggleReaction}
              />
            )}
            {canStartThread && !threadSummary && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onStartThread(message)}
                disabled={busy === `thread:create:${message.id}`}
              >
                <Split size={14} />
                Thread
              </Button>
            )}
            <span className="self-center font-mono text-[11px] text-muted-foreground">
              {shortId(message.id, 10)}
            </span>
          </div>
        </div>
      </div>
    </article>
  );
}
