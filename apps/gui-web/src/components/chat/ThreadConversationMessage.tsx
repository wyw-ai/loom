import { memo } from "react";
import type { Actor, MachineInfo, Message, Run } from "@/ipc/types";
import { bodyPollFromMessage, actionChoices, metadataText } from "@/lib/message-utils";
import { displayName, actorName } from "@/lib/format-utils";
import { formatTime } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { AgentMessageAvatar } from "@/components/agent/AgentMessageAvatar";
import { MessageMarkdown } from "@/components/chat/MessageMarkdown";
import { AttachmentStack } from "@/components/chat/AttachmentStack";
import { PollCard } from "@/components/chat/PollCard";
import { ReactionPicker } from "@/components/chat/ReactionPicker";

export const ThreadConversationMessage = memo(function ThreadConversationMessage({
  actor,
  actors,
  currentActorId,
  machines,
  runs,
  message,
  busy,
  root = false,
  onOpenAgentSettings,
  onToggleReaction,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  currentActorId: string | null;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  message: Message;
  busy: string | null;
  root?: boolean;
  onOpenAgentSettings: (actorId: string) => void;
  onToggleReaction: (message: Message, emoji: string) => void;
}) {
  const reactions = message.reactions ?? [];
  const bodyPoll = bodyPollFromMessage(message);
  const choices = actionChoices(message);
  const pollChoices = choices.length > 0 ? choices : bodyPoll?.choices ?? [];
  const displayBody = bodyPoll?.question || message.body || metadataText(message);
  const attachments = message.attachments ?? [];
  return (
    <article
      className={cn(
        "group rounded-xl px-4 py-3 transition-colors",
        root ? "bg-[#fbfbfd]" : "hover:bg-[#f7f8fb]",
      )}
    >
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          runs={runs}
          onOpenAgentSettings={onOpenAgentSettings}
          preferredPlacement="left"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">
              {actor ? displayName(actor) : message.authorActorId}
            </span>
            <span className="text-xs font-medium text-[#667085]">
              {formatTime(message.createdAt)}
            </span>
          </div>
          <div className="message-markdown mt-1 max-w-none break-words text-[15px] leading-6 text-[#111827]">
            <MessageMarkdown
              actors={actors}
              body={displayBody}
              mentions={displayBody === message.body ? message.mentions : []}
            />
          </div>
          {pollChoices.length > 0 && (
            <PollCard choices={pollChoices} disabled />
          )}
          {attachments.length > 0 && (
            <AttachmentStack attachments={attachments} />
          )}
          {reactions.length > 0 ? (
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
          ) : (
            <div className="mt-2 flex flex-wrap gap-2 opacity-0 transition-opacity group-hover:opacity-100">
              <ReactionPicker
                busy={busy}
                message={message}
                onToggleReaction={onToggleReaction}
              />
            </div>
          )}
        </div>
      </div>
    </article>
  );
}, (prev, next) => {
  return (
    prev.message === next.message &&
    prev.actor === next.actor &&
    prev.busy === next.busy &&
    prev.root === next.root
  );
});
