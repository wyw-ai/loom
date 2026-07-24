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

  /**
   * Clear specific downloaded artifact mappings by artifact ID (localStorage).
   * ARCH D3 design: used by CacheManagementSection after per-category cache clear.
   */
  const clearDownloadedByIds = useCallback((artifactIds: string[]) => {
    if (artifactIds.length === 0) return;
    const current = loadMap();
    const next = { ...current };
    for (const id of artifactIds) {
      delete next[id];
    }
    saveMap(next);
    notifyAll();
  }, []);

  /**
   * Reconcile localStorage mappings against the on-disk cache.
   * Removes entries whose artifactId is NOT in cachedIds (orphan mappings
   * where localStorage records a path but the disk file was externally
   * deleted).
   *
   * ARCH TODO#2 Tier 2: called by CacheManagementSection.refresh() after
   * getting the breakdown (incl. cachedIds) from BE. This is the batch
   * counterpart to the lazy per-artifact cleanup in useAutoDownloadImage.
   */
  const reconcileDownloaded = useCallback((cachedIds: string[]) => {
    const current = loadMap();
    const onDiskSet = new Set(cachedIds);
    let changed = false;
    const next: DownloadMap = {};
    for (const [id, path] of Object.entries(current)) {
      if (onDiskSet.has(id)) {
        next[id] = path;
      } else {
        changed = true; // orphan detected - disk file externally deleted
      }
    }
    if (changed) {
      saveMap(next);
      notifyAll();
    }
  }, []);

  return { isDownloaded, setDownloaded, clearDownloaded, clearDownloadedByIds, reconcileDownloaded };
}
