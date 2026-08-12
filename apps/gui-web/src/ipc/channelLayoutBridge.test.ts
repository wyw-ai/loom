import { beforeEach, describe, expect, it, vi } from "vitest";

const webMocks = vi.hoisted(() => ({
  callRpc: vi.fn(),
}));

vi.mock("@/ipc/webBridge", () => ({
  callRpc: webMocks.callRpc,
}));

import { channelLayoutGet, channelLayoutSet } from "@/ipc/bridge";

describe("channel layout bridge", () => {
  beforeEach(() => {
    webMocks.callRpc.mockReset();
  });

  it("forwards get and set with the agreed RPC methods and wire shapes", async () => {
    webMocks.callRpc.mockResolvedValue({
      layout: {
        actorId: "actor_human_test",
        sections: [],
        revision: 0,
        updatedAt: "",
      },
    });

    await channelLayoutGet();
    expect(webMocks.callRpc).toHaveBeenNthCalledWith(1, "channel.layout.get", {});

    const sections = [
      {
        id: "section-b",
        title: "B",
        channelIds: ["channel-2", "channel-1"],
        collapsed: false,
      },
    ];
    await channelLayoutSet({ sections, merge: true });
    expect(webMocks.callRpc).toHaveBeenNthCalledWith(2, "channel.layout.set", {
      sections,
      merge: true,
    });
  });
});
