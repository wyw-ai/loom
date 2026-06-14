import { create } from "zustand";
import type { Message } from "@/ipc/types";
import type { ThreadActivityStats } from "@/lib/types";

export interface MessageStore {
  // State
  messages: Message[];
  threadMessages: Message[];
  directMessages: Message[];
  threadStatsById: Record<string, ThreadActivityStats>;
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
  draft: "",
  threadDraft: "",
  directDraft: "",

  setMessages: (messages) => set((state) => ({ messages: typeof messages === 'function' ? messages(state.messages) : messages })),
  setThreadMessages: (threadMessages) => set((state) => ({ threadMessages: typeof threadMessages === 'function' ? threadMessages(state.threadMessages) : threadMessages })),
  setDirectMessages: (directMessages) => set((state) => ({ directMessages: typeof directMessages === 'function' ? directMessages(state.directMessages) : directMessages })),
  setThreadStatsById: (threadStatsById) => set((state) => ({ threadStatsById: typeof threadStatsById === 'function' ? threadStatsById(state.threadStatsById) : threadStatsById })),
  setDraft: (draft) => set((state) => ({ draft: typeof draft === 'function' ? draft(state.draft) : draft })),
  setThreadDraft: (threadDraft) => set((state) => ({ threadDraft: typeof threadDraft === 'function' ? threadDraft(state.threadDraft) : threadDraft })),
  setDirectDraft: (directDraft) => set((state) => ({ directDraft: typeof directDraft === 'function' ? directDraft(state.directDraft) : directDraft })),

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
