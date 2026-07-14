import { create } from "zustand";
import type { ComposerType } from "@/lib/composer-utils";
import { loadComposerHeight, saveComposerHeight, clearComposerHeight } from "@/lib/composer-utils";

/**
 * Composer resize state.
 *
 * - `mode` is "auto" (textarea grows up to maxRows, then scrolls) or "manual"
 *   (height locked to `manualHeight` after a drag).
 * - `manualHeight` is the persisted pixel height when in manual mode.
 */

export type ComposerMode = "auto" | "manual";

interface ComposerHeightEntry {
  mode: ComposerMode;
  manualHeight: number | null;
}

export interface ComposerStore {
  entries: Record<ComposerType, ComposerHeightEntry>;

  /** Set the manual height for a composer type and switch to manual mode. */
  setManualHeight: (type: ComposerType, height: number) => void;
  /** Reset a composer type to auto mode. */
  resetToAuto: (type: ComposerType) => void;
  /** Get the entry for a composer type. */
  getEntry: (type: ComposerType) => ComposerHeightEntry;
}

function loadInitialEntries(): Record<ComposerType, ComposerHeightEntry> {
  const channelHeight = loadComposerHeight("channel");
  const threadHeight = loadComposerHeight("thread");
  return {
    channel:
      channelHeight !== null
        ? { mode: "manual", manualHeight: channelHeight }
        : { mode: "auto", manualHeight: null },
    thread:
      threadHeight !== null
        ? { mode: "manual", manualHeight: threadHeight }
        : { mode: "auto", manualHeight: null },
  };
}

export const useComposerStore = create<ComposerStore>((set, get) => ({
  entries: loadInitialEntries(),

  setManualHeight: (type, height) =>
    set((state) => {
      saveComposerHeight(type, height);
      return {
        entries: {
          ...state.entries,
          [type]: { mode: "manual", manualHeight: height },
        },
      };
    }),

  resetToAuto: (type) =>
    set((state) => {
      clearComposerHeight(type);
      return {
        entries: {
          ...state.entries,
          [type]: { mode: "auto", manualHeight: null },
        },
      };
    }),

  getEntry: (type) => get().entries[type],
}));
