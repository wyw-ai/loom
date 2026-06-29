import { create } from "zustand";
import type { Channel, ScopeRef, Thread } from "@/ipc/types";
import type { ChannelGroup } from "@/lib/types";
import { loadChannelGroups, channelGroupStorageKey } from "@/lib/channel-utils";

export interface ChannelStore {
  // State
  channels: Channel[];
  channelGroups: ChannelGroup[];
  threadsByChannel: Record<string, Thread[]>;
  activeChannelId: string | null;
  activeThreadId: string | null;
  activeDirectActorId: string | null;
  directScopesByActorId: Record<string, ScopeRef>;

  // Setters
  setChannels: (channels: Channel[] | ((prev: Channel[]) => Channel[])) => void;
  setChannelGroups: (groups: ChannelGroup[] | ((prev: ChannelGroup[]) => ChannelGroup[])) => void;
  setThreadsByChannel: (threads: Record<string, Thread[]> | ((prev: Record<string, Thread[]>) => Record<string, Thread[]>)) => void;
  setActiveChannelId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setActiveThreadId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setActiveDirectActorId: (id: string | null | ((prev: string | null) => string | null)) => void;
  setDirectScopesByActorId: (scopes: Record<string, ScopeRef> | ((prev: Record<string, ScopeRef>) => Record<string, ScopeRef>)) => void;
}

export const useChannelStore = create<ChannelStore>((set) => ({
  channels: [],
  channelGroups: loadChannelGroups(channelGroupStorageKey(null)),
  threadsByChannel: {},
  activeChannelId: null,
  activeThreadId: null,
  activeDirectActorId: null,
  directScopesByActorId: {},

  setChannels: (channels) => set((state) => ({ channels: typeof channels === 'function' ? channels(state.channels) : channels })),
  setChannelGroups: (channelGroups) => set((state) => ({ channelGroups: typeof channelGroups === 'function' ? channelGroups(state.channelGroups) : channelGroups })),
  setThreadsByChannel: (threadsByChannel) => set((state) => ({ threadsByChannel: typeof threadsByChannel === 'function' ? threadsByChannel(state.threadsByChannel) : threadsByChannel })),
  setActiveChannelId: (activeChannelId) => set((state) => ({ activeChannelId: typeof activeChannelId === 'function' ? activeChannelId(state.activeChannelId) : activeChannelId })),
  setActiveThreadId: (activeThreadId) => set((state) => ({ activeThreadId: typeof activeThreadId === 'function' ? activeThreadId(state.activeThreadId) : activeThreadId })),
  setActiveDirectActorId: (activeDirectActorId) => set((state) => ({ activeDirectActorId: typeof activeDirectActorId === 'function' ? activeDirectActorId(state.activeDirectActorId) : activeDirectActorId })),
  setDirectScopesByActorId: (directScopesByActorId) => set((state) => ({ directScopesByActorId: typeof directScopesByActorId === 'function' ? directScopesByActorId(state.directScopesByActorId) : directScopesByActorId })),
}));
