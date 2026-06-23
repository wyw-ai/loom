import { create } from "zustand";
import type { Actor, InboxListEntry, MachineInfo, Run } from "@/ipc/types";

export interface ActorStore {
  // State
  actors: Record<string, Actor>;
  runs: Record<string, Run>;
  inbox: InboxListEntry[];
  machines: MachineInfo[];

  // Setters
  setActors: (actors: Record<string, Actor> | ((prev: Record<string, Actor>) => Record<string, Actor>)) => void;
  setRuns: (runs: Record<string, Run> | ((prev: Record<string, Run>) => Record<string, Run>)) => void;
  setInbox: (inbox: InboxListEntry[] | ((prev: InboxListEntry[]) => InboxListEntry[])) => void;
  setMachines: (machines: MachineInfo[] | ((prev: MachineInfo[]) => MachineInfo[])) => void;

  // Mutations
  upsertActor: (actor: Actor) => void;
}

export const useActorStore = create<ActorStore>((set, get) => ({
  actors: {},
  runs: {},
  inbox: [],
  machines: [],

  setActors: (actors) => set((state) => ({ actors: typeof actors === 'function' ? actors(state.actors) : actors })),
  setRuns: (runs) => set((state) => ({ runs: typeof runs === 'function' ? runs(state.runs) : runs })),
  setInbox: (inbox) => set((state) => ({ inbox: typeof inbox === 'function' ? inbox(state.inbox) : inbox })),
  setMachines: (machines) => set((state) => ({ machines: typeof machines === 'function' ? machines(state.machines) : machines })),

  upsertActor: (actor) => {
    set({ actors: { ...get().actors, [actor.id]: actor } });
  },
}));
