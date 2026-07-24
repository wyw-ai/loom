// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "react-dom/client";
import React from "react";

// Enable React's act() environment so warnings about the testing
// environment are silenced and act() behaves correctly.
(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

import * as ipc from "@/ipc/bridge";
import { useAutoDownloadImage } from "@/hooks/useAutoDownloadImage";
import type { Artifact } from "@/ipc/types";

/**
 * ARCH TODO#2 Tier 1 integration tests: useAutoDownloadImage must clear
 * the orphan localStorage mapping when pathExists returns false (file was
 * externally deleted), but must NOT clear it when pathExists throws
 * (transient FS error - conservative).
 *
 * Issue 1 fix (PR#53 review): the previous version of this test asserted
 * on the resulting localStorage map state, which was tautological -
 * `setDownloaded` from the re-download reset the same id regardless of
 * whether `clearDownloaded` ran. The fix is to mock `useDownloadedArtifacts`
 * with a spy on `clearDownloaded` and assert the spy was called (or not)
 * directly. This makes the test fail if the Tier 1 cleanup is removed.
 *
 * Uses jsdom so React effects run.
 */

vi.mock("@/ipc/bridge");

// Mock useDownloadedArtifacts so clearDownloaded is a spy we can assert on.
// `isDownloaded` returns a cached path so the cache-hit branch is taken.
// `setDownloaded` is a no-op (we don't need localStorage writes here).
const clearDownloadedMock = vi.fn();
const setDownloadedMock = vi.fn();
vi.mock("@/hooks/useDownloadedArtifacts", () => ({
  useDownloadedArtifacts: () => ({
    isDownloaded: () => "/cache/art-orphan/blob.png",
    setDownloaded: setDownloadedMock,
    clearDownloaded: clearDownloadedMock,
    clearDownloadedByIds: () => {},
    reconcileDownloaded: () => {},
  }),
}));

function makeArtifact(id: string): Artifact {
  return {
    id,
    uri: `loom://${id}`,
    kind: "file",
    name: `${id}.png`,
    mediaType: "image/png",
    size: 10,
    checksum: "sha256:0",
    createdBy: "test",
    createdAt: "2026-07-25T00:00:00Z",
  };
}

/**
 * Render useAutoDownloadImage and flush effects. Returns a cleanup function.
 */
function renderHook(artifact: Artifact | null) {
  function Probe() {
    useAutoDownloadImage(artifact);
    return null;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  React.act(() => {
    root.render(React.createElement(Probe));
  });
  return {
    unmount() {
      React.act(() => {
        root.unmount();
      });
    },
  };
}

describe("useAutoDownloadImage Tier 1 orphan cleanup (ARCH TODO#2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("calls clearDownloaded when pathExists returns false (orphan cleanup)", async () => {
    // pathExists -> false (file deleted externally). This is the definitive
    // "file missing" signal that must trigger Tier 1 cleanup.
    vi.mocked(ipc.pathExists).mockResolvedValue(false);
    // After clearing the orphan, the hook falls through to a network
    // download. Mock downloadToCache + readLocalFileBytes so the effect
    // completes without error.
    vi.mocked(ipc.downloadToCache).mockResolvedValue("/cache/art-orphan/blob.png");
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [1, 2, 3],
      truncated: false,
    });

    const result = renderHook(makeArtifact("art-orphan"));

    // Wait for the async load() to settle.
    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    // Tier 1 assertion: clearDownloaded MUST have been called with the
    // orphan artifactId. This is the direct spy assertion that fails if
    // the Tier 1 cleanup is removed from useAutoDownloadImage.
    expect(clearDownloadedMock).toHaveBeenCalledWith("art-orphan");
    // pathExists was called to verify the cached path.
    expect(ipc.pathExists).toHaveBeenCalledWith("/cache/art-orphan/blob.png");
    // Re-download happened (setDownloaded called after fall-through).
    expect(setDownloadedMock).toHaveBeenCalledWith("art-orphan", "/cache/art-orphan/blob.png");

    result.unmount();
  });

  it("does NOT call clearDownloaded when pathExists throws (transient FS error)", async () => {
    // pathExists throws a transient FS error (permission/lock), NOT a
    // definitive "file missing" signal. Conservative: do not clear.
    vi.mocked(ipc.pathExists).mockRejectedValue(new Error("EACCES"));
    // Fall-through download still succeeds.
    vi.mocked(ipc.downloadToCache).mockResolvedValue("/cache/art-transient/blob.png");
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [1, 2, 3],
      truncated: false,
    });

    const result = renderHook(makeArtifact("art-transient"));

    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    // Conservative assertion: clearDownloaded MUST NOT have been called
    // because pathExists threw (transient error), not returned false.
    expect(clearDownloadedMock).not.toHaveBeenCalled();
    // pathExists was called (and threw), then fall-through download ran.
    expect(ipc.pathExists).toHaveBeenCalled();
    expect(setDownloadedMock).toHaveBeenCalledWith("art-transient", "/cache/art-transient/blob.png");

    result.unmount();
  });

  it("does NOT call clearDownloaded when pathExists returns true (file exists)", async () => {
    // pathExists -> true (file is on disk). No orphan, no cleanup needed.
    // Clear the module-level ObjectURL cache so this test is isolated.
    vi.mocked(ipc.pathExists).mockResolvedValue(true);
    vi.mocked(ipc.readLocalFileBytes).mockResolvedValue({
      bytes: [1, 2, 3],
      truncated: false,
    });

    const result = renderHook(makeArtifact("art-present"));

    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    // File exists -> no orphan cleanup needed.
    expect(clearDownloadedMock).not.toHaveBeenCalled();
    expect(ipc.pathExists).toHaveBeenCalledWith("/cache/art-orphan/blob.png");

    result.unmount();
  });
});
