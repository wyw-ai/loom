import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Keeps a surface mounted briefly after it closes so CSS can render an exit
 * transition. Consumers should remove interactive semantics while `open` is
 * false, because the retained DOM is visual-only during that short interval.
 */
export function usePresence(open: boolean, exitDurationMs = 140) {
  const [mounted, setMounted] = useState(open);

  useEffect(() => {
    if (open) {
      setMounted(true);
      return;
    }

    if (!mounted) return;
    const timer = window.setTimeout(() => setMounted(false), exitDurationMs);
    return () => window.clearTimeout(timer);
  }, [exitDurationMs, mounted, open]);

  return mounted;
}

/** Delays a dialog's final unmount while its closing animation runs. */
export function useAnimatedDismiss(onDismiss: () => void, durationMs = 180) {
  const [closing, setClosing] = useState(false);
  const timerRef = useRef<number | null>(null);
  const dismissingRef = useRef(false);

  const dismiss = useCallback(() => {
    if (dismissingRef.current) return;
    dismissingRef.current = true;
    setClosing(true);
    timerRef.current = window.setTimeout(onDismiss, durationMs);
  }, [durationMs, onDismiss]);

  useEffect(() => () => {
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
  }, []);

  return { closing, dismiss };
}
