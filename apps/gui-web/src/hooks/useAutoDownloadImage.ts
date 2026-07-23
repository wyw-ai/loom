import { useEffect, useRef, useState } from "react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { errorText } from "@/lib/format-utils";
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
 * Clear all ObjectURLs from the memory cache.
 * ARCH D3-r1: called by ClearCacheSection after clearing disk cache,
 * so components re-render and re-download visible images.
 */
export function clearAllObjectUrls(): void {
  for (const [, entry] of objectUrlCache) {
    URL.revokeObjectURL(entry.objectUrl);
  }
  objectUrlCache.clear();
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
 * for inline display. Uses three-level caching (ARCH D3-r1):
 * 1. ObjectURL memory cache — shared blob URLs with reference counting
 * 2. localStorage (via useDownloadedArtifacts) — tracks local cache path
 * 3. Disk cache (data_dir/loom/cache/attachments/) — persistent across restarts
 *
 * Cache-hit path: if localStorage has a path and the file exists on disk,
 * read from local file (readLocalFileBytes) instead of network download.
 *
 * ARCH D3-r1: uses downloadToCache (not downloadToTemp) for persistence.
 */
export function useAutoDownloadImage(artifact: Artifact | null): AutoDownloadImageState {
  const { isDownloaded, setDownloaded } = useDownloadedArtifacts();
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heldArtifactIdRef = useRef<string | null>(null);

  const localPath = artifact ? isDownloaded(artifact.id) : null;

  useEffect(() => {
    if (!artifact) {
      setObjectUrl(null);
      setLoading(false);
      setError(null);
      return;
    }

    const artifactId = artifact.id;
    const artifactSnapshot = artifact;
    const cachedLocalPath = isDownloaded(artifactId);
    let cancelled = false;

    // Release previously held ObjectURL reference
    if (heldArtifactIdRef.current && heldArtifactIdRef.current !== artifactId) {
      releaseObjectUrl(heldArtifactIdRef.current);
      heldArtifactIdRef.current = null;
    }

    // Check memory cache first
    const memCached = objectUrlCache.get(artifactId);
    if (memCached) {
      memCached.refCount += 1;
      heldArtifactIdRef.current = artifactId;
      setObjectUrl(memCached.objectUrl);
      setLoading(false);
      setError(null);
      return;
    }

    // Need to load
    setLoading(true);
    setError(null);

    async function load() {
      try {
        let promise = inFlightDownloads.get(artifactId);
        if (!promise) {
          promise = (async () => {
            // Cache-hit path: try reading from local disk first (ARCH D3-r1)
            if (cachedLocalPath) {
              try {
                const exists = await ipc.pathExists(cachedLocalPath);
                if (exists) {
                  const blob = await readLocalFileAsBlob(cachedLocalPath, artifactSnapshot.mediaType);
                  const url = acquireObjectUrl(artifactId, blob);
                  return { blob, objectUrl: url };
                }
              } catch {
                // Local file read failed — fall through to network download
              }
            }

            // Network download via downloadToCache (ARCH D3-r1: persistent cache)
            const cachePath = await ipc.downloadToCache({
              artifactId,
              suggestedName: artifactSnapshot.name || `${artifactId}.bin`,
            });
            setDownloaded(artifactId, cachePath);

            // Read the cached file to create ObjectURL
            const blob = await readLocalFileAsBlob(cachePath, artifactSnapshot.mediaType);
            const url = acquireObjectUrl(artifactId, blob);
            return { blob, objectUrl: url };
          })();
          inFlightDownloads.set(artifactId, promise);
        }

        const result = await promise!;
        inFlightDownloads.delete(artifactId);

        if (cancelled) {
          releaseObjectUrl(artifactId);
          return;
        }

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

    void load();

    return () => {
      cancelled = true;
      if (heldArtifactIdRef.current) {
        releaseObjectUrl(heldArtifactIdRef.current);
        heldArtifactIdRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [artifact]);

  return { objectUrl, loading, error, localPath };
}

/**
 * Download an artifact to the persistent cache directory (for Open/Reveal operations).
 * ARCH D3-r1: uses downloadToCache (not downloadToTemp) for images.
 * Returns the local path. Uses the shared useDownloadedArtifacts cache.
 */
export function useDownloadToLocal() {
  const { setDownloaded } = useDownloadedArtifacts();

  return async function downloadToLocal(artifact: Artifact): Promise<string> {
    const cachePath = await ipc.downloadToCache({
      artifactId: artifact.id,
      suggestedName: artifact.name || `${artifact.id}.bin`,
    });
    setDownloaded(artifact.id, cachePath);
    return cachePath;
  };
}

/**
 * Read a local file as a Blob by paginating through readLocalFileBytes.
 * ARCH D3-r1: cache-hit path — reads from disk instead of network.
 */
async function readLocalFileAsBlob(path: string, mediaType: string): Promise<Blob> {
  const chunkSize = 1024 * 1024;
  const chunks: Uint8Array[] = [];
  let offset = 0;
  for (let i = 0; i < 512; i++) {
    const result = await ipc.readLocalFileBytes({ path, offset, maxBytes: chunkSize });
    const bytes = new Uint8Array(result.bytes);
    chunks.push(bytes);
    if (!result.truncated) {
      const parts = chunks.map((chunk) => {
        const copy = new Uint8Array(chunk.byteLength);
        copy.set(chunk);
        return copy.buffer;
      });
      return new Blob(parts, { type: mediaType || "application/octet-stream" });
    }
    offset = result.nextOffset ?? offset + bytes.byteLength;
    if (offset <= (result.nextOffset ?? 0) - bytes.byteLength) break;
  }
  throw new Error("Local file is too large to read in one operation.");
}
