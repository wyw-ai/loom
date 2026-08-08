import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Actor, InboxActorStatus, InboxListEntry, Run, ScopeRef } from "@/ipc/types";
import * as ipc from "@/ipc/bridge";
import { actorName, errorText } from "@/lib/format-utils";
import { usePresenceStore } from "@/store/presenceStore";

export type AgentActivityStatus = "running" | "queued" | "offline" | "unknown" | "idle";

export interface AgentQueueItem {
  sourceId: string;
  updatedAt: string;
  authorActorId: string | null;
  authorName: string;
  preview: string;
}

interface AgentQueueState {
  items: AgentQueueItem[];
  loading: boolean;
  error: string | null;
}

export interface AgentActivityAgent {
  actorId: string;
  actor: Actor;
  status: AgentActivityStatus;
  online: boolean;
  pending: number;
  oldestPendingAt?: string | null;
  activeRun: Run | null;
  expanded: boolean;
  queue: AgentQueueItem[];
  queueLoading: boolean;
  queueError: string | null;
}

export interface UseAgentActivityParams {
  actors: Record<string, Actor>;
  agentActorIds: string[];
  runs: Record<string, Run>;
  scope: ScopeRef | null;
  enabled?: boolean;
}

const TERMINAL_RUN_STATUSES = new Set<Run["status"]>([
  "completed",
  "failed",
  "canceled",
]);

const ACTIVE_RUN_PRIORITY: Record<Run["status"], number> = {
  running: 5,
  waiting_tool: 4,
  preparing_context: 3,
  queued: 2,
  completed: 0,
  failed: 0,
  canceled: 0,
};

function pickNewerRun(current: Run | undefined, candidate: Run) {
  if (!current) return candidate;
  const currentPriority = ACTIVE_RUN_PRIORITY[current.status] ?? 0;
  const candidatePriority = ACTIVE_RUN_PRIORITY[candidate.status] ?? 0;
  if (candidatePriority !== currentPriority) {
    return candidatePriority > currentPriority ? candidate : current;
  }
  return candidate.openedAt.localeCompare(current.openedAt) > 0 ? candidate : current;
}

function fallbackAgent(actorId: string): Actor {
  return { id: actorId, kind: "agent" };
}

function previewText(entry: InboxListEntry | undefined) {
  const raw = entry?.message?.body ?? "";
  const oneLine = raw.replace(/\s+/g, " ").trim();
  if (!oneLine) return "No message preview";
  return oneLine.length > 140 ? `${oneLine.slice(0, 139)}…` : oneLine;
}

function queueItemsFromResults(
  actorId: string,
  actors: Record<string, Actor>,
  status: InboxActorStatus | undefined,
  deliveries: InboxListEntry[],
): AgentQueueItem[] {
  const bySourceId = new Map(deliveries.map((entry) => [entry.delivery.sourceId, entry]));
  const ordered = status?.entries?.length
    ? status.entries.map((entry) => ({
        sourceId: entry.sourceId,
        updatedAt: entry.updatedAt,
      }))
    : deliveries.map((entry) => ({
        sourceId: entry.delivery.sourceId,
        updatedAt: entry.delivery.updatedAt,
      }));
  const seen = new Set<string>();
  return ordered.flatMap((entry) => {
    if (seen.has(entry.sourceId)) return [];
    seen.add(entry.sourceId);
    const deliveryEntry = bySourceId.get(entry.sourceId);
    const authorActorId = deliveryEntry?.message?.authorActorId ?? null;
    return [{
      sourceId: entry.sourceId,
      updatedAt: deliveryEntry?.delivery.updatedAt ?? entry.updatedAt,
      authorActorId,
      authorName: authorActorId ? actorName(actors, authorActorId) : actorId,
      preview: previewText(deliveryEntry),
    }];
  });
}

export function useAgentActivity({
  actors,
  agentActorIds,
  runs,
  scope,
  enabled = true,
}: UseAgentActivityParams) {
  const onlineByActorId = usePresenceStore((state) => state.onlineByActorId);
  const presenceTick = usePresenceStore((state) => state.presenceTick);
  const deliveryTick = usePresenceStore((state) => state.deliveryTick);
  const setPresenceSnapshot = usePresenceStore((state) => state.setPresenceSnapshot);

  const [statusByActorId, setStatusByActorId] = useState<Record<string, InboxActorStatus>>({});
  const [queuesByActorId, setQueuesByActorId] = useState<Record<string, AgentQueueState>>({});
  const [expandedActorIds, setExpandedActorIds] = useState<string[]>([]);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const statusRequestIdRef = useRef(0);

  const baseAgentIds = useMemo(
    () => Array.from(new Set(agentActorIds)).sort(),
    [agentActorIds],
  );
  const baseAgentIdsKey = baseAgentIds.join("|");
  const scopeKind = scope?.kind ?? null;
  const scopeId = scope?.id ?? null;

  const scopedRuns = useMemo(
    () => Object.values(runs).filter((run) =>
      Boolean(scopeKind && scopeId && run.scope.kind === scopeKind && run.scope.id === scopeId),
    ),
    [runs, scopeId, scopeKind],
  );

  const relatedActorIds = useMemo(() => {
    const ids = new Set(baseAgentIds);
    for (const run of scopedRuns) {
      const actor = actors[run.actorId];
      if (!actor || actor.kind === "agent") ids.add(run.actorId);
    }
    return Array.from(ids).sort((a, b) => {
      const nameA = actors[a]?.displayName ?? a;
      const nameB = actors[b]?.displayName ?? b;
      return nameA.localeCompare(nameB);
    });
  }, [actors, baseAgentIdsKey, scopedRuns]);
  const relatedActorIdsKey = relatedActorIds.join("|");

  const activeRunsByActorId = useMemo(() => {
    const next: Record<string, Run> = {};
    for (const run of scopedRuns) {
      if (TERMINAL_RUN_STATUSES.has(run.status)) continue;
      next[run.actorId] = pickNewerRun(next[run.actorId], run);
    }
    return next;
  }, [scopedRuns]);

  const expandedSet = useMemo(() => new Set(expandedActorIds), [expandedActorIds]);
  const expandedSetRef = useRef(expandedSet);
  useEffect(() => {
    expandedSetRef.current = expandedSet;
  }, [expandedSet]);

  useEffect(() => {
    const validIds = new Set(relatedActorIds);
    setExpandedActorIds((current) => current.filter((actorId) => validIds.has(actorId)));
  }, [relatedActorIdsKey, relatedActorIds]);

  const refreshPresence = useCallback(async () => {
    if (!enabled || relatedActorIds.length === 0) return;
    const result = await ipc.connectionList({ actorIds: relatedActorIds });
    setPresenceSnapshot(relatedActorIds, result.actorIds);
  }, [enabled, relatedActorIds, setPresenceSnapshot]);

  const refreshStatus = useCallback(async () => {
    const actorIds = relatedActorIds;
    statusRequestIdRef.current += 1;
    const requestId = statusRequestIdRef.current;
    if (!enabled || actorIds.length === 0) {
      setStatusByActorId({});
      setError(null);
      return;
    }
    try {
      const result = await ipc.inboxStatus({ actorIds });
      if (requestId !== statusRequestIdRef.current) return;
      const actorIdSet = new Set(actorIds);
      const next = Object.fromEntries(
        actorIds.map((actorId) => [actorId, { actorId, pending: 0 } satisfies InboxActorStatus]),
      );
      for (const status of result.actors) {
        if (actorIdSet.has(status.actorId)) next[status.actorId] = status;
      }
      setStatusByActorId(next);
      setError(null);
    } catch (err) {
      if (requestId === statusRequestIdRef.current) setError(errorText(err));
    }
  }, [enabled, relatedActorIds]);

  const loadQueue = useCallback(async (actorId: string) => {
    if (!enabled) return;
    setQueuesByActorId((current) => ({
      ...current,
      [actorId]: {
        items: current[actorId]?.items ?? [],
        loading: true,
        error: null,
      },
    }));
    try {
      const [statusResult, listResult] = await Promise.all([
        ipc.inboxStatus({ actorIds: [actorId], includeEntries: true, entryLimit: 50 }),
        ipc.inboxList({ actorId, state: "pending", limit: 50 }),
      ]);
      const status = statusResult.actors.find((item) => item.actorId === actorId);
      const items = queueItemsFromResults(actorId, actors, status, listResult.deliveries);
      setQueuesByActorId((current) => ({
        ...current,
        [actorId]: { items, loading: false, error: null },
      }));
    } catch (err) {
      setQueuesByActorId((current) => ({
        ...current,
        [actorId]: {
          items: current[actorId]?.items ?? [],
          loading: false,
          error: errorText(err),
        },
      }));
    }
  }, [actors, enabled]);

  const refreshAfterDeliveryAction = useCallback(async (actorId: string) => {
    await refreshStatus();
    if (expandedSetRef.current.has(actorId)) await loadQueue(actorId);
  }, [loadQueue, refreshStatus]);

  const runBusyAction = useCallback(async (key: string, action: () => Promise<void>) => {
    setBusyAction(key);
    try {
      await action();
    } finally {
      setBusyAction((current) => (current === key ? null : current));
    }
  }, []);

  const stopRun = useCallback(async (runId: string) => {
    await runBusyAction(`stop:${runId}`, async () => {
      await ipc.runCancel({ runId });
      await refreshStatus();
    });
  }, [refreshStatus, runBusyAction]);

  const cancelOne = useCallback(async (actorId: string, sourceId: string) => {
    await runBusyAction(`cancel:${actorId}:${sourceId}`, async () => {
      await ipc.deliveryCancel({ actorId, sourceIds: [sourceId] });
      await refreshAfterDeliveryAction(actorId);
    });
  }, [refreshAfterDeliveryAction, runBusyAction]);

  const cancelAll = useCallback(async (actorId: string) => {
    await runBusyAction(`cancel-all:${actorId}`, async () => {
      await ipc.deliveryCancel({ actorId, allPending: true });
      await refreshAfterDeliveryAction(actorId);
    });
  }, [refreshAfterDeliveryAction, runBusyAction]);

  const expedite = useCallback(async (actorId: string, sourceId: string) => {
    await runBusyAction(`expedite:${actorId}:${sourceId}`, async () => {
      await ipc.deliveryExpedite({ actorId, sourceId });
      await refreshAfterDeliveryAction(actorId);
    });
  }, [refreshAfterDeliveryAction, runBusyAction]);

  const toggleAgentQueue = useCallback((actorId: string) => {
    const isExpanded = expandedSetRef.current.has(actorId);
    if (!isExpanded) void loadQueue(actorId);
    setExpandedActorIds((current) =>
      current.includes(actorId)
        ? current.filter((item) => item !== actorId)
        : [...current, actorId],
    );
  }, [loadQueue]);

  useEffect(() => {
    void refreshPresence().catch(() => {});
  }, [refreshPresence]);

  useEffect(() => {
    void refreshStatus();
    if (!enabled || relatedActorIds.length === 0) return;
    const interval = window.setInterval(() => {
      void refreshStatus();
    }, 30_000);
    return () => window.clearInterval(interval);
  }, [enabled, refreshStatus, relatedActorIds.length]);

  useEffect(() => {
    if (!enabled || relatedActorIds.length === 0) return;
    if (deliveryTick === 0 && presenceTick === 0) return;
    const timer = window.setTimeout(() => {
      void refreshStatus();
    }, 1_000);
    return () => window.clearTimeout(timer);
  }, [deliveryTick, enabled, presenceTick, refreshStatus, relatedActorIds.length]);

  const agents = useMemo<AgentActivityAgent[]>(() =>
    relatedActorIds.map((actorId) => {
      const actor = actors[actorId] ?? fallbackAgent(actorId);
      const inboxStatus = statusByActorId[actorId];
      const pending = inboxStatus?.pending ?? 0;
      const activeRun = activeRunsByActorId[actorId] ?? null;
      const presenceKnown = Object.prototype.hasOwnProperty.call(onlineByActorId, actorId);
      const online = onlineByActorId[actorId] === true;
      let status: AgentActivityStatus = "idle";
      if (activeRun) {
        status = online ? "running" : "unknown";
      } else if (presenceKnown && !online) {
        status = "offline";
      } else if (pending > 0) {
        status = "queued";
      }
      const queueState = queuesByActorId[actorId];
      return {
        actorId,
        actor,
        status,
        online,
        pending,
        oldestPendingAt: inboxStatus?.oldestPendingAt,
        activeRun,
        expanded: expandedSet.has(actorId),
        queue: queueState?.items ?? [],
        queueLoading: queueState?.loading ?? false,
        queueError: queueState?.error ?? null,
      };
    }),
    [
      activeRunsByActorId,
      actors,
      expandedSet,
      onlineByActorId,
      queuesByActorId,
      relatedActorIds,
      statusByActorId,
    ],
  );

  const runningCount = agents.filter((agent) => agent.status === "running").length;
  const pendingTotal = agents.reduce((sum, agent) => sum + agent.pending, 0);
  const visibleAgents = agents.filter((agent) => agent.status !== "idle");

  return {
    agents,
    visibleAgents,
    runningCount,
    pendingTotal,
    hasActivity: visibleAgents.length > 0,
    busyAction,
    error,
    refresh: refreshStatus,
    toggleAgentQueue,
    loadQueue,
    stopRun,
    cancelOne,
    cancelAll,
    expedite,
  };
}
