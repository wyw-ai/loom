// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createRoot } from "react-dom/client";
import React from "react";

// Enable React's act() environment so warnings about the testing
// environment are silenced and act() behaves correctly.
(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";

/**
 * Tests for useDownloadedArtifacts, focused on ARCH TODO#2 Tier 2
 * `reconcileDownloaded`. Uses jsdom so real localStorage + React effects
 * run. The hook is captured via a minimal mounted probe component.
 */

const STORAGE_KEY = "loom:downloaded-artifacts";

function setMap(map: Record<string, string>): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
}

function readMap(): Record<string, string> {
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw ? (JSON.parse(raw) as Record<string, string>) : {};
}

function captureHookReturn(): ReturnType<typeof useDownloadedArtifacts> {
  let captured: ReturnType<typeof useDownloadedArtifacts> | null = null;
  function Probe() {
    captured = useDownloadedArtifacts();
    return null;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  React.act(() => {
    root.render(React.createElement(Probe));
  });
  if (!captured) throw new Error("hook return was not captured");
  // Unmount to clean up the listener registered in useEffect.
  React.act(() => {
    root.unmount();
  });
  return captured;
}

describe("useDownloadedArtifacts.reconcileDownloaded (ARCH TODO#2 Tier 2)", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("removes orphan entries not present on disk", () => {
    setMap({ a: "/cache/a", b: "/cache/b", c: "/cache/c" });

    const { reconcileDownloaded } = captureHookReturn();
    // Disk only has a and c; b is an orphan mapping.
    React.act(() => {
      reconcileDownloaded(["a", "c"]);
    });

    expect(readMap()).toEqual({ a: "/cache/a", c: "/cache/c" });
  });

  it("keeps all entries when all are present on disk", () => {
    setMap({ a: "/cache/a", b: "/cache/b" });

    const { reconcileDownloaded } = captureHookReturn();
    React.act(() => {
      reconcileDownloaded(["a", "b"]);
    });

    expect(readMap()).toEqual({ a: "/cache/a", b: "/cache/b" });
  });

  it("clears all entries when disk is empty", () => {
    setMap({ a: "/cache/a", b: "/cache/b" });

    const { reconcileDownloaded } = captureHookReturn();
    React.act(() => {
      reconcileDownloaded([]);
    });

    expect(readMap()).toEqual({});
  });

  it("is a no-op on empty localStorage", () => {
    const { reconcileDownloaded } = captureHookReturn();
    // localStorage has no entries; should not throw.
    expect(() =>
      React.act(() => {
        reconcileDownloaded(["a"]);
      }),
    ).not.toThrow();
    expect(readMap()).toEqual({});
  });

  it("does not notify (write) when there is no change", () => {
    setMap({ a: "/cache/a" });
    const before = localStorage.getItem(STORAGE_KEY);

    const { reconcileDownloaded } = captureHookReturn();
    React.act(() => {
      reconcileDownloaded(["a"]);
    });
    const after = localStorage.getItem(STORAGE_KEY);

    // saveMap is only called when an orphan is detected; with all entries
    // on disk the serialized value must be byte-identical.
    expect(after).toBe(before);
  });
});
