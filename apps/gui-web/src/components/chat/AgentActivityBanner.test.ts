// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ActorActivityBanner } from "@/components/chat/ActorActivityBanner";
import { useAgentActivity, type AgentActivityAgent } from "@/hooks/useAgentActivity";
import type {
  Actor,
  MachineInfo,
  Run,
  ScopeRef,
  ServiceRuntimeState,
} from "@/ipc/types";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/hooks/useAgentActivity", () => ({
  useAgentActivity: vi.fn(),
}));

const roots: Root[] = [];
const scope: ScopeRef = { kind: "channel", id: "channel-status" };
const primaryActor: Actor = {
  id: "actor_agent_primary",
  kind: "agent",
  displayName: "Primary Agent",
};
const secondaryActor: Actor = {
  id: "actor_agent_secondary",
  kind: "agent",
  displayName: "Secondary Agent",
};
const serviceActor: Actor = {
  id: "actor_service_indexer",
  kind: "service",
  displayName: "Indexer",
};

function makeRun(id: string, actorId: string, status: Run["status"]): Run {
  return {
    id,
    actorId,
    scope,
    agentConfigVersionId: "config-1",
    status,
    openedAt: "2026-08-09T02:00:00Z",
    metadata: {},
  };
}

function makeAgent(
  actor: Actor,
  run: Run | null,
  status: AgentActivityAgent["status"] = "running",
): AgentActivityAgent {
  return {
    actorId: actor.id,
    actor,
    status,
    online: status !== "offline" && status !== "unknown",
    pending: 0,
    activeRun: run,
    expanded: false,
    queue: [],
    queueLoading: false,
    queueError: null,
  };
}

function mockActivity(visibleAgents: AgentActivityAgent[]) {
  const stopRun = vi.fn(async () => {});
  vi.mocked(useAgentActivity).mockReturnValue({
    agents: visibleAgents,
    visibleAgents,
    activeCount: visibleAgents.filter((agent) => agent.activeRun).length,
    primaryAgent: visibleAgents.find((agent) => agent.activeRun) ?? null,
    runningCount: visibleAgents.filter((agent) => agent.status === "running").length,
    pendingTotal: visibleAgents.reduce((total, agent) => total + agent.pending, 0),
    hasActivity: visibleAgents.length > 0,
    busyAction: null,
    error: null,
    refresh: vi.fn(async () => {}),
    toggleAgentQueue: vi.fn(),
    loadQueue: vi.fn(async () => {}),
    stopRun,
    cancelOne: vi.fn(async () => {}),
    cancelAll: vi.fn(async () => {}),
    expedite: vi.fn(async () => {}),
  });
  return stopRun;
}

function makeServiceRuntime(
  runtimeId: string,
  runtimeScope: ScopeRef,
  phase: ServiceRuntimeState["phase"] = "running",
  lastError: string | null = null,
): ServiceRuntimeState {
  return {
    runtimeId,
    machineId: "machine-local",
    serviceId: "service-indexer",
    actorId: serviceActor.id,
    pluginKind: "indexer",
    lifecycle: "thread_bound",
    instanceId: runtimeId,
    scopes: [runtimeScope],
    phase,
    startedAt: "2026-08-09T02:00:00Z",
    updatedAt: "2026-08-09T02:01:00Z",
    lastError,
  };
}

function makeMachine(serviceRuntimeStates: ServiceRuntimeState[]): MachineInfo {
  return {
    id: "machine-local",
    name: "Local host",
    kind: "local",
    source: "local",
    readOnly: false,
    canCommand: true,
    canOpenLocalPath: true,
    capabilities: [],
    inventoryRevision: 1,
    status: "online",
    setupStatus: "ready",
    connectionStatus: "connected",
    connectionActorId: "actor_human_local",
    dataRoot: "",
    configDir: "",
    agentCount: 0,
    onlineAgentCount: 0,
    serviceCount: 1,
    providers: [],
    agents: [],
    services: [{
      id: "service-indexer",
      kind: "indexer",
      displayName: "Indexer",
      actor: serviceActor,
      lifecycle: "thread_bound",
    }],
    serviceRuntimeStates,
    serveCommand: "",
    setupScript: "",
  };
}

function renderBanner(
  actors: Actor[],
  options: { machines?: MachineInfo[]; activityScope?: ScopeRef } = {},
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);
  React.act(() => {
    root.render(React.createElement(ActorActivityBanner, {
      actors: Object.fromEntries(actors.map((actor) => [actor.id, actor])),
      actorIds: actors.map((actor) => actor.id),
      machines: options.machines ?? [],
      runs: {},
      scope: options.activityScope ?? scope,
    }));
  });
  return container;
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
});

describe("ActorActivityBanner", () => {
  it("shows the primary agent status and keeps expansion separate from confirmed Stop", () => {
    const run = makeRun("run-primary", primaryActor.id, "running");
    const stopRun = mockActivity([makeAgent(primaryActor, run)]);
    const container = renderBanner([primaryActor]);

    expect(container.textContent).toContain("Primary Agent");
    expect(container.textContent).toContain("Thinking");
    expect(container.querySelector('button[aria-label="Expand actor activity"]')).not.toBeNull();

    const stop = container.querySelector<HTMLButtonElement>(
      'button[title="Stop Primary Agent\'s run"]',
    );
    expect(stop).not.toBeNull();

    React.act(() => stop!.click());
    expect(stopRun).not.toHaveBeenCalled();
    expect(stop!.textContent).toContain("Stop?");
    expect(container.textContent).not.toContain("Actor activity");

    React.act(() => stop!.click());
    expect(stopRun).toHaveBeenCalledWith(run.id);
    expect(container.textContent).not.toContain("Actor activity");

    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Expand actor activity"]')!.click();
    });
    expect(container.textContent).toContain("Actor activity");
  });

  it("summarizes additional agents and exposes Force stop for an offline worker", () => {
    const primaryRun = makeRun("run-primary", primaryActor.id, "running");
    const offlineRun = makeRun("run-offline", secondaryActor.id, "waiting_tool");
    mockActivity([
      makeAgent(primaryActor, primaryRun),
      makeAgent(secondaryActor, offlineRun, "unknown"),
    ]);
    const container = renderBanner([primaryActor, secondaryActor]);

    expect(container.textContent).toContain("Primary Agent");
    expect(container.textContent).toContain("+1 other");

    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Expand actor activity"]')!.click();
    });
    expect(container.textContent).toContain("Status unknown · worker offline");
    expect(container.textContent).toContain("Force stop");
  });

  it("keeps agent run controls while showing a service as runtime activity", () => {
    const run = makeRun("run-primary", primaryActor.id, "running");
    mockActivity([makeAgent(primaryActor, run)]);
    const machine = makeMachine([makeServiceRuntime("runtime-channel", scope)]);
    const container = renderBanner([primaryActor, serviceActor], { machines: [machine] });

    expect(container.textContent).toContain("+1 other");
    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Expand actor activity"]')!.click();
    });

    expect(container.textContent).toContain("Primary Agent");
    expect(container.textContent).toContain("Indexer");
    expect(container.textContent).toContain("Host: Local host");
    expect(container.textContent).toContain("Plugin: indexer");
    const serviceRow = container.querySelector('[data-actor-activity-kind="service"]');
    expect(serviceRow).not.toBeNull();
    expect(serviceRow!.querySelector("button")).toBeNull();
    expect(container.textContent).toContain("Stop");
  });

  it("shows a service without inventing run or Stop controls", () => {
    mockActivity([]);
    const machine = makeMachine([makeServiceRuntime("runtime-service-only", scope)]);
    const container = renderBanner([serviceActor], { machines: [machine] });

    expect(container.textContent).toContain("Indexer");
    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Expand actor activity"]')!.click();
    });

    const serviceRow = container.querySelector('[data-actor-activity-kind="service"]');
    expect(serviceRow?.textContent).toContain("Running");
    expect(serviceRow?.textContent).toContain("1 instance");
    expect(serviceRow?.textContent).not.toContain("run");
    expect(serviceRow?.textContent).not.toContain("Stop");
    expect(serviceRow?.querySelector("button")).toBeNull();
  });

  it("does not merge service instances from a different thread", () => {
    mockActivity([]);
    const currentThread: ScopeRef = { kind: "thread", id: "thread-current" };
    const otherThread: ScopeRef = { kind: "thread", id: "thread-other" };
    const machine = makeMachine([
      makeServiceRuntime("runtime-current", currentThread),
      makeServiceRuntime("runtime-other", otherThread, "failed", "wrong thread failure"),
    ]);
    const container = renderBanner([serviceActor], {
      machines: [machine],
      activityScope: currentThread,
    });

    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Expand actor activity"]')!.click();
    });
    const serviceRow = container.querySelector('[data-actor-activity-kind="service"]');
    expect(serviceRow?.textContent).toContain("Running");
    expect(serviceRow?.textContent).toContain("1 instance");
    expect(serviceRow?.textContent).not.toContain("wrong thread failure");
    expect(serviceRow?.textContent).not.toContain("Failed");
  });
});
