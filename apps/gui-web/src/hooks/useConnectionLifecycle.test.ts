// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useConnectionLifecycle } from "@/hooks/useConnectionLifecycle";
import { createConnectionGenerationState } from "@/hooks/connectionGeneration";
import {
  useWorkspaceConnection,
  type WorkspaceConnectionDeps,
} from "@/hooks/useWorkspaceConnection";
import * as ipc from "@/ipc/bridge";
import type { ConnectionEvent, Workspace } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/ipc/bridge");

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function workspace(id: string): Workspace {
  return {
    id,
    name: `Workspace ${id}`,
    serverUrl: "ws://localhost:7878/rpc",
    actorId: "actor_human_local_test",
    displayName: "Test User",
  };
}

interface Harness {
  root: Root;
  connect: ReturnType<typeof useWorkspaceConnection>["connectWorkspace"];
  connectionEvents: ConnectionState[];
  errors: Array<string | null>;
  workspaces: Workspace[];
  emitConnection: (event: ConnectionEvent) => void;
}

async function renderHarness(options: {
  config?: WorkspaceConnectionDeps["config"];
  requestServerPassword?: WorkspaceConnectionDeps["requestServerPassword"];
} = {}): Promise<Harness> {
  const connectionEvents: ConnectionState[] = [];
  const errors: Array<string | null> = [];
  const workspaces: Workspace[] = [];
  let connectionListener: ((event: ConnectionEvent) => void) | null = null;
  let connectionApi: ReturnType<typeof useWorkspaceConnection> | null = null;

  vi.mocked(ipc.onStream).mockResolvedValue(() => {});
  vi.mocked(ipc.onConnection).mockImplementation(async (listener) => {
    connectionListener = listener;
    return () => {
      connectionListener = null;
    };
  });
  vi.mocked(ipc.workspacesList).mockResolvedValue(options.config ?? { workspaces: [] });
  vi.mocked(ipc.machineList).mockResolvedValue({ machines: [] });
  vi.mocked(ipc.machineCheck).mockResolvedValue({ machines: [] });
  vi.mocked(ipc.actorList).mockResolvedValue({ actors: [] });
  vi.mocked(ipc.channelList).mockResolvedValue({ channels: [] });
  vi.mocked(ipc.taskList).mockResolvedValue({ tasks: [] });
  vi.mocked(ipc.inboxList).mockResolvedValue({ deliveries: [] });

  const workspaceRef: React.MutableRefObject<Workspace | null> = { current: null };
  const autoReconnectRef = { current: false };
  const hasOpenedConnectionRef = { current: false };
  const reconnectTimerRef: React.MutableRefObject<number | null> = { current: null };
  const reconnectAttemptRef = { current: 0 };
  const actorIdRef: React.MutableRefObject<string | null> = { current: null };

  const deps: WorkspaceConnectionDeps = {
    config: options.config ?? { workspaces: [] },
    setConfig: () => {},
    workspace: null,
    setWorkspace: (next) => {
      const value = typeof next === "function" ? next(workspaces.at(-1) ?? null) : next;
      if (value) workspaces.push(value);
    },
    connection: "idle",
    setConnection: (next) => {
      const previous = connectionEvents.at(-1) ?? "idle";
      connectionEvents.push(typeof next === "function" ? next(previous) : next);
    },
    setError: (error) => errors.push(error),
    setNotice: () => {},
    setBusy: () => {},
    setWorkspaceForm: () => {},
    setView: () => {},
    setChannels: () => {},
    setActiveChannelId: () => {},
    setActors: () => {},
    setTasks: () => {},
    setInbox: () => {},
    setMachines: () => {},
    setAgentForm: () => {},
    setOnboardingActive: () => {},
    setConfigLoaded: () => {},
    account: null,
    workspaceRef,
    autoReconnectRef,
    hasOpenedConnectionRef,
    reconnectTimerRef,
    reconnectAttemptRef,
    actorIdRef,
    serverPasswordsRef: { current: new Map() },
    requestServerPassword: options.requestServerPassword ?? (async () => null),
  };

  function Probe() {
    const api = useWorkspaceConnection(deps);
    connectionApi = api;
    useConnectionLifecycle({
      loadConfig: api.loadConfig,
      loadMachines: api.loadMachines,
      handleStream: () => {},
      connectWorkspace: api.connectWorkspace,
      setConnection: (next) => connectionEvents.push(next),
      setError: (error) => errors.push(error),
      connection: "idle",
      account: null,
      workspace: null,
      workspaceRef,
      autoReconnectRef,
      hasOpenedConnectionRef,
      reconnectTimerRef,
      reconnectAttemptRef,
      clearReconnectTimer: api.clearReconnectTimer,
      connectionGenerationRef: api.connectionGenerationRef,
    });
    return null;
  }

  const container = document.createElement("div");
  const root = createRoot(container);
  await React.act(async () => {
    root.render(React.createElement(Probe));
    await Promise.resolve();
    await Promise.resolve();
  });

  const initializedApi = connectionApi as ReturnType<typeof useWorkspaceConnection> | null;
  const initializedListener = connectionListener as ((event: ConnectionEvent) => void) | null;
  if (!initializedApi || !initializedListener) {
    throw new Error("connection hooks did not initialize");
  }

  return {
    root,
    connect: initializedApi.connectWorkspace,
    connectionEvents,
    errors,
    workspaces,
    emitConnection: initializedListener,
  };
}

describe("connection generation ordering", () => {
  let roots: Root[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
    roots = [];
  });

  afterEach(() => {
    for (const root of roots) React.act(() => root.unmount());
  });

  it("does not reopen when Closed arrives before connect resolves", async () => {
    const pending = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    vi.mocked(ipc.connect).mockReturnValue(pending.promise);
    const harness = await renderHarness();
    roots.push(harness.root);

    let resultPromise!: Promise<Workspace | null>;
    React.act(() => {
      resultPromise = harness.connect("one", { automatic: true, quiet: true });
    });
    React.act(() => {
      harness.emitConnection({
        state: "closed",
        connectionId: 11,
        reason: "transport lost",
      });
    });
    pending.resolve({ workspace: workspace("one"), open: {}, connectionId: 11 });

    let result: Workspace | null = workspace("unexpected");
    await React.act(async () => {
      result = await resultPromise;
    });

    expect(result).toBeNull();
    expect(harness.connectionEvents).toContain("closed");
    expect(harness.connectionEvents.at(-1)).not.toBe("open");
    expect(harness.workspaces).toEqual([]);
  });

  it("prompts and retries when a newly saved server requires a password", async () => {
    const protectedWorkspace = workspace("protected");
    const requestServerPassword = vi.fn(async () => "correct horse battery staple");
    vi.mocked(ipc.connect)
      .mockRejectedValueOnce(new Error("LOOM_AUTH_REQUIRED: server password required"))
      .mockResolvedValueOnce({
        workspace: protectedWorkspace,
        open: {},
        connectionId: 12,
      });
    const harness = await renderHarness({
      // This deliberately stays stale to cover the first-connect path after
      // configureWebConnection has written the authoritative profile.
      config: { workspaces: [] },
      requestServerPassword,
    });
    vi.mocked(ipc.workspacesList).mockResolvedValue({
      active: protectedWorkspace.id,
      workspaces: [protectedWorkspace],
    });
    roots.push(harness.root);

    let result!: Workspace | null;
    await React.act(async () => {
      result = await harness.connect(protectedWorkspace.id, { quiet: true });
    });

    expect(result?.id).toBe(protectedWorkspace.id);
    expect(requestServerPassword).toHaveBeenCalledWith({
      serverName: protectedWorkspace.name,
      serverUrl: protectedWorkspace.serverUrl,
      invalid: false,
    });
    expect(vi.mocked(ipc.connect).mock.calls).toEqual([
      [protectedWorkspace.id, undefined],
      [protectedWorkspace.id, "correct horse battery staple"],
    ]);
  });

  it("ignores a late failure from an older connect attempt", async () => {
    const first = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    const second = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    vi.mocked(ipc.connect)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const harness = await renderHarness();
    roots.push(harness.root);

    let firstResult!: Promise<Workspace | null>;
    let secondResult!: Promise<Workspace | null>;
    React.act(() => {
      firstResult = harness.connect("one", { automatic: true, quiet: true });
      secondResult = harness.connect("two", { automatic: true, quiet: true });
    });
    second.resolve({ workspace: workspace("two"), open: {}, connectionId: 22 });
    await React.act(async () => {
      await secondResult;
    });
    const eventCount = harness.connectionEvents.length;
    const errorCount = harness.errors.length;

    first.reject(new Error("late connect failure"));
    await React.act(async () => {
      expect(await firstResult).toBeNull();
    });

    expect(harness.connectionEvents).toHaveLength(eventCount);
    expect(harness.errors).toHaveLength(errorCount);
    expect(harness.workspaces.at(-1)?.id).toBe("two");
  });

  it("ignores a late successful result from an older connect attempt", async () => {
    const first = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    const second = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    vi.mocked(ipc.connect)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const harness = await renderHarness();
    roots.push(harness.root);

    let firstResult!: Promise<Workspace | null>;
    let secondResult!: Promise<Workspace | null>;
    React.act(() => {
      firstResult = harness.connect("one", { automatic: true, quiet: true });
      secondResult = harness.connect("two", { automatic: true, quiet: true });
    });
    second.resolve({ workspace: workspace("two"), open: {}, connectionId: 42 });
    await React.act(async () => {
      expect((await secondResult)?.id).toBe("two");
    });
    const eventCount = harness.connectionEvents.length;
    const errorCount = harness.errors.length;

    first.resolve({ workspace: workspace("one"), open: {}, connectionId: 41 });
    await React.act(async () => {
      expect(await firstResult).toBeNull();
    });

    expect(harness.connectionEvents).toHaveLength(eventCount);
    expect(harness.errors).toHaveLength(errorCount);
    expect(harness.workspaces.map((item) => item.id)).toEqual(["two"]);
  });

  it("buffers an older attempt's Open while the newest attempt is unbound", async () => {
    const first = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    const second = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    vi.mocked(ipc.connect)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const harness = await renderHarness();
    roots.push(harness.root);

    let firstResult!: Promise<Workspace | null>;
    let secondResult!: Promise<Workspace | null>;
    React.act(() => {
      firstResult = harness.connect("one", { automatic: true, quiet: true });
      secondResult = harness.connect("two", { automatic: true, quiet: true });
      harness.emitConnection({ state: "open", connectionId: 51 });
    });
    expect(harness.connectionEvents).not.toContain("open");

    second.resolve({ workspace: workspace("two"), open: {}, connectionId: 52 });
    await React.act(async () => {
      expect((await secondResult)?.id).toBe("two");
    });
    expect(harness.connectionEvents.at(-1)).toBe("open");

    first.resolve({ workspace: workspace("one"), open: {}, connectionId: 51 });
    await React.act(async () => {
      expect(await firstResult).toBeNull();
    });
    expect(harness.workspaces.map((item) => item.id)).toEqual(["two"]);
  });

  it("buffers an older attempt's Closed while the newest attempt is unbound", async () => {
    const first = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    const second = deferred<Awaited<ReturnType<typeof ipc.connect>>>();
    vi.mocked(ipc.connect)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const harness = await renderHarness();
    roots.push(harness.root);

    let firstResult!: Promise<Workspace | null>;
    let secondResult!: Promise<Workspace | null>;
    React.act(() => {
      firstResult = harness.connect("one", { automatic: true, quiet: true });
      secondResult = harness.connect("two", { automatic: true, quiet: true });
      harness.emitConnection({
        state: "closed",
        connectionId: 61,
        reason: "older attempt closed",
      });
    });
    expect(harness.connectionEvents).not.toContain("closed");

    second.resolve({ workspace: workspace("two"), open: {}, connectionId: 62 });
    await React.act(async () => {
      expect((await secondResult)?.id).toBe("two");
    });
    first.resolve({ workspace: workspace("one"), open: {}, connectionId: 61 });
    await React.act(async () => {
      expect(await firstResult).toBeNull();
    });

    expect(harness.connectionEvents.at(-1)).toBe("open");
    expect(harness.connectionEvents).not.toContain("closed");
    expect(harness.workspaces.map((item) => item.id)).toEqual(["two"]);
  });

  it("ignores Open and Closed events from an older backend generation", async () => {
    vi.mocked(ipc.connect)
      .mockResolvedValueOnce({ workspace: workspace("one"), open: {}, connectionId: 31 })
      .mockResolvedValueOnce({ workspace: workspace("two"), open: {}, connectionId: 32 });
    const harness = await renderHarness();
    roots.push(harness.root);

    await React.act(async () => {
      await harness.connect("one", { automatic: true, quiet: true });
      await harness.connect("two", { automatic: true, quiet: true });
    });
    const eventCount = harness.connectionEvents.length;

    React.act(() => {
      harness.emitConnection({ state: "closed", connectionId: 31, reason: "late" });
      harness.emitConnection({ state: "open", connectionId: 31 });
    });
    expect(harness.connectionEvents).toHaveLength(eventCount);

    React.act(() => {
      harness.emitConnection({ state: "closed", connectionId: 32, reason: "current" });
    });
    expect(harness.connectionEvents.at(-1)).toBe("closed");
  });

  it("disposes listeners that finish registering after unmount", async () => {
    const streamRegistration = deferred<() => void>();
    const connectionRegistration = deferred<() => void>();
    const offStream = vi.fn();
    const offConnection = vi.fn();
    const loadConfig = vi.fn(async () => {});
    vi.mocked(ipc.onStream).mockReturnValue(streamRegistration.promise);
    vi.mocked(ipc.onConnection).mockReturnValue(connectionRegistration.promise);

    const root = createRoot(document.createElement("div"));
    const workspaceRef: React.MutableRefObject<Workspace | null> = { current: null };
    function Probe() {
      useConnectionLifecycle({
        loadConfig,
        loadMachines: async () => [],
        handleStream: () => {},
        connectWorkspace: async () => null,
        setConnection: () => {},
        setError: () => {},
        connection: "idle",
        account: null,
        workspace: null,
        workspaceRef,
        autoReconnectRef: { current: false },
        hasOpenedConnectionRef: { current: false },
        reconnectTimerRef: { current: null },
        reconnectAttemptRef: { current: 0 },
        clearReconnectTimer: () => {},
        connectionGenerationRef: { current: createConnectionGenerationState() },
      });
      return null;
    }

    React.act(() => root.render(React.createElement(Probe)));
    React.act(() => root.unmount());
    streamRegistration.resolve(offStream);
    connectionRegistration.resolve(offConnection);
    await React.act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(offStream).toHaveBeenCalledOnce();
    expect(offConnection).toHaveBeenCalledOnce();
    expect(loadConfig).not.toHaveBeenCalled();
  });
});
