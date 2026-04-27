// Global actor directory. Populated lazily from multiple sources
// (actor/list on connect, channel_members fetches, agent/list) so any UI
// that renders an actorId can resolve a human-friendly displayName without
// having to fetch from the server itself.

import { create } from "zustand";

import type { Actor } from "@/ipc/types";

interface ActorsState {
  byId: Record<string, Actor>;
  upsert: (actor: Actor) => void;
  upsertMany: (actors: Actor[]) => void;
  clear: () => void;
}

export const useActors = create<ActorsState>((set) => ({
  byId: {},
  upsert: (actor) =>
    set((s) => ({ byId: { ...s.byId, [actor.id]: actor } })),
  upsertMany: (actors) =>
    set((s) => {
      const next = { ...s.byId };
      for (const a of actors) next[a.id] = a;
      return { byId: next };
    }),
  clear: () => set({ byId: {} }),
}));

// Pure lookup helper — UI code can call this without subscribing to the
// store when it just needs a best-effort name (e.g. modal titles).
export function displayNameOf(actorId: string): string {
  const a = useActors.getState().byId[actorId];
  return a?.displayName || actorId;
}
