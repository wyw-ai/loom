import { useEffect, useRef, useState } from "react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { errorText } from "@/lib/format-utils";
import { readArtifactBlob } from "@/lib/artifact-blob";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";

/**
 * ObjectURL memory cache with reference counting.
 *
 * Multiple InlineImage components showing the same artifact share a single
 * ObjectURL. The URL is revoked only when the last consumer releases it.
 * A beforeunload handler revokes all URLs as a safety net.
 */

interface CacheEntry {
  objectUrl: string;
  refCount: number;
}

const objectUrlCache = new Map<string, CacheEntry>();
const beforeUnloadRegistered = { current: false };

function ensureBeforeUnloadHook(): void {
  if (beforeUnloadRegistered.current) return;
  beforeUnloadRegistered.current = true;
  window.addEventListener("beforeunload", () => {
    for (const [, entry] of objectUrlCache) {
      URL.revokeObjectURL(entry.objectUrl);
    }
    objectUrlCache.clear();
  });
}

function acquireObjectUrl(artifactId: string, blob: Blob): string {
  ensureBeforeUnloadHook();
  const existing = objectUrlCache.get(artifactId);
  if (existing) {
    existing.refCount += 1;
    return existing.objectUrl;
  }
  const objectUrl = URL.createObjectURL(blob);
  objectUrlCache.set(artifactId, { objectUrl, refCount: 1 });
  return objectUrl;
}

function releaseObjectUrl(artifactId: string): void {
  const entry = objectUrlCache.get(artifactId);
  if (!entry) return;
  entry.refCount -= 1;
  if (entry.refCount <= 0) {
    URL.revokeObjectURL(entry.objectUrl);
    objectUrlCache.delete(artifactId);
  }
}

/**
 * In-flight download deduplication.
 * If two components request the same artifact simultaneously, only one
 * network download runs; both receive the result.
 */
const inFlightDownloads = new Map<
  string,
  Promise<{ blob: Blob; objectUrl: string }>
>();

export interface AutoDownloadImageState {
  objectUrl: string | null;
  loading: boolean;
  error: string | null;
  localPath: string | null;
}

/**
 * Auto-downloads an image artifact on mount and provides an ObjectURL
 * for inline display. Uses two-level caching:
 * 1. localStorage (via useDownloadedArtifacts) — tracks local temp path
 * 2. ObjectURL memory cache — shared blob URLs with reference counting
 *
 * ARCH D3 design: inFlightDownloads Map deduplicates concurrent requests.
 */
export function useAutoDownloadImage(artifact: Artifact | null): AutoDownloadImageState {
  const { isDownloaded } = useDownloadedArtifacts();
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heldArtifactIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!artifact) {
      setObjectUrl(null);
      setLoading(false);
      setError(null);
      return;
    }

    const artifactId = artifact.id;
    const artifactSnapshot = artifact;
    let cancelled = false;

    // Release previously held ObjectURL reference
    if (heldArtifactIdRef.current && heldArtifactIdRef.current !== artifactId) {
      releaseObjectUrl(heldArtifactIdRef.current);
      heldArtifactIdRef.current = null;
    }

    // Check memory cache first
    const cached = objectUrlCache.get(artifactId);
    if (cached) {
      cached.refCount += 1;
      heldArtifactIdRef.current = artifactId;
      setObjectUrl(cached.objectUrl);
      setLoading(false);
      setError(null);
      return;
    }

    // Need to download
    setLoading(true);
    setError(null);

    async function download() {
      try {
        let promise = inFlightDownloads.get(artifactId);
        if (!promise) {
          promise = (async () => {
            const blob = await readArtifactBlob(artifactSnapshot);
            const url = acquireObjectUrl(artifactId, blob);
            return { blob, objectUrl: url };
          })();
          inFlightDownloads.set(artifactId, promise);
        }

        const result = await promise;
        inFlightDownloads.delete(artifactId);

        if (cancelled) {
          // Component unmounted or artifact changed before download completed.
          // The acquireObjectUrl inside the promise already incremented refCount,
          // so release our share.
          releaseObjectUrl(artifactId);
          return;
        }

        // Persist to localStorage cache (local temp path tracking).
        // We don't have a real temp path here (ObjectURL is in-memory),
        // but we mark it as "downloaded" so context-menu actions that
        // need a local file can trigger a real downloadToTemp.
        // Actually — per ARCH design, context menu items that need a local
        // file path should call downloadToTemp separately. The ObjectURL
        // is for display only. So we do NOT setDownloaded here.

        heldArtifactIdRef.current = artifactId;
        setObjectUrl(result.objectUrl);
        setLoading(false);
      } catch (err) {
        if (!cancelled) {
          setError(errorText(err));
          setLoading(false);
        }
        inFlightDownloads.delete(artifactId);
      }
    }

    void download();

    return () => {
      cancelled = true;
      if (heldArtifactIdRef.current) {
        releaseObjectUrl(heldArtifactIdRef.current);
        heldArtifactIdRef.current = null;
      }
    };
  }, [artifact]);

  const localPath = artifact ? isDownloaded(artifact.id) : null;

  return { objectUrl, loading, error, localPath };
}

/**
 * Download an artifact to a local temp file (for Open/Reveal operations).
 * Returns the local path. Uses the shared useDownloadedArtifacts cache.
 */
export function useDownloadToLocal() {
  const { setDownloaded } = useDownloadedArtifacts();

  return async function downloadToLocal(artifact: Artifact): Promise<string> {
    const tempPath = await ipc.downloadToTemp({
      artifactId: artifact.id,
      suggestedName: artifact.name || `${artifact.id}.bin`,
    });
    setDownloaded(artifact.id, tempPath);
    return tempPath;
  };
}
