import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as ipc from "@/ipc/bridge";
import {
  acquireObjectUrl,
  clearAllObjectUrls,
  readLocalFileAsBlob,
  releaseObjectUrl,
} from "@/hooks/useAutoDownloadImage";

vi.mock("@/ipc/bridge");

// Stub window for ensureBeforeUnloadHook (node environment has no window)
const fakeWindowListeners: Record<string, EventListener> = {};
vi.stubGlobal("window", {
  addEventListener: vi.fn((type: string, listener: EventListener) => {
    fakeWindowListeners[type] = listener;
  }),
  removeEventListener: vi.fn(),
  innerWidth: 1920,
  innerHeight: 1080,
});

// Stub useDownloadedArtifacts to avoid localStorage side effects.
// `clearDownloaded` is spied per-test for ARCH TODO#2 Tier 1 assertions.
const clearDownloadedMock = vi.fn();
vi.mock("@/hooks/useDownloadedArtifacts", () => ({
  useDownloadedArtifacts: () => ({
    isDownloaded: () => null,
    setDownloaded: () => {},
    clearDownloaded: clearDownloadedMock,
    clearDownloadedByIds: () => {},
  }),
}));

describe("ObjectURL reference counting", () => {
  beforeEach(() => {
    clearAllObjectUrls();
  });

  afterEach(() => {
    clearAllObjectUrls();
    vi.restoreAllMocks();
  });

  it("creates a new ObjectURL on first acquire", () => {
    const blob = new Blob([new Uint8Array([1, 2, 3])]);
    const url = acquireObjectUrl("art-a", blob);

    expect(url).toMatch(/^blob:/);
  });

  it("returns the same URL and increments refCount on second acquire", () => {
    const blob = new Blob([new Uint8Array([1])]);
    const url1 = acquireObjectUrl("art-a", blob);
    const url2 = acquireObjectUrl("art-a", blob);

    expect(url2).toBe(url1);
  });

  it("revokes the URL when refCount drops to zero", () => {
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");
    const blob = new Blob([new Uint8Array([1])]);

    acquireObjectUrl("art-b", blob);
    releaseObjectUrl("art-b");

    expect(revokeSpy).toHaveBeenCalledTimes(1);
  });

  it("does NOT revoke when other references remain", () => {
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");
    const blob = new Blob([new Uint8Array([1])]);

    acquireObjectUrl("art-c", blob);
    acquireObjectUrl("art-c", blob); // refCount=2
    releaseObjectUrl("art-c");       // refCount=1

    expect(revokeSpy).not.toHaveBeenCalled();
  });

  it("revokes after all references released (multi-acquire)", () => {
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");
    const blob = new Blob([new Uint8Array([1])]);

    acquireObjectUrl("art-d", blob);
    acquireObjectUrl("art-d", blob);
    acquireObjectUrl("art-d", blob); // refCount=3

    releaseObjectUrl("art-d"); // 2
    expect(revokeSpy).not.toHaveBeenCalled();

    releaseObjectUrl("art-d"); // 1
    expect(revokeSpy).not.toHaveBeenCalled();

    releaseObjectUrl("art-d"); // 0 → revoke
    expect(revokeSpy).toHaveBeenCalledTimes(1);
  });

  it("releaseObjectUrl is a no-op for unknown artifactId", () => {
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");

    releaseObjectUrl("nonexistent");

    expect(revokeSpy).not.toHaveBeenCalled();
  });

  it("clearAllObjectUrls revokes all cached URLs", () => {
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");
    const blob = new Blob([new Uint8Array([1])]);

    acquireObjectUrl("art-e", blob);
    acquireObjectUrl("art-f", blob);

    clearAllObjectUrls();

    expect(revokeSpy).toHaveBeenCalledTimes(2);
  });

  it("clearAllObjectUrls is safe to call when cache is empty", () => {
    expect(() => clearAllObjectUrls()).not.toThrow();
  });

  it("manages independent artifacts separately", () => {
    const blob = new Blob([new Uint8Array([1])]);
    const url1 = acquireObjectUrl("art-g", blob);
    const url2 = acquireObjectUrl("art-h", blob);

    expect(url1).not.toBe(url2);

    // Releasing one should not affect the other
    const revokeSpy = vi.spyOn(URL, "revokeObjectURL");
    releaseObjectUrl("art-g");

    expect(revokeSpy).toHaveBeenCalledTimes(1);

    // art-h should still be acquirable (refCount increments)
    const url2Again = acquireObjectUrl("art-h", blob);
    expect(url2Again).toBe(url2);
  });
});

describe("readLocalFileAsBlob", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads a single-chunk file (not truncated)", async () => {
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [10, 20, 30],
      truncated: false,
    });

    const blob = await readLocalFileAsBlob("/cache/file.dat", "application/pdf");
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual([10, 20, 30]);
    expect(blob.type).toBe("application/pdf");
  });

  it("paginates through multiple chunks", async () => {
    vi.mocked(ipc.readLocalFileBytes)
      .mockResolvedValueOnce({ bytes: [1, 2], truncated: true, nextOffset: 2 })
      .mockResolvedValueOnce({ bytes: [3, 4], truncated: true, nextOffset: 4 })
      .mockResolvedValueOnce({ bytes: [5], truncated: false });

    const blob = await readLocalFileAsBlob("/cache/file.dat", "image/png");
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual([1, 2, 3, 4, 5]);
    expect(ipc.readLocalFileBytes).toHaveBeenCalledTimes(3);
  });

  it("throws on zero-progress (nextOffset <= offset) — BLK-2 guard", async () => {
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [1, 2],
      truncated: true,
      nextOffset: 0, // same as initial offset=0 → no progress
    });

    await expect(readLocalFileAsBlob("/cache/file.dat", "image/png")).rejects.toThrow(
      "Local file read did not advance.",
    );
  });

  it("throws when nextOffset equals offset mid-stream", async () => {
    vi.mocked(ipc.readLocalFileBytes)
      .mockResolvedValueOnce({ bytes: [1, 2], truncated: true, nextOffset: 2 })
      .mockResolvedValueOnce({ bytes: [3, 4], truncated: true, nextOffset: 2 }); // stuck at 2

    await expect(readLocalFileAsBlob("/cache/file.dat", "image/png")).rejects.toThrow(
      "Local file read did not advance.",
    );
  });

  it("defaults to octet-stream when mediaType is empty", async () => {
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [1],
      truncated: false,
    });

    const blob = await readLocalFileAsBlob("/cache/file.dat", "");

    expect(blob.type).toBe("application/octet-stream");
  });

  it("uses nextOffset fallback (offset + bytes.length) when nextOffset is absent", async () => {
    // No nextOffset field — should compute from bytes.length
    vi.mocked(ipc.readLocalFileBytes)
      .mockResolvedValueOnce({ bytes: [1, 2], truncated: true }) // no nextOffset
      .mockResolvedValueOnce({ bytes: [3], truncated: false });

    const blob = await readLocalFileAsBlob("/cache/file.dat", "image/png");
    const buf = new Uint8Array(await blob.arrayBuffer());

    expect(Array.from(buf)).toEqual([1, 2, 3]);
  });
});
