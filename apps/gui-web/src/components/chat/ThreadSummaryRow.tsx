import type { Actor, Thread } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";
import { threadParticipants, threadReplyCount, threadLastReplyLabel } from "@/lib/message-utils";
import { AvatarStack } from "@/components/agent/AvatarStack";

export function ThreadSummaryRow({
  actors,
  rootAuthor,
  thread,
  threadStats,
  onOpen,
}: {
  actors: Record<string, Actor>;
  rootAuthor?: Actor;
  thread: Thread;
  threadStats?: ThreadActivityStats;
  onOpen: () => void;
}) {
  const participants = threadParticipants(thread, actors, rootAuthor, threadStats);
  const replyCount = threadReplyCount(thread, threadStats);
  const lastReply = threadLastReplyLabel(thread, threadStats);
  return (
    <button type="button" className="thread-summary-row" onClick={onOpen}>
      <AvatarStack actors={participants} max={4} small />
      <span className="min-w-0 truncate text-xs font-bold text-[#503ed4]">
        {typeof replyCount === "number"
          ? `${replyCount}${threadStats?.hasMoreReplies ? "+" : ""} ${
              replyCount === 1 ? "reply" : "replies"
            }`
          : "Thread"}
      </span>
      {lastReply && (
        <span className="shrink-0 text-xs font-medium text-[#667085]">
          Last reply {lastReply}
        </span>
      )}
    </button>
  );
}
