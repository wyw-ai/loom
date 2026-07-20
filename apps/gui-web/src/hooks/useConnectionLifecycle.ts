import { useEffect } from "react";
import * as ipc from "@/ipc/bridge";
import { machineStatusPollIntervalMs } from "@/lib/constants";
import { reconnectDelayMs } from "@/lib/format-utils";
import type { Workspace, StreamUpdate } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";

export interface ConnectionLifecycleDeps {
  loadConfig: () => Promise<void>;
  loadMachines: (force?: boolean) => Promise<unknown>;
  handleStream: (update: StreamUpdate) => void;
  connectWorkspace: (workspaceId: string, opts?: {
    automatic?: boolean;
    quiet?: boolean;
    reconnect?: boolean;
  }) => Promise<Workspace | null>;
  setConnection: (conn: ConnectionState) => void;
  setError: (err: string | null) => void;
  connection: ConnectionState;
  account: unknown;
  workspace: Workspace | null;
  workspaceRef: React.MutableRefObject<Workspace | null>;
  autoReconnectRef: React.MutableRefObject<boolean>;
  hasOpenedConnectionRef: React.MutableRefObject<boolean>;
  reconnectTimerRef: React.MutableRefObject<number | null>;
  reconnectAttemptRef: React.MutableRefObject<number>;
  clearReconnectTimer: () => void;
}

export function useConnectionLifecycle(deps: ConnectionLifecycleDeps) {
  const {
    loadConfig,
    loadMachines,
    handleStream,
    connectWorkspace,
    setConnection,
    setError,
    connection,
    account,
    workspace,
    workspaceRef,
    autoReconnectRef,
    hasOpenedConnectionRef,
    reconnectTimerRef,
    reconnectAttemptRef,
    clearReconnectTimer,
  } = deps;

  // Stream + connection listener setup
  useEffect(() => {
    let unlistenStream: (() => void) | null = null;
    let unlistenConnection: (() => void) | null = null;

    void loadConfig();
    void ipc.onStream((update) => handleStream(update)).then((off) => {
      unlistenStream = off;
    });
    void ipc.onConnection((event) => {
      if (event.state === "closed") {
        setConnection("closed");
        if (
          hasOpenedConnectionRef.current &&
          autoReconnectRef.current &&
          workspaceRef.current
        ) {
          setError("Connection lost. Reconnecting...");
        } else {
          autoReconnectRef.current = false;
          setError(null);
        }
        void loadMachines(true).catch(() => {});
      } else {
        hasOpenedConnectionRef.current = true;
        if (workspaceRef.current) autoReconnectRef.current = true;
        reconnectAttemptRef.current = 0;
        setConnection("open");
        setError(null);
        void loadMachines(true).catch(() => {});
      }
    }).then((off) => {
      unlistenConnection = off;
    });

    return () => {
      unlistenStream?.();
      unlistenConnection?.();
    };
  }, [loadConfig, loadMachines]);

  // Keep workspaceRef in sync
  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  // Auto-connect when account + workspaceId are ready
  useEffect(() => {
    if (!account || !workspace?.id || connection !== "idle") return;
    autoReconnectRef.current = false;
    hasOpenedConnectionRef.current = false;
    reconnectAttemptRef.current = 0;
    void connectWorkspace(workspace.id, { automatic: true, quiet: true });
  }, [account, workspace?.id, connectWorkspace, connection]);

  // Reconnect on connection loss
  useEffect(() => {
    if (
      !account ||
      !autoReconnectRef.current ||
      !hasOpenedConnectionRef.current ||
      !workspace ||
      (connection !== "closed" && connection !== "error")
    ) {
      return;
    }

    const attempt = reconnectAttemptRef.current + 1;
    reconnectAttemptRef.current = attempt;
    const delay = reconnectDelayMs(attempt);
    const timer = window.setTimeout(() => {
      reconnectTimerRef.current = null;
      void connectWorkspace(workspace.id, { automatic: true, reconnect: true });
    }, delay);
    reconnectTimerRef.current = timer;

    return () => {
      if (reconnectTimerRef.current === timer) {
        window.clearTimeout(timer);
        reconnectTimerRef.current = null;
      }
    };
  }, [account, connectWorkspace, connection, workspace]);

  // Periodic machine status polling
  useEffect(() => {
    if (connection !== "open") return;

    const refreshMachines = () => {
      void loadMachines(true).catch(() => {});
    };
    refreshMachines();

    const interval = window.setInterval(refreshMachines, machineStatusPollIntervalMs);
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshMachines();
    };
    document.addEventListener("visibilitychange", refreshWhenVisible);

    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [connection, loadMachines]);

  // Cleanup reconnect timer on unmount
  useEffect(() => {
    return () => clearReconnectTimer();
  }, [clearReconnectTimer]);
}
