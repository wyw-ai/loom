import { create } from "zustand";

import type { Workspace } from "@/ipc/types";

interface WorkspacesState {
  workspaces: Workspace[];
  activeId: string | null;
  connectingId: string | null;
  /// Drives <AddWorkspaceHost />. Keeping the flag here (rather than in the
  /// UI store) avoids a dependency cycle between "workspace CRUD" and
  /// "generic modal machinery" — the workspace forms are special enough
  /// to own their visibility state.
  addOpen: boolean;
  switchOpen: boolean;

  setConfig: (cfg: { workspaces: Workspace[]; active?: string | null }) => void;
  setActive: (id: string | null) => void;
  setConnecting: (id: string | null) => void;
  setAddOpen: (open: boolean) => void;
  setSwitchOpen: (open: boolean) => void;
  active: () => Workspace | null;
}

export const useWorkspaces = create<WorkspacesState>((set, get) => ({
  workspaces: [],
  activeId: null,
  connectingId: null,
  addOpen: false,
  switchOpen: false,

  setConfig: (cfg) =>
    set({
      workspaces: cfg.workspaces,
      activeId: cfg.active ?? null,
    }),
  setActive: (id) => set({ activeId: id }),
  setConnecting: (id) => set({ connectingId: id }),
  setAddOpen: (open) => set({ addOpen: open }),
  setSwitchOpen: (open) => set({ switchOpen: open }),
  active: () => {
    const s = get();
    return s.workspaces.find((w) => w.id === s.activeId) ?? null;
  },
}));
