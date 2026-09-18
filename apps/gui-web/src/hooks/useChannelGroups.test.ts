// @vitest-environment jsdom
import React, { useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChannelLayout, ChannelLayoutResult } from "@/ipc/types";
import type { ChannelGroup, ConnectionState } from "@/lib/types";
import {
  channelGroupDirtyStorageKey,
  channelGroupMigrationStorageKey,
  hasDirtyChannelGroups,
  hasMigratedChannelGroups,
  loadChannelGroups,
  saveChannelGroups,
} from "@/lib/channel-utils";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ipcMocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
}));

vi.mock("@/ipc/bridge", () => ({
  channelLayoutGet: ipcMocks.get,
  channelLayoutSet: ipcMocks.set,
}));

import {
  isUnsupportedChannelLayoutError,
  useChannelGroups,
} from "@/hooks/useChannelGroups";
import { channelLayoutFromStreamUpdate } from "@/hooks/useStreamHandler";

type HookApi = ReturnType<typeof useChannelGroups>;
type HarnessSnapshot = { groups: ChannelGroup[]; api: HookApi };

const roots: Root[] = [];
let latest: HarnessSnapshot | null = null;

function Harness({
  storageKey,
  connection = "open",
}: {
  storageKey: string;
  connection?: ConnectionState;
}) {
  const [groups, setGroups] = useState<ChannelGroup[]>([]);
  const api = useChannelGroups({
    channelGroups: groups,
    setChannelGroups: setGroups,
    channelGroupsKey: storageKey,
    connection,
    workspaceId: "workspace-test",
  });
  latest = { groups, api };
  return null;
}

function layout(
  sections: ChannelGroup[],
  revision: number,
): ChannelLayout {
  return {
    actorId: "actor_human_test",
    sections,
    revision,
    updatedAt: `2026-08-10T00:00:${String(revision).padStart(2, "0")}Z`,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function settle() {
  await React.act(async () => {
    for (let index = 0; index < 8; index += 1) await Promise.resolve();
  });
}

function renderHarness(storageKey: string, connection: ConnectionState = "open") {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);
  React.act(() => {
    root.render(React.createElement(Harness, { storageKey, connection }));
  });
}

beforeEach(() => {
  latest = null;
  ipcMocks.get.mockReset();
  ipcMocks.set.mockReset();
  localStorage.clear();
});

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
});

describe("useChannelGroups server synchronization", () => {
  it("migrates the existing local layout once with merge=true", async () => {
    const key = "loom:channel-groups:v1:migration";
    const local = [
      {
        id: "local-section",
        title: "Local",
        channelIds: ["channel-3", "channel-1"],
        collapsed: false,
      },
    ];
    const server = [
      {
        id: "server-section",
        title: "Server",
        channelIds: ["channel-2"],
        collapsed: true,
      },
    ];
    const merged = [...server, ...local];
    saveChannelGroups(key, local);
    ipcMocks.get.mockResolvedValue({ layout: layout(server, 3) });
    ipcMocks.set.mockResolvedValue({ layout: layout(merged, 4) });

    renderHarness(key);
    await settle();

    expect(ipcMocks.set).toHaveBeenCalledTimes(1);
    expect(ipcMocks.set).toHaveBeenCalledWith({ sections: local, merge: true });
    expect(latest?.groups).toEqual(merged);
    expect(loadChannelGroups(key)).toEqual(merged);
    expect(hasMigratedChannelGroups(key)).toBe(true);
    expect(hasDirtyChannelGroups(key)).toBe(false);
  });

  it("rebases edits made during migration without replacing server sections", async () => {
    const key = "loom:channel-groups:v1:migration-in-flight-edit";
    const local = [
      {
        id: "local-section",
        title: "Local",
        channelIds: ["channel-local"],
        collapsed: false,
      },
    ];
    const server = [
      {
        id: "server-section",
        title: "Server",
        channelIds: ["channel-server"],
        collapsed: true,
      },
    ];
    const concurrentServer = {
      id: "server-concurrent",
      title: "Server concurrent",
      channelIds: ["channel-concurrent"],
      collapsed: false,
    };
    const latestLocal = [{ ...local[0]!, title: "Local latest" }];
    const rebased = [...server, concurrentServer, ...latestLocal];
    const rebasedNewest = [
      ...server,
      concurrentServer,
      { ...local[0]!, title: "Local newest" },
    ];
    const first = deferred<ChannelLayoutResult>();
    const second = deferred<ChannelLayoutResult>();
    const third = deferred<ChannelLayoutResult>();
    saveChannelGroups(key, local);
    ipcMocks.get.mockResolvedValue({ layout: layout(server, 5) });
    ipcMocks.set
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
      .mockImplementationOnce(() => third.promise);

    renderHarness(key);
    await settle();
    expect(ipcMocks.set).toHaveBeenCalledTimes(1);
    expect(ipcMocks.set).toHaveBeenLastCalledWith({
      sections: local,
      merge: true,
    });

    React.act(() => {
      latest?.api.renameChannelGroup("local-section", "Local latest");
    });
    expect(latest?.groups).toEqual(latestLocal);

    first.resolve({
      layout: layout([...server, concurrentServer, ...local], 6),
    });
    await settle();
    expect(ipcMocks.set).toHaveBeenCalledTimes(2);
    expect(ipcMocks.set).toHaveBeenLastCalledWith({ sections: rebased });
    expect(latest?.groups).toEqual(latestLocal);
    expect(hasMigratedChannelGroups(key)).toBe(false);

    React.act(() => {
      latest?.api.renameChannelGroup("local-section", "Local newest");
    });
    expect(latest?.groups).toEqual([
      { ...local[0]!, title: "Local newest" },
    ]);

    second.resolve({ layout: layout(rebased, 7) });
    await settle();
    expect(ipcMocks.set).toHaveBeenCalledTimes(3);
    expect(ipcMocks.set).toHaveBeenLastCalledWith({ sections: rebasedNewest });
    expect(hasMigratedChannelGroups(key)).toBe(false);

    third.resolve({ layout: layout(rebasedNewest, 8) });
    await settle();
    expect(latest?.groups).toEqual(rebasedNewest);
    expect(hasMigratedChannelGroups(key)).toBe(true);
    expect(hasDirtyChannelGroups(key)).toBe(false);
  });

  it("keeps the local cache as an old-server fallback", async () => {
    const key = "loom:channel-groups:v1:legacy-server";
    const local = [
      {
        id: "offline",
        title: "Offline",
        channelIds: ["channel-b", "channel-a"],
        collapsed: false,
      },
    ];
    saveChannelGroups(key, local);
    ipcMocks.get.mockRejectedValue(
      new Error("rpc failed: unknown method `channel.layout.get` (code -32601)"),
    );

    renderHarness(key);
    await settle();
    expect(latest?.groups).toEqual(local);

    React.act(() => latest?.api.renameChannelGroup("offline", "Still local"));
    await settle();
    expect(latest?.groups[0]?.title).toBe("Still local");
    expect(ipcMocks.set).not.toHaveBeenCalled();
    expect(hasDirtyChannelGroups(key)).toBe(true);
  });

  it("serializes writes and never lets an older response replace the latest edit", async () => {
    const key = "loom:channel-groups:v1:serialized";
    const initial = [
      {
        id: "ordered",
        title: "Initial",
        channelIds: ["channel-2", "channel-1"],
        collapsed: false,
      },
    ];
    localStorage.setItem(channelGroupMigrationStorageKey(key), "1");
    saveChannelGroups(key, initial);
    ipcMocks.get.mockResolvedValue({ layout: layout(initial, 1) });
    const first = deferred<ChannelLayoutResult>();
    const second = deferred<ChannelLayoutResult>();
    ipcMocks.set
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    renderHarness(key);
    await settle();

    React.act(() => latest?.api.renameChannelGroup("ordered", "First"));
    await settle();
    expect(ipcMocks.set).toHaveBeenCalledTimes(1);

    React.act(() => latest?.api.renameChannelGroup("ordered", "Latest"));
    expect(latest?.groups[0]?.title).toBe("Latest");
    expect(ipcMocks.set).toHaveBeenCalledTimes(1);

    first.resolve({
      layout: layout([{ ...initial[0]!, title: "First" }], 2),
    });
    await settle();
    expect(ipcMocks.set).toHaveBeenCalledTimes(2);
    expect(ipcMocks.set).toHaveBeenLastCalledWith({
      sections: [{ ...initial[0]!, title: "Latest" }],
    });
    expect(latest?.groups[0]?.title).toBe("Latest");

    second.resolve({
      layout: layout([{ ...initial[0]!, title: "Latest" }], 3),
    });
    await settle();
    expect(latest?.groups[0]?.title).toBe("Latest");
    expect(latest?.groups[0]?.channelIds).toEqual(["channel-2", "channel-1"]);
    expect(hasDirtyChannelGroups(key)).toBe(false);
  });

  it("applies channel.layout.updated from another desktop without echo-writing", async () => {
    const key = "loom:channel-groups:v1:remote";
    const initial = [
      {
        id: "initial",
        title: "Initial",
        channelIds: ["channel-1"],
        collapsed: false,
      },
    ];
    const remote = [
      {
        id: "remote-b",
        title: "Remote B",
        channelIds: ["channel-3", "channel-2"],
        collapsed: true,
      },
      {
        id: "remote-a",
        title: "Remote A",
        channelIds: ["channel-1"],
        collapsed: false,
      },
    ];
    localStorage.setItem(channelGroupMigrationStorageKey(key), "1");
    ipcMocks.get.mockResolvedValue({ layout: layout(initial, 8) });

    renderHarness(key);
    await settle();
    const update = channelLayoutFromStreamUpdate({
      kind: "channel.layout.updated",
      data: { layout: layout(remote, 9) },
    });
    expect(update).not.toBeNull();

    React.act(() => latest?.api.applyRemoteChannelLayout(update));
    expect(latest?.groups).toEqual(remote);
    expect(ipcMocks.set).not.toHaveBeenCalled();
    expect(loadChannelGroups(key)).toEqual(remote);
  });

  it("cleans a deleted channel from the local cache without a full layout write", async () => {
    const key = "loom:channel-groups:v1:deleted-channel-local-cleanup";
    const initial = [
      {
        id: "section-a",
        title: "Section A",
        channelIds: ["channel-1", "channel-2"],
        collapsed: false,
      },
    ];
    localStorage.setItem(channelGroupMigrationStorageKey(key), "1");
    ipcMocks.get.mockResolvedValue({ layout: layout(initial, 10) });

    renderHarness(key);
    await settle();

    React.act(() => latest?.api.removeChannelFromGroupsLocally("channel-1"));
    expect(latest?.groups[0]?.channelIds).toEqual(["channel-2"]);
    expect(loadChannelGroups(key)[0]?.channelIds).toEqual(["channel-2"]);
    expect(ipcMocks.set).not.toHaveBeenCalled();
    expect(hasDirtyChannelGroups(key)).toBe(false);
  });

  it("uses distinct migration and dirty marker keys", () => {
    const key = "loom:channel-groups:v1:markers";
    expect(channelGroupMigrationStorageKey(key)).not.toBe(
      channelGroupDirtyStorageKey(key),
    );
  });

  it("recognizes structured method-not-found errors from older servers", () => {
    expect(isUnsupportedChannelLayoutError({ code: -32601 })).toBe(true);
  });
});
