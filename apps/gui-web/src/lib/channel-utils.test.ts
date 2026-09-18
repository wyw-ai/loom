// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Channel } from "@/ipc/types";
import {
  channelGroupDirtyStorageKey,
  channelGroupMigrationStorageKey,
  hasDirtyChannelGroups,
  hasMigratedChannelGroups,
  markChannelGroupsDirty,
  markChannelGroupsMigrated,
  channelGroupSections,
  moveChannelInGroups,
  normalizeChannelLayout,
  reorderChannelGroups,
  sortChannels,
} from "@/lib/channel-utils";

describe("channel layout normalization", () => {
  it("preserves section and channel ordering exactly", () => {
    const layout = normalizeChannelLayout({
      actorId: "actor_human_test",
      sections: [
        {
          id: "section-z",
          title: "Zeta",
          channelIds: ["channel-3", "channel-1", "channel-2"],
          collapsed: true,
        },
        {
          id: "section-a",
          title: "Alpha",
          channelIds: ["channel-9", "channel-4"],
          collapsed: false,
        },
      ],
      revision: 42,
      updatedAt: "2026-08-10T00:00:00Z",
    });

    expect(layout?.sections.map((section) => section.id)).toEqual([
      "section-z",
      "section-a",
    ]);
    expect(layout?.sections[0]?.channelIds).toEqual([
      "channel-3",
      "channel-1",
      "channel-2",
    ]);
    expect(layout?.sections[1]?.channelIds).toEqual(["channel-9", "channel-4"]);
    expect(layout?.actorId).toBe("actor_human_test");
  });

  it("rejects layouts without a valid actor id", () => {
    expect(
      normalizeChannelLayout({
        sections: [],
        revision: 1,
        updatedAt: "2026-08-10T00:00:00Z",
      }),
    ).toBeNull();
  });
});

describe("channel layout drag placement", () => {
  const groups = [
    {
      id: "section-one",
      title: "One",
      channelIds: ["channel-a", "channel-b"],
      collapsed: false,
    },
    {
      id: "section-two",
      title: "Two",
      channelIds: ["channel-c"],
      collapsed: true,
    },
  ];

  it("reorders channels at the inferred insertion point and expands the target", () => {
    const moved = moveChannelInGroups(
      groups,
      "channel-a",
      "section-two",
      "channel-c",
    );

    expect(moved[0]?.channelIds).toEqual(["channel-b"]);
    expect(moved[1]?.channelIds).toEqual(["channel-a", "channel-c"]);
    expect(moved[1]?.collapsed).toBe(false);
  });

  it("persists the visible ungrouped order in a reserved section", () => {
    const moved = moveChannelInGroups(
      groups,
      "channel-a",
      "__ungrouped",
      "channel-d",
      ["channel-d", "channel-e"],
    );

    expect(moved.at(-1)).toEqual({
      id: "__ungrouped",
      title: "Ungrouped",
      channelIds: ["channel-a", "channel-d", "channel-e"],
      collapsed: false,
    });
  });

  it("reorders sections without exposing the reserved ungrouped section as a group", () => {
    const reordered = reorderChannelGroups(
      [
        ...groups,
        {
          id: "__ungrouped",
          title: "Ungrouped",
          channelIds: ["channel-d"],
          collapsed: false,
        },
      ],
      "section-two",
      "section-one",
    );

    expect(reordered.map((group) => group.id)).toEqual([
      "section-two",
      "section-one",
      "__ungrouped",
    ]);
  });

  it("renders newly discovered channels after an explicit ungrouped order", () => {
    const channels: Channel[] = ["channel-a", "channel-b", "channel-c"].map(
      (id) => ({ id, title: id, visibility: "public", members: [] }),
    );
    const sections = channelGroupSections(
      [
        {
          id: "__ungrouped",
          title: "Ungrouped",
          channelIds: ["channel-b", "channel-a"],
          collapsed: false,
        },
      ],
      channels,
    );

    expect(sections).toHaveLength(1);
    expect(sections[0]?.local).toBe(false);
    expect(sections[0]?.title).toBe("Channels");
    expect(sections[0]?.channels.map((channel) => channel.id)).toEqual([
      "channel-b",
      "channel-a",
      "channel-c",
    ]);
  });
});

describe("channel layout local sync markers", () => {
  const key = "loom:channel-groups:v1:test";

  beforeEach(() => {
    localStorage.removeItem(channelGroupMigrationStorageKey(key));
    localStorage.removeItem(channelGroupDirtyStorageKey(key));
  });

  it("tracks one-time migration and unsynced local edits independently", () => {
    expect(hasMigratedChannelGroups(key)).toBe(false);
    expect(hasDirtyChannelGroups(key)).toBe(false);

    markChannelGroupsMigrated(key);
    markChannelGroupsDirty(key, true);
    expect(hasMigratedChannelGroups(key)).toBe(true);
    expect(hasDirtyChannelGroups(key)).toBe(true);

    markChannelGroupsDirty(key, false);
    expect(hasMigratedChannelGroups(key)).toBe(true);
    expect(hasDirtyChannelGroups(key)).toBe(false);
  });
});

describe("deterministic channel sorting", () => {
  it("normalizes titles and uses the id as a locale-independent tie-break", () => {
    const channels: Channel[] = [
      {
        id: "channel-z",
        title: " ALPHA ",
        visibility: "public",
        members: [],
      },
      {
        id: "channel-a",
        title: "Ａｌｐｈａ",
        visibility: "public",
        members: [],
      },
      {
        id: "channel-beta",
        title: "beta",
        visibility: "public",
        members: [],
      },
      {
        id: "channel-cjk",
        title: "频道",
        visibility: "public",
        members: [],
      },
    ];
    const localeCompare = vi
      .spyOn(String.prototype, "localeCompare")
      .mockImplementation(() => {
        throw new Error("sortChannels must not depend on the host locale");
      });

    try {
      expect(sortChannels(channels).map((channel) => channel.id)).toEqual([
        "channel-a",
        "channel-z",
        "channel-beta",
        "channel-cjk",
      ]);
      expect(channels.map((channel) => channel.id)).toEqual([
        "channel-z",
        "channel-a",
        "channel-beta",
        "channel-cjk",
      ]);
    } finally {
      localeCompare.mockRestore();
    }
  });
});
