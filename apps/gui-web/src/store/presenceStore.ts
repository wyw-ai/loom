import { create } from "zustand";
import type { PresenceChangedData } from "@/ipc/types";

interface PresenceStore {
  onlineByActorId: Record<string, boolean>;
  presenceTick: number;
  deliveryTick: number;
  setPresenceSnapshot: (actorIds: string[], onlineActorIds: string[]) => void;
  applyPresenceChanged: (data: PresenceChangedData) => void;
  bumpDeliveryTick: () => void;
}

export const usePresenceStore = create<PresenceStore>((set) => ({
  onlineByActorId: {},
  presenceTick: 0,
  deliveryTick: 0,
  setPresenceSnapshot: (actorIds, onlineActorIds) => {
    const online = new Set(onlineActorIds);
    set((state) => {
      const next = { ...state.onlineByActorId };
      for (const actorId of actorIds) next[actorId] = online.has(actorId);
      return {
        onlineByActorId: next,
        presenceTick: state.presenceTick + 1,
      };
    });
  },
  applyPresenceChanged: (data) => {
    set((state) => ({
      onlineByActorId: {
        ...state.onlineByActorId,
        [data.actorId]: data.online,
      },
      presenceTick: state.presenceTick + 1,
    }));
  },
  bumpDeliveryTick: () => {
    set((state) => ({ deliveryTick: state.deliveryTick + 1 }));
  },
}));

export function applyPresenceChanged(data: PresenceChangedData) {
  usePresenceStore.getState().applyPresenceChanged(data);
}

export function bumpDeliveryTick() {
  usePresenceStore.getState().bumpDeliveryTick();
}
