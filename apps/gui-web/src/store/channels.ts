import { create } from "zustand";

import type { Actor, Channel, ScopeRef, Thread } from "@/ipc/types";
import { useActors } from "./actors";

interface ChannelsState {
  channels: Channel[];
  threadsByChannel: Record<string, Thread[]>;
  archivedThreadsByChannel: Record<string, Thread[]>;
  membersByChannel: Record<string, Actor[]>;
  currentScope: ScopeRef | null;
  selected: { channelId?: string; threadId?: string };

  replaceChannels: (cs: Channel[]) => void;
  upsertChannel: (c: Channel) => void;
  removeChannel: (id: string) => void;
  replaceThreads: (channelId: string, threads: Thread[]) => void;
  replaceArchivedThreads: (channelId: string, threads: Thread[]) => void;
  upsertThread: (t: Thread) => void;
  removeThread: (channelId: string, threadId: string) => void;
  replaceMembers: (channelId: string, members: Actor[]) => void;

  setCurrentScope: (scope: ScopeRef | null) => void;
  setSelected: (sel: { channelId?: string; threadId?: string }) => void;
}

export const useChannels = create<ChannelsState>((set) => ({
  channels: [],
  threadsByChannel: {},
  archivedThreadsByChannel: {},
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
      const { [id]: __, ...restArchived } = s.archivedThreadsByChannel;
      const { [id]: ___, ...restMembers } = s.membersByChannel;
      return {
        channels: s.channels.filter((c) => c.id !== id),
        threadsByChannel: rest,
        archivedThreadsByChannel: restArchived,
        membersByChannel: restMembers,
      };
    }),

  replaceThreads: (channelId, threads) =>
    set((s) => ({
      threadsByChannel: {
        ...s.threadsByChannel,
        [channelId]: sortByTitle(threads.filter((t) => !t.archivedAt)),
      },
    })),
  replaceArchivedThreads: (channelId, threads) =>
    set((s) => {
      const archived = threads.filter((t) => t.archivedAt);
      const archivedIds = new Set(archived.map((t) => t.id));
      return {
        threadsByChannel: {
          ...s.threadsByChannel,
          [channelId]: (s.threadsByChannel[channelId] ?? []).filter(
            (t) => !archivedIds.has(t.id),
          ),
        },
        archivedThreadsByChannel: {
          ...s.archivedThreadsByChannel,
          [channelId]: sortByArchivedTime(archived),
        },
      };
    }),
  upsertThread: (t) =>
    set((s) => {
      const active = (s.threadsByChannel[t.channelId] ?? []).filter(
        (x) => x.id !== t.id,
      );
      const archived = (s.archivedThreadsByChannel[t.channelId] ?? []).filter(
        (x) => x.id !== t.id,
      );
      const isArchived = Boolean(t.archivedAt);
      return {
        threadsByChannel: {
          ...s.threadsByChannel,
          [t.channelId]: sortByTitle(isArchived ? active : [...active, t]),
        },
        archivedThreadsByChannel: {
          ...s.archivedThreadsByChannel,
          [t.channelId]: sortByArchivedTime(
            isArchived ? [...archived, t] : archived,
          ),
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
      archivedThreadsByChannel: {
        ...s.archivedThreadsByChannel,
        [channelId]: (s.archivedThreadsByChannel[channelId] ?? []).filter(
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

function sortByArchivedTime<T extends { title: string; archivedAt?: string | null }>(
  xs: T[],
): T[] {
  return [...xs].sort((a, b) => {
    const at = a.archivedAt ? Date.parse(a.archivedAt) : 0;
    const bt = b.archivedAt ? Date.parse(b.archivedAt) : 0;
    return bt - at || a.title.localeCompare(b.title);
  });
}
