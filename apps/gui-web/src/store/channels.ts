import { create } from "zustand";

import type { Actor, Channel, ScopeRef, Thread } from "@/ipc/types";
import { useActors } from "./actors";

interface ChannelsState {
  channels: Channel[];
  threadsByChannel: Record<string, Thread[]>;
  membersByChannel: Record<string, Actor[]>;
  currentScope: ScopeRef | null;
  selected: { channelId?: string; threadId?: string };

  replaceChannels: (cs: Channel[]) => void;
  upsertChannel: (c: Channel) => void;
  removeChannel: (id: string) => void;
  replaceThreads: (channelId: string, threads: Thread[]) => void;
  upsertThread: (t: Thread) => void;
  removeThread: (channelId: string, threadId: string) => void;
  replaceMembers: (channelId: string, members: Actor[]) => void;

  setCurrentScope: (scope: ScopeRef | null) => void;
  setSelected: (sel: { channelId?: string; threadId?: string }) => void;
}

export const useChannels = create<ChannelsState>((set) => ({
  channels: [],
  threadsByChannel: {},
  membersByChannel: {},
  currentScope: null,
  selected: {},

  replaceChannels: (cs) => set({ channels: sortByTitle(cs) }),
  upsertChannel: (c) =>
    set((s) => {
      const existing = s.channels.findIndex((x) => x.id === c.id);
      const channels =
        existing >= 0
          ? s.channels.map((x) => (x.id === c.id ? { ...x, ...c } : x))
          : [...s.channels, c];
      return { channels: sortByTitle(channels) };
    }),
  removeChannel: (id) =>
    set((s) => {
      const { [id]: _, ...rest } = s.threadsByChannel;
      const { [id]: __, ...restMembers } = s.membersByChannel;
      return {
        channels: s.channels.filter((c) => c.id !== id),
        threadsByChannel: rest,
        membersByChannel: restMembers,
      };
    }),

  replaceThreads: (channelId, threads) =>
    set((s) => ({
      threadsByChannel: {
        ...s.threadsByChannel,
        [channelId]: sortByTitle(threads),
      },
    })),
  upsertThread: (t) =>
    set((s) => {
      const list = s.threadsByChannel[t.channelId] ?? [];
      const existing = list.findIndex((x) => x.id === t.id);
      const next =
        existing >= 0
          ? list.map((x) => (x.id === t.id ? { ...x, ...t } : x))
          : [...list, t];
      return {
        threadsByChannel: {
          ...s.threadsByChannel,
          [t.channelId]: sortByTitle(next),
        },
      };
    }),
  removeThread: (channelId, threadId) =>
    set((s) => ({
      threadsByChannel: {
        ...s.threadsByChannel,
        [channelId]: (s.threadsByChannel[channelId] ?? []).filter(
          (t) => t.id !== threadId,
        ),
      },
    })),

  replaceMembers: (channelId, members) => {
    // Piggyback onto member refreshes to keep the global actor directory
    // warm — Bubble headers and @-palette previews both read from it.
    useActors.getState().upsertMany(members);
    const memberIds = members.map((member) => member.id);
    set((s) => ({
      channels: s.channels.map((channel) =>
        channel.id === channelId
          ? { ...channel, members: memberIds }
          : channel,
      ),
      membersByChannel: { ...s.membersByChannel, [channelId]: members },
    }));
  },

  setCurrentScope: (scope) => set({ currentScope: scope }),
  setSelected: (sel) => set({ selected: sel }),
}));

function sortByTitle<T extends { title: string }>(xs: T[]): T[] {
  return [...xs].sort((a, b) => a.title.localeCompare(b.title));
}
