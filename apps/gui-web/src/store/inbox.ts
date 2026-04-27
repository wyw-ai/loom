import { create } from "zustand";

import type { ActionChoice, ScopeRef } from "@/ipc/types";

export interface InboxItem {
  requestEventId: string;
  scope: ScopeRef;
  title: string;
  description: string;
  reason?: string;
  command?: string;
  choices: ActionChoice[];
  arrivedAt: string;
  seen: boolean;
}

interface InboxState {
  items: InboxItem[];
  add: (item: InboxItem) => void;
  remove: (requestEventId: string) => void;
  markAllSeen: () => void;
  unseenCount: () => number;
}

export const useInbox = create<InboxState>((set, get) => ({
  items: [],
  add: (item) =>
    set((s) => {
      if (s.items.some((x) => x.requestEventId === item.requestEventId)) {
        return s;
      }
      return { items: [item, ...s.items] };
    }),
  remove: (id) =>
    set((s) => ({ items: s.items.filter((x) => x.requestEventId !== id) })),
  markAllSeen: () =>
    set((s) => ({ items: s.items.map((x) => ({ ...x, seen: true })) })),
  unseenCount: () => get().items.filter((x) => !x.seen).length,
}));
