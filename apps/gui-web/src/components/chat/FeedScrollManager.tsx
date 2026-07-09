import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import { ScrollJumpButtons } from "@/components/chat/ScrollJumpButtons";
import type { FeedItem } from "@/components/chat/MessageFeed";

/**
 * Isolates Virtuoso's rangeChanged setState from the parent MessageFeed.
 * Without this, every scroll tick triggers a parent re-render which
 * invalidates all useMemo/useCallback in MessageFeed.
 *
 * The parent passes a stable renderItem callback and the feed data;
 * FeedScrollManager owns the scroll position state internally.
 */
export const FeedScrollManager = memo(function FeedScrollManager({
  feedKey,
  feedItems,
  renderItem,
  headerRenderer,
}: {
  feedKey: string;
  feedItems: FeedItem[];
  renderItem: (index: number) => ReactNode;
  headerRenderer?: () => ReactNode;
}) {
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const [firstVisibleIndex, setFirstVisibleIndex] = useState(0);
  const [lastVisibleIndex, setLastVisibleIndex] = useState(0);
  const [isAtBottom, setIsAtBottom] = useState(true);

  const showJumpToTop = firstVisibleIndex > 2;
  const showJumpToBottom = lastVisibleIndex < feedItems.length - 3;

  const components = useMemo(() => {
    if (!headerRenderer) return undefined;
    return { Header: headerRenderer };
  }, [headerRenderer]);

  // Auto-scroll to latest on thread/channel entry
  useEffect(() => {
    if (feedItems.length === 0) return;
    const threshold = 200;
    const timer = setTimeout(() => {
      virtuosoRef.current?.scrollToIndex({
        index: feedItems.length - 1,
        behavior: feedItems.length > threshold ? "auto" : "smooth",
      });
    }, 50);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [feedKey]);

  const onJumpToTop = useCallback(
    () => virtuosoRef.current?.scrollToIndex({ index: 0, behavior: "smooth" }),
    [],
  );
  const onJumpToBottom = useCallback(
    () =>
      virtuosoRef.current?.scrollToIndex({
        index: feedItems.length - 1,
        behavior: "smooth",
      }),
    [feedItems.length],
  );

  return (
    <div className="relative min-h-0 flex-1 bg-white">
      <Virtuoso
        key={feedKey}
        ref={virtuosoRef}
        className="h-full soft-scrollbar"
        totalCount={feedItems.length}
        followOutput={isAtBottom ? "smooth" : false}
        increaseViewportBy={{ top: 200, bottom: 200 }}
        atBottomStateChange={(atBottom: boolean) => setIsAtBottom(atBottom)}
        components={components}
        computeItemKey={(index) => {
          const item = feedItems[index];
          if (!item) return `item-${index}`;
          return item.kind === "date-divider" ? item.key : item.message.id;
        }}
        rangeChanged={(range) => {
          setFirstVisibleIndex(range.startIndex);
          setLastVisibleIndex(range.endIndex);
        }}
        itemContent={(index) => renderItem(index)}
      />
      <ScrollJumpButtons
        showJumpToTop={showJumpToTop}
        showJumpToBottom={showJumpToBottom}
        onJumpToTop={onJumpToTop}
        onJumpToBottom={onJumpToBottom}
      />
    </div>
  );
});
