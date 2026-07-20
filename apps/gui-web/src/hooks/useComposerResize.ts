import { useCallback, useState } from "react";
import type { ComposerType } from "@/lib/composer-utils";
import {
  COMPOSER_DEFAULT_CAP,
  COMPOSER_MIN_HEIGHT,
  THREAD_COMPOSER_DEFAULT_CAP,
  THREAD_COMPOSER_MIN_HEIGHT,
  composerMaxHeightPx,
} from "@/lib/composer-utils";
import { useComposerStore } from "@/store/composerStore";

export interface UseComposerResizeOptions {
  type: ComposerType;
  maxHeightOverride?: number;
}

export interface UseComposerResizeReturn {
  effectiveHeight: number;
  isManual: boolean;
  liveHeight: number | null;
  maxHeightPx: number;
  handleResize: (
    _e: unknown,
    _dir: unknown,
    _ref: unknown,
    delta: { height: number },
  ) => void;
  handleResizeStop: (
    _e: MouseEvent | TouchEvent,
    _dir: string,
    _ref: HTMLElement,
    delta: { height: number },
  ) => void;
  handleKeyboardResize: (deltaPx: number) => void;
  handleReset: () => void;
}

export function useComposerResize({
  type,
  maxHeightOverride,
}: UseComposerResizeOptions): UseComposerResizeReturn {
  const [liveHeight, setLiveHeight] = useState<number | null>(null);

  const entry = useComposerStore((s) => s.entries[type]);
  const setManualHeight = useComposerStore((s) => s.setManualHeight);
  const resetToAuto = useComposerStore((s) => s.resetToAuto);

  const minHeight =
    type === "thread" ? THREAD_COMPOSER_MIN_HEIGHT : COMPOSER_MIN_HEIGHT;
  const defaultCap =
    type === "thread" ? THREAD_COMPOSER_DEFAULT_CAP : COMPOSER_DEFAULT_CAP;

  const isManual = entry.mode === "manual" && entry.manualHeight !== null;
  const resolvedHeight = isManual ? entry.manualHeight! : defaultCap;
  const effectiveHeight = liveHeight ?? resolvedHeight;
  const maxHeightPx = maxHeightOverride ?? composerMaxHeightPx();

  const handleResize = useCallback(
    (_e: unknown, _dir: unknown, _ref: unknown, delta: { height: number }) => {
      setLiveHeight(resolvedHeight + delta.height);
    },
    [resolvedHeight],
  );

  const handleResizeStop = useCallback(
    (
      _e: MouseEvent | TouchEvent,
      _dir: string,
      _ref: HTMLElement,
      delta: { height: number },
    ) => {
      const newHeight = Math.max(
        minHeight,
        Math.min(resolvedHeight + delta.height, maxHeightPx),
      );
      setManualHeight(type, newHeight);
      setLiveHeight(null);
    },
    [resolvedHeight, maxHeightPx, setManualHeight, type, minHeight],
  );

  const handleKeyboardResize = useCallback(
    (deltaPx: number) => {
      const newHeight = Math.max(
        minHeight,
        Math.min(resolvedHeight + deltaPx, maxHeightPx),
      );
      setManualHeight(type, newHeight);
    },
    [resolvedHeight, maxHeightPx, setManualHeight, type, minHeight],
  );

  const handleReset = useCallback(() => {
    resetToAuto(type);
  }, [resetToAuto, type]);

  return {
    effectiveHeight,
    isManual,
    liveHeight,
    maxHeightPx,
    handleResize,
    handleResizeStop,
    handleKeyboardResize,
    handleReset,
  };
}
