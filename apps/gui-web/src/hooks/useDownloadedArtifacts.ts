import { useCallback, useEffect, useState } from "react";

/**
 * Ephemeral cache of downloaded artifact → local temp file path.
 *
 * NOTE: This localStorage map is an ephemeral hint, not a source of truth.
 * The OS may clean up temp files at any time (reboot, disk cleanup, manual
 * purge), which can leave stale artifactId → path mappings that point to
 * deleted files. Consumers must treat a positive lookup as "possibly
 * downloaded" and gracefully fall back to re-download when the file is
 * missing. No runtime validation is performed here because stat-ing every
 * cached path on each render would be prohibitively expensive (ARCH
 * decision: accept stale-mapping risk over perf cost).
 */
const STORAGE_KEY = "loom:downloaded-artifacts";

type DownloadMap = Record<string, string>;

function loadMap(): DownloadMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as DownloadMap) : {};
  } catch {
    return {};
  }
}

function saveMap(map: DownloadMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // localStorage full or disabled — silent degradation
  }
}

const listeners = new Set<() => void>();

function notifyAll(): void {
  listeners.forEach((fn) => fn());
}

export function useDownloadedArtifacts() {
  const [map, setMap] = useState<DownloadMap>(loadMap);

  useEffect(() => {
    const listener = () => setMap(loadMap());
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);

  const isDownloaded = useCallback(
    (artifactId: string): string | null => map[artifactId] ?? null,
    [map],
  );

  const setDownloaded = useCallback((artifactId: string, localPath: string) => {
    const next = { ...loadMap(), [artifactId]: localPath };
    saveMap(next);
    notifyAll();
  }, []);

  const clearDownloaded = useCallback((artifactId: string) => {
    const current = loadMap();
    if (!(artifactId in current)) return;
    const next = { ...current };
    delete next[artifactId];
    saveMap(next);
    notifyAll();
  }, []);

  return { isDownloaded, setDownloaded, clearDownloaded };
}
