import { useCallback, useEffect, useRef, useState } from "react";
import * as ipc from "@/ipc/bridge";
import type { Run, RunStatus } from "@/ipc/types";
import type { ConnectionState, View } from "@/lib/types";

const ACTIVE_STATUSES: RunStatus[] = [
  "queued",
  "preparing_context",
  "running",
  "waiting_tool",
];
const RUN_LIST_LIMIT = 200;
const RUNS_REFRESH_MS = 15_000;

function isActiveRun(run: Run): boolean {
  return ACTIVE_STATUSES.includes(run.status);
}

/**
 * Initial Runs-view data. The `run.updated` stream only covers subscribed
 * scopes, so entering the runs view fetches `run.list` and merges it into the
 * runs store; the effect also refetches when the connection (re)opens.
 */
export function useRunsList({
  connection,
  workspaceId,
  view,
  setRuns,
}: {
  connection: ConnectionState;
  workspaceId: string | null;
  view: View;
  setRuns: (
    updater: (current: Record<string, Run>) => Record<string, Run>,
  ) => void;
}) {
  const [loading, setLoading] = useState(false);
  const [activeError, setActiveError] = useState<string | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const recentRequestRef = useRef(0);
  const recentInFlightRef = useRef(false);
  const previousWorkspaceIdRef = useRef<string | null>(null);

  const fetchRecent = useCallback(
    async (silent = false) => {
      if (connection !== "open" || !workspaceId || recentInFlightRef.current) return;
      const requestId = ++recentRequestRef.current;
      recentInFlightRef.current = true;
      if (!silent) {
        setLoading(true);
        setRecentError(null);
      }
      try {
        const result = await ipc.runList({ limit: RUN_LIST_LIMIT });
        if (requestId !== recentRequestRef.current) return;
        setRuns((current) => {
          const next: Record<string, Run> = {};
          // Keep the active snapshot recovered on connection even if it falls
          // outside the bounded recent page.
          for (const run of Object.values(current)) {
            if (isActiveRun(run)) next[run.id] = run;
          }
          for (const run of result.runs) next[run.id] = run;
          return next;
        });
        setRecentError(null);
      } catch (err) {
        if (requestId === recentRequestRef.current) {
          setRecentError(
            `Unable to refresh runs: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      } finally {
        if (requestId === recentRequestRef.current) {
          recentInFlightRef.current = false;
          setLoading(false);
        }
      }
    },
    [connection, setRuns, workspaceId],
  );

  const refresh = useCallback(() => fetchRecent(false), [fetchRecent]);

  // Recover the canonical active snapshot on every successful connection,
  // including reconnects while the user stays in Chat.
  useEffect(() => {
    if (connection !== "open" || !workspaceId) {
      recentRequestRef.current += 1;
      recentInFlightRef.current = false;
      setLoading(false);
      setActiveError(null);
      setRecentError(null);
      if (!workspaceId) previousWorkspaceIdRef.current = null;
      return;
    }
    if (previousWorkspaceIdRef.current !== workspaceId) {
      previousWorkspaceIdRef.current = workspaceId;
      recentRequestRef.current += 1;
      recentInFlightRef.current = false;
      setLoading(false);
      setRecentError(null);
      setRuns(() => ({}));
    }
    let alive = true;
    setActiveError(null);
    void ipc
      .runList({ statuses: ACTIVE_STATUSES, limit: RUN_LIST_LIMIT })
      .then((result) => {
        if (!alive) return;
        setRuns((current) => {
          const next: Record<string, Run> = {};
          for (const run of Object.values(current)) {
            if (!isActiveRun(run)) next[run.id] = run;
          }
          for (const run of result.runs) next[run.id] = run;
          return next;
        });
        setActiveError(null);
      })
      .catch((err) => {
        if (!alive) return;
        setActiveError(
          `Unable to restore active runs: ${err instanceof Error ? err.message : String(err)}`,
        );
      });
    return () => {
      alive = false;
    };
  }, [connection, setRuns, workspaceId]);

  // Entering Runs fetches a fresh bounded page. Poll while it remains visible
  // because run.updated only arrives for scopes this client subscribed to.
  useEffect(() => {
    if (connection !== "open" || !workspaceId || view !== "runs") {
      setLoading(false);
      return;
    }
    void fetchRecent(false);
    const interval = window.setInterval(() => {
      void fetchRecent(true);
    }, RUNS_REFRESH_MS);
    return () => {
      window.clearInterval(interval);
      recentRequestRef.current += 1;
      recentInFlightRef.current = false;
    };
  }, [connection, fetchRecent, view, workspaceId]);

  return { loading, error: recentError ?? activeError, refresh };
}
