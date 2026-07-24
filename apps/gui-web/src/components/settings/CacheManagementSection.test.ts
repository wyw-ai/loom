// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "react-dom/client";
import React from "react";

// Enable React's act() environment so warnings about the testing
// environment are silenced and act() behaves correctly.
(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

import * as ipc from "@/ipc/bridge";
import { calcPercent, CacheManagementSection } from "@/components/settings/CacheManagementSection";

vi.mock("@/ipc/bridge");

const STORAGE_KEY = "loom:downloaded-artifacts";

describe("calcPercent", () => {
  it("returns 100 when size equals total", () => {
    expect(calcPercent(500, 500)).toBe(100);
  });

  it("returns 0 when size is zero", () => {
    expect(calcPercent(0, 500)).toBe(0);
  });

  it("returns 50 for half", () => {
    expect(calcPercent(250, 500)).toBe(50);
  });

  it("rounds to nearest integer", () => {
    // 1/3 ≈ 33.33% → rounds to 33
    expect(calcPercent(1, 3)).toBe(33);
    // 2/3 ≈ 66.67% → rounds to 67
    expect(calcPercent(2, 3)).toBe(67);
  });

  it("rounds 0.5 up (Math.round behavior)", () => {
    // 1/8 = 12.5% → Math.round rounds to 13
    expect(calcPercent(1, 8)).toBe(13);
  });

  it("returns null when size is null", () => {
    expect(calcPercent(null, 500)).toBeNull();
  });

  it("returns null when total is null", () => {
    expect(calcPercent(500, null)).toBeNull();
  });

  it("returns null when both are null", () => {
    expect(calcPercent(null, null)).toBeNull();
  });

  it("returns null when total is zero (division guard)", () => {
    expect(calcPercent(100, 0)).toBeNull();
  });

  it("returns null when total is negative (division guard)", () => {
    expect(calcPercent(100, -50)).toBeNull();
  });

  it("handles large numbers", () => {
    // 1GB out of 4GB = 25%
    expect(calcPercent(1_073_741_824, 4_294_967_296)).toBe(25);
  });

  it("can exceed 100 if size > total (edge case)", () => {
    expect(calcPercent(600, 500)).toBe(120);
  });
});

/**
 * ARCH TODO#2 Tier 2: CacheManagementSection.refresh() must call
 * reconcileDownloaded with the cachedIds returned by the breakdown, so
 * orphan localStorage entries (file externally deleted) are removed.
 */
describe("CacheManagementSection.refresh reconcileDownloaded (ARCH TODO#2 Tier 2)", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("calls reconcileDownloaded with cachedIds from breakdown", async () => {
    // Seed localStorage with an orphan entry that disk does not have.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ orphan: "/cache/orphan/blob", ondisk: "/cache/ondisk/blob" }),
    );

    const breakdown = {
      images: { size: 100, count: 1 },
      other: { size: 0, count: 0 },
      total: { size: 100, count: 1 },
      cachedIds: ["ondisk"], // disk only has "ondisk"; "orphan" is stale
    };
    vi.mocked(ipc.getAttachmentCacheBreakdown).mockResolvedValue(breakdown);

    const container = document.createElement("div");
    const root = createRoot(container);
    await React.act(async () => {
      root.render(React.createElement(CacheManagementSection));
    });
    // Allow the initial refresh effect to complete.
    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    // After refresh, reconcileDownloaded must have removed the orphan.
    const raw = localStorage.getItem(STORAGE_KEY);
    const map = raw ? (JSON.parse(raw) as Record<string, string>) : {};
    expect(map).toEqual({ ondisk: "/cache/ondisk/blob" });

    React.act(() => {
      root.unmount();
    });
  });
});

