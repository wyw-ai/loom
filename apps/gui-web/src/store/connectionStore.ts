import { create } from "zustand";
import type { DesktopConfig, Workspace } from "@/ipc/types";
import { localServerUrl } from "@/lib/constants";
import type { ConnectionState } from "@/lib/types";

export interface ConnectionStore {
  // State
  config: DesktopConfig;
  workspace: Workspace | null;
  connection: ConnectionState;
  error: string | null;
  notice: string | null;
  busy: string | null;
  workspaceForm: { name: string; serverUrl: string };

  // Setters
  setConfig: (config: DesktopConfig | ((prev: DesktopConfig) => DesktopConfig)) => void;
  setWorkspace: (workspace: Workspace | null | ((prev: Workspace | null) => Workspace | null)) => void;
  setConnection: (connection: ConnectionState | ((prev: ConnectionState) => ConnectionState)) => void;
  setError: (error: string | null | ((prev: string | null) => string | null)) => void;
  setNotice: (notice: string | null | ((prev: string | null) => string | null)) => void;
  setBusy: (busy: string | null | ((prev: string | null) => string | null)) => void;
  setWorkspaceForm: (form: { name: string; serverUrl: string } | ((prev: { name: string; serverUrl: string }) => { name: string; serverUrl: string })) => void;
}

export const useConnectionStore = create<ConnectionStore>((set) => ({
  config: {
    workspaces: [],
    account: null,
  },
  workspace: null,
  connection: "idle",
  error: null,
  notice: null,
  busy: null,
  workspaceForm: {
    name: "Local",
    serverUrl: localServerUrl,
  },

  setConfig: (config) => set((state) => ({ config: typeof config === 'function' ? config(state.config) : config })),
  setWorkspace: (workspace) => set((state) => ({ workspace: typeof workspace === 'function' ? workspace(state.workspace) : workspace })),
  setConnection: (connection) => set((state) => ({ connection: typeof connection === 'function' ? connection(state.connection) : connection })),
  setError: (error) => set((state) => ({ error: typeof error === 'function' ? error(state.error) : error })),
  setNotice: (notice) => set((state) => ({ notice: typeof notice === 'function' ? notice(state.notice) : notice })),
  setBusy: (busy) => set((state) => ({ busy: typeof busy === 'function' ? busy(state.busy) : busy })),
  setWorkspaceForm: (workspaceForm) => set((state) => ({ workspaceForm: typeof workspaceForm === 'function' ? workspaceForm(state.workspaceForm) : workspaceForm })),
}));
