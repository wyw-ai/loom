import { useCallback, useEffect, useState } from "react";

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
