import { useCallback, useLayoutEffect, useRef } from "react";

export function useStickToBottomScroll({
  contentKey,
  itemCount,
  scrollKey,
}: {
  contentKey: string;
  itemCount: number;
  scrollKey: string;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);

  useLayoutEffect(() => {
    stickToBottomRef.current = true;
  }, [scrollKey]);

  useLayoutEffect(() => {
    if (itemCount === 0) {
      stickToBottomRef.current = true;
      return;
    }
    const element = scrollRef.current;
    if (!element || !stickToBottomRef.current) return;
    const frame = window.requestAnimationFrame(() => {
      element.scrollTop = element.scrollHeight;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [contentKey, itemCount, scrollKey]);

  const onScroll = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const distanceFromBottom =
      element.scrollHeight - element.scrollTop - element.clientHeight;
    stickToBottomRef.current = distanceFromBottom < 160;
  }, []);

  return { onScroll, ref: scrollRef };
}
