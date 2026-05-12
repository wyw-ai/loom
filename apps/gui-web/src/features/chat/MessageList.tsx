import { useEffect, useMemo, useRef } from "react";

import type { Bubble as BubbleModel, ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { useTasks } from "@/store/tasks";
import { Bubble, type BubbleReplyContext, type ThreadLink } from "./Bubble";

export function MessageList({ scope }: { scope: ScopeRef }) {
  const scopeStore = useMessages((s) => s.byScope[scopeKey(scope)]);
  const rootEventId = useChannels((s) => {
    if (scope.kind !== "thread") return undefined;
    for (const threads of Object.values(s.threadsByChannel)) {
      const thread = threads.find((t) => t.id === scope.id);
      if (thread?.rootEventId) return thread.rootEventId;
    }
    return undefined;
  });
  const threadsByChannel = useChannels((s) => s.threadsByChannel);
  const tasks = useTasks((s) => s.tasks);
  const bubbles = scopeStore?.bubbles ?? [];
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [bubbles.length]);

  const bubblesById = useMemo(() => {
    const index = new Map<string, BubbleModel>();
    for (const b of bubbles) index.set(b.id, b);
    return index;
  }, [bubbles]);
  const threadLinksByRoot = useMemo(() => {
    const links = new Map<string, ThreadLink>();
    for (const [channelId, threads] of Object.entries(threadsByChannel)) {
      for (const thread of threads) {
        if (!thread.rootEventId) continue;
        links.set(thread.rootEventId, {
          channelId,
          threadId: thread.id,
          rootEventId: thread.rootEventId,
          title: thread.title,
        });
      }
    }
    for (const task of tasks) {
      const existing = links.get(task.sourceEventId);
      links.set(task.sourceEventId, {
        channelId: task.channelId,
        threadId: task.canonicalThreadId,
        rootEventId: task.sourceEventId,
        title: existing?.title ?? task.title,
        taskNumber: task.number,
        taskStatus: task.status,
      });
    }
    return links;
  }, [threadsByChannel, tasks]);

  // Group consecutive same-actor bubbles within 5 minutes.
  const groups: Array<typeof bubbles> = [];
  for (const b of bubbles) {
    const last = groups[groups.length - 1];
    const lastB = last?.[last.length - 1];
    if (
      lastB &&
      lastB.actorId === b.actorId &&
      lastB.kind !== "system" &&
      b.kind !== "system" &&
      !lastB.replyToEventId &&
      !b.replyToEventId &&
      Math.abs(new Date(b.ts).getTime() - new Date(lastB.ts).getTime()) <
        5 * 60 * 1000
    ) {
      last.push(b);
    } else {
      groups.push([b]);
    }
  }

  return (
    <div className="stable-scrollbar h-full min-h-0 min-w-0 overflow-y-auto overflow-x-hidden bg-white px-4 py-5">
      {bubbles.length === 0 ? (
        <div className="flex min-h-full items-center justify-center font-mono text-sm text-black/40">
          No messages yet. Say something.
        </div>
      ) : (
        groups.map((group, gi) => (
          <div key={gi} className="mb-5 last:mb-0">
            {group.map((b, bi) => {
              const replyContext = buildReplyContext(
                b.replyToEventId,
                bubblesById,
                rootEventId,
              );
              return (
                <Bubble
                  key={b.id}
                  bubble={b}
                  showHeader={bi === 0}
                  domId={domIdForBubble(b.id)}
                  replyContext={replyContext}
                  threadLink={threadLinksByRoot.get(b.id)}
                />
              );
            })}
          </div>
        ))
      )}
      <div ref={bottomRef} />
    </div>
  );
}

function buildReplyContext(
  replyToEventId: string | undefined,
  bubblesById: Map<string, BubbleModel>,
  threadRootEventId: string | undefined,
): BubbleReplyContext | undefined {
  if (!replyToEventId) return undefined;
  if (replyToEventId === threadRootEventId) return undefined;
  const target = bubblesById.get(replyToEventId);
  if (!target) {
    return {
      actorId: "system",
      preview: "Original message unavailable",
      missing: true,
    };
  }
  return {
    actorId: target.actorId,
    preview: replyPreview(target.text, target.handoffTarget),
    domId: domIdForBubble(target.id),
  };
}

function domIdForBubble(id: string): string {
  return `bubble-${id.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
}

function replyPreview(text: string, handoffTarget?: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const head = handoffTarget ? `to @${handoffTarget}: ` : "";
  const body = flat.length > 84 ? `${flat.slice(0, 83)}…` : flat;
  return `${head}${body}` || "(no text)";
}
