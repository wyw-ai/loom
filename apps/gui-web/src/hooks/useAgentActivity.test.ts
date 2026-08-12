// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "react-dom/client";
import React from "react";

import { useAgentActivity } from "@/hooks/useAgentActivity";
import * as ipc from "@/ipc/bridge";
import type { Actor, InboxStatusEntry, InboxStatusResult, Run, ScopeRef } from "@/ipc/types";
import { usePresenceStore } from "@/store/presenceStore";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/ipc/bridge");

const agentId = "actor_agent_test";
const actors: Record<string, Actor> = {
  [agentId]: { id: agentId, kind: "agent", displayName: "Test agent" },
};
const agentActorIds = [agentId];
const runs: Record<string, Run> = {};
const scope: ScopeRef = { kind: "channel", id: "channel_test" };

describe("useAgentActivity queue expansion", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    usePresenceStore.setState({
      onlineByActorId: {},
      presenceTick: 0,
      deliveryTick: 0,
    });
    vi.mocked(ipc.connectionList).mockResolvedValue({ actorIds: [] });
    vi.mocked(ipc.inboxStatus).mockImplementation(async (params) => ({
      actors: [{
        actorId: agentId,
        pending: 1,
        entries: params.includeEntries
          ? [{ sourceId: "msg_queued_1", updatedAt: "2026-08-09T01:02:03Z" }]
          : [],
      }],
    }));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses metadata-only inbox.status instead of reading another actor's inbox", async () => {
    let activity: ReturnType<typeof useAgentActivity> | null = null;

    function Probe() {
      activity = useAgentActivity({
        actors,
        agentActorIds,
        runs,
        scope,
      });
      return null;
    }

    const container = document.createElement("div");
    const root = createRoot(container);
    React.act(() => root.render(React.createElement(Probe)));

    await React.act(async () => {
      activity!.toggleAgentQueue(agentId);
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });

    expect(ipc.inboxStatus).toHaveBeenCalledWith({
      actorIds: [agentId],
      includeEntries: true,
      entryLimit: 50,
    });
    expect(ipc.inboxList).not.toHaveBeenCalled();
    expect(activity!.agents[0]?.queue).toEqual([{
      sourceId: "msg_queued_1",
      updatedAt: "2026-08-09T01:02:03Z",
      sourceKind: "Message",
    }]);
    expect(activity!.agents[0]?.expanded).toBe(true);
    expect(activity!.agents[0]?.queueError).toBeNull();

    React.act(() => root.unmount());
  });

  it("reloads an expanded queue when delivery state changes", async () => {
    let activity: ReturnType<typeof useAgentActivity> | null = null;
    let entries: InboxStatusEntry[] = [
      { sourceId: "msg_queued_1", updatedAt: "2026-08-09T01:02:03Z" },
    ];
    vi.mocked(ipc.inboxStatus).mockImplementation(async (params) => ({
      actors: [{
        actorId: agentId,
        pending: entries.length,
        entries: params.includeEntries ? [...entries] : [],
      }],
    }));

    function Probe() {
      activity = useAgentActivity({ actors, agentActorIds, runs, scope });
      return null;
    }

    const container = document.createElement("div");
    const root = createRoot(container);
    React.act(() => root.render(React.createElement(Probe)));

    await React.act(async () => {
      activity!.toggleAgentQueue(agentId);
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });
    expect(activity!.agents[0]?.queue.map((item) => item.sourceId)).toEqual([
      "msg_queued_1",
    ]);

    entries = [
      ...entries,
      { sourceId: "msg_queued_2", updatedAt: "2026-08-09T01:02:04Z" },
    ];
    React.act(() => {
      usePresenceStore.getState().bumpDeliveryTick();
    });
    await React.act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 1_100));
    });

    expect(activity!.agents[0]?.pending).toBe(2);
    expect(activity!.agents[0]?.queue.map((item) => item.sourceId)).toEqual([
      "msg_queued_1",
      "msg_queued_2",
    ]);

    React.act(() => root.unmount());
  });

  it("does not let an older queue request overwrite a newer result", async () => {
    let activity: ReturnType<typeof useAgentActivity> | null = null;
    const pendingResolvers: Array<(result: InboxStatusResult) => void> = [];
    vi.mocked(ipc.inboxStatus).mockImplementation((params) => {
      if (!params.includeEntries) {
        return Promise.resolve({ actors: [{ actorId: agentId, pending: 0, entries: [] }] });
      }
      return new Promise<InboxStatusResult>((resolve) => pendingResolvers.push(resolve));
    });

    function Probe() {
      activity = useAgentActivity({ actors, agentActorIds, runs, scope });
      return null;
    }

    const container = document.createElement("div");
    const root = createRoot(container);
    React.act(() => root.render(React.createElement(Probe)));
    await React.act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });

    let olderRequest!: Promise<void>;
    let newerRequest!: Promise<void>;
    await React.act(async () => {
      olderRequest = activity!.loadQueue(agentId);
      newerRequest = activity!.loadQueue(agentId);
      await Promise.resolve();
    });
    expect(pendingResolvers).toHaveLength(2);

    await React.act(async () => {
      pendingResolvers[1]!({
        actors: [{
          actorId: agentId,
          pending: 2,
          entries: [
            { sourceId: "msg_newer_1", updatedAt: "2026-08-09T01:02:03Z" },
            { sourceId: "msg_newer_2", updatedAt: "2026-08-09T01:02:04Z" },
          ],
        }],
      });
      await newerRequest;
    });
    await React.act(async () => {
      pendingResolvers[0]!({
        actors: [{
          actorId: agentId,
          pending: 1,
          entries: [{ sourceId: "msg_stale", updatedAt: "2026-08-09T01:02:02Z" }],
        }],
      });
      await olderRequest;
    });

    expect(activity!.agents[0]?.queue.map((item) => item.sourceId)).toEqual([
      "msg_newer_1",
      "msg_newer_2",
    ]);

    React.act(() => root.unmount());
  });

  it("derives thread agents only from non-terminal runs in the exact thread", async () => {
    let activity: ReturnType<typeof useAgentActivity> | null = null;
    const currentAgentId = "actor_agent_current_thread";
    const otherAgentId = "actor_agent_other_thread";
    const historicalAgentId = "actor_agent_historical_thread";
    const threadActors: Record<string, Actor> = {
      [currentAgentId]: { id: currentAgentId, kind: "agent", displayName: "Current" },
      [otherAgentId]: { id: otherAgentId, kind: "agent", displayName: "Other" },
      [historicalAgentId]: { id: historicalAgentId, kind: "agent", displayName: "Historical" },
    };
    const currentScope: ScopeRef = { kind: "thread", id: "thread-current" };
    const threadRuns: Record<string, Run> = {
      current: {
        id: "run-current",
        actorId: currentAgentId,
        scope: currentScope,
        agentConfigVersionId: "config-1",
        status: "running",
        openedAt: "2026-08-09T01:00:00Z",
        metadata: {},
      },
      other: {
        id: "run-other",
        actorId: otherAgentId,
        scope: { kind: "thread", id: "thread-other" },
        agentConfigVersionId: "config-1",
        status: "running",
        openedAt: "2026-08-09T01:00:01Z",
        metadata: {},
      },
      historical: {
        id: "run-historical",
        actorId: historicalAgentId,
        scope: currentScope,
        agentConfigVersionId: "config-1",
        status: "completed",
        openedAt: "2026-08-09T00:00:00Z",
        metadata: {},
      },
    };

    function Probe() {
      activity = useAgentActivity({
        actors: threadActors,
        agentActorIds: [],
        runs: threadRuns,
        scope: currentScope,
      });
      return null;
    }

    const container = document.createElement("div");
    const root = createRoot(container);
    React.act(() => root.render(React.createElement(Probe)));
    await React.act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });

    expect(activity!.agents.map((agent) => agent.actorId)).toEqual([currentAgentId]);
    expect(activity!.agents[0]?.activeRun?.id).toBe("run-current");

    React.act(() => root.unmount());
  });
});
