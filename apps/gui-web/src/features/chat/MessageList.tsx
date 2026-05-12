import { useEffect, useMemo, useRef } from "react";

import type { Bubble as BubbleModel, ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { useChannels } from "@/store/channels";
import { useMessages } from "@/store/messages";
import { Bubble, type BubbleReplyContext } from "./Bubble";

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
