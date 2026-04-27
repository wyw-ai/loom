import { create } from "zustand";

import type { Workspace } from "@/ipc/types";

type ConnectionState = "idle" | "connecting" | "open" | "closed" | "error";

interface SessionState {
  /// Active workspace the WS is currently bound to. `null` when not
  /// connected yet (fresh launch or post-disconnect).
  workspace: Workspace | null;
  connection: ConnectionState;
  error: string | null;
  setWorkspace: (w: Workspace | null) => void;
  setConnection: (s: ConnectionState, error?: string) => void;
}

export const useSession = create<SessionState>((set) => ({
  workspace: null,
  connection: "idle",
  error: null,
  setWorkspace: (w) => set({ workspace: w }),
  setConnection: (s, error) =>
    set({ connection: s, error: s === "error" ? error ?? null : null }),
}));
