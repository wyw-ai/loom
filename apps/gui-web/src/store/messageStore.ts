import { create } from "zustand";
import type { Message } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";

// ---------------------------------------------------------------------------
// LRU caps — prevent unbounded growth (ARCH P7 spec)
// ---------------------------------------------------------------------------

const MAX_CHANNEL_MESSAGES = 150;   // channels: 150 messages
const MAX_THREAD_MESSAGES = 100;    // threads:  100 messages
const MAX_DIRECT_MESSAGES = 150;    // DMs:      150 messages
const MAX_THREAD_STATS = 50;        // threadStatsById: max 50 entries

/** Trim array to at most `max` items, keeping the newest. */
function capMessages<T>(items: T[], max: number): T[] {
  return items.length > max ? items.slice(-max) : items;
}

/**
 * LRU-evict a Record<string, V> by tracking access order.
 * After eviction the record has at most `max` entries,
 * dropping the least-recently-accessed keys.
 */
function lruRecord<V>(
  prev: Record<string, V>,
  prevOrder: string[],
  max: number,
): { record: Record<string, V>; order: string[] } {
  if (prevOrder.length <= max) return { record: prev, order: prevOrder };
  const toEvict = new Set(prevOrder.slice(0, prevOrder.length - max));
  const record: Record<string, V> = {};
  const order: string[] = [];
  for (const key of prevOrder) {
    if (!toEvict.has(key)) {
      record[key] = prev[key];
      order.push(key);
    }
  }
  return { record, order };
}

/** Touch a key in the access order (move to end). */
function touchKey(order: string[], key: string): string[] {
  const filtered = order.filter((k) => k !== key);
  filtered.push(key);
  return filtered;
}

export interface MessageStore {
  // State
  messages: Message[];
  threadMessages: Message[];
  directMessages: Message[];
  threadStatsById: Record<string, ThreadActivityStats>;
  _threadStatsOrder: string[];
  draft: string;
  threadDraft: string;
  directDraft: string;

  // Setters
  setMessages: (messages: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadMessages: (messages: Message[] | ((prev: Message[]) => Message[])) => void;
  setDirectMessages: (messages: Message[] | ((prev: Message[]) => Message[])) => void;
  setThreadStatsById: (stats: Record<string, ThreadActivityStats> | ((prev: Record<string, ThreadActivityStats>) => Record<string, ThreadActivityStats>)) => void;
  setDraft: (draft: string | ((prev: string) => string)) => void;
  setThreadDraft: (draft: string | ((prev: string) => string)) => void;
  setDirectDraft: (draft: string | ((prev: string) => string)) => void;

  // Mutations
  upsertMessage: (message: Message) => void;
}

export const useMessageStore = create<MessageStore>((set, get) => ({
  messages: [],
  threadMessages: [],
  directMessages: [],
  threadStatsById: {},
  _threadStatsOrder: [],
  draft: "",
  threadDraft: "",
  directDraft: "",

  // Messages: capped at MAX_CHANNEL_MESSAGES (150), newest-first trimming
  setMessages: (messages) =>
    set((state) => ({
      messages: capMessages(
        typeof messages === "function" ? messages(state.messages) : messages,
        MAX_CHANNEL_MESSAGES,
      ),
    })),

  // Thread messages: capped at MAX_THREAD_MESSAGES (100)
  setThreadMessages: (threadMessages) =>
    set((state) => ({
      threadMessages: capMessages(
        typeof threadMessages === "function"
          ? threadMessages(state.threadMessages)
          : threadMessages,
        MAX_THREAD_MESSAGES,
      ),
    })),

  // Direct messages: capped at MAX_DIRECT_MESSAGES (150)
  setDirectMessages: (directMessages) =>
    set((state) => ({
      directMessages: capMessages(
        typeof directMessages === "function"
          ? directMessages(state.directMessages)
          : directMessages,
        MAX_DIRECT_MESSAGES,
      ),
    })),

  // threadStatsById: LRU-capped at MAX_THREAD_STATS (50) entries
  setThreadStatsById: (threadStatsById) =>
    set((state) => {
      const resolved =
        typeof threadStatsById === "function"
          ? threadStatsById(state.threadStatsById)
          : threadStatsById;

      // Build fresh access order: existing keys + any newly touched keys
      const existingKeys = new Set(Object.keys(state.threadStatsById));
      let order = [...state._threadStatsOrder];
      for (const key of Object.keys(resolved)) {
        if (!existingKeys.has(key) || resolved[key] !== state.threadStatsById[key]) {
          order = touchKey(order, key);
        }
      }

      const { record, order: finalOrder } = lruRecord(resolved, order, MAX_THREAD_STATS);
      return { threadStatsById: record, _threadStatsOrder: finalOrder };
    }),

  setDraft: (draft) =>
    set((state) => ({
      draft: typeof draft === "function" ? draft(state.draft) : draft,
    })),
  setThreadDraft: (threadDraft) =>
    set((state) => ({
      threadDraft:
        typeof threadDraft === "function" ? threadDraft(state.threadDraft) : threadDraft,
    })),
  setDirectDraft: (directDraft) =>
    set((state) => ({
      directDraft:
        typeof directDraft === "function" ? directDraft(state.directDraft) : directDraft,
    })),

  upsertMessage: (message) => {
    const state = get();

    // Update in messages
    const msgIdx = state.messages.findIndex((m) => m.id === message.id);
    if (msgIdx >= 0) {
      const updated = [...state.messages];
      updated[msgIdx] = message;
      set({ messages: updated });
    }

    // Update in threadMessages
    const thrIdx = state.threadMessages.findIndex((m) => m.id === message.id);
    if (thrIdx >= 0) {
      const updated = [...state.threadMessages];
      updated[thrIdx] = message;
      set({ threadMessages: updated });
    }

    // Update in directMessages
    const dmIdx = state.directMessages.findIndex((m) => m.id === message.id);
    if (dmIdx >= 0) {
      const updated = [...state.directMessages];
      updated[dmIdx] = message;
      set({ directMessages: updated });
    }
  },
}));
