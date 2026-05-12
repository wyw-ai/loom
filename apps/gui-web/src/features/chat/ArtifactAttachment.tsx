import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import clsx from "clsx";
import {
  Download,
  Eye,
  File,
  FileArchive,
  FileText,
  Image,
  Loader2,
  Music,
  Video,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { useUI } from "@/store/ui";

const TEXT_CHUNK_BYTES = 64 * 1024;
const DOWNLOAD_CHUNK_BYTES = 1024 * 1024;
const PREVIEW_MAX_BYTES = 32 * 1024 * 1024;

export function ArtifactAttachments({
  artifactIds,
}: {
  artifactIds?: string[];
}) {
  const uniqueIds = useMemo(
    () => [...new Set((artifactIds ?? []).filter(Boolean))],
    [artifactIds],
  );
  const [preview, setPreview] = useState<Artifact | null>(null);

  if (uniqueIds.length === 0) return null;

  return (
    <>
      <div className="mt-2 grid max-w-2xl gap-2">
        {uniqueIds.map((artifactId) => (
          <ArtifactAttachmentCard
            key={artifactId}
            artifactId={artifactId}
            onPreview={setPreview}
          />
        ))}
      </div>
      {preview && (
        <AttachmentPreviewModal
          artifact={preview}
          onClose={() => setPreview(null)}
        />
      )}
    </>
  );
}

function ArtifactAttachmentCard({
  artifactId,
  onPreview,
}: {
  artifactId: string;
  onPreview: (artifact: Artifact) => void;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [artifact, setArtifact] = useState<Artifact | null>(null);
  const [error, setError] = useState("");
  const [downloading, setDownloading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setArtifact(null);
    setError("");
    void ipc
      .artifactGet({ artifactId })
      .then((res) => {
        if (!cancelled) setArtifact(res.artifact);
      })
      .catch((err) => {
        if (!cancelled) setError(formatError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [artifactId]);

  if (error) {
    return (
      <div className="border-2 border-black bg-white px-3 py-2 text-xs font-bold text-danger shadow-brutal-sm">
        attachment unavailable: {error}
      </div>
    );
  }

  if (!artifact) {
    return (
      <div className="flex items-center gap-2 border-2 border-black bg-white px-3 py-2 text-xs font-bold text-black/55 shadow-brutal-sm">
        <Loader2 size={14} className="animate-spin" />
        Loading attachment
      </div>
    );
  }

  const kind = artifactKind(artifact);
  const previewable = isPreviewable(artifact);
  const Icon = iconForKind(kind);

  const download = async () => {
    setDownloading(true);
    try {
      const bytes = await readArtifactBytes(artifact.id);
      saveBlob(bytes, artifact.mediaType, artifact.name);
    } catch (err) {
      pushToast("error", `download failed: ${formatError(err)}`);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="min-w-0 border-2 border-black bg-white px-3 py-2 shadow-brutal-sm">
      <div className="flex min-w-0 flex-wrap items-center gap-3">
        <div
          className={clsx(
            "flex h-10 w-10 shrink-0 items-center justify-center border-2 border-black",
            kind === "image"
              ? "bg-brutal-cyan"
              : kind === "text"
                ? "bg-brutal-yellow"
                : "bg-brutal-cream",
          )}
        >
          <Icon size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-black text-black">
            {artifact.name}
          </div>
          <div className="mt-0.5 font-mono text-[11px] text-black/50">
            {labelForKind(kind)} · {artifact.mediaType || "unknown"} ·{" "}
            {formatBytes(artifact.size)}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {previewable && (
            <button
              type="button"
              title="Preview"
              onClick={() => onPreview(artifact)}
              className="btn-brutal-sm bg-white p-1"
            >
              <Eye size={14} />
            </button>
          )}
          <button
            type="button"
            title="Download"
            disabled={downloading}
            onClick={() => void download()}
            className="btn-brutal-sm bg-brutal-lime p-1"
          >
            {downloading ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Download size={14} />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

function AttachmentPreviewModal({
  artifact,
  onClose,
}: {
  artifact: Artifact;
  onClose: () => void;
}) {
  const kind = artifactKind(artifact);
  const [text, setText] = useState("");
  const [nextOffset, setNextOffset] = useState<number | null>(0);
  const [blobUrl, setBlobUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [onClose]);

  useEffect(() => {
    let cancelled = false;
    setText("");
    setNextOffset(kind === "text" ? 0 : null);
    setBlobUrl("");
    setLoading(true);
    setError("");

    if (kind === "text") {
      void readTextChunk(artifact.id, 0).then(
        (chunk) => {
          if (cancelled) return;
          setText(chunk.content);
          setNextOffset(chunk.nextOffset ?? null);
          setLoading(false);
        },
        (err) => {
          if (cancelled) return;
          setError(formatError(err));
          setLoading(false);
        },
      );
      return () => {
        cancelled = true;
      };
    }

    if (!isPreviewable(artifact)) {
      setLoading(false);
      return () => {
        cancelled = true;
      };
    }

    void readArtifactBytes(artifact.id, PREVIEW_MAX_BYTES).then(
      (bytes) => {
        if (cancelled) return;
        const url = URL.createObjectURL(
          new Blob([bytesToArrayBuffer(bytes)], { type: artifact.mediaType }),
        );
        setBlobUrl(url);
        setLoading(false);
      },
      (err) => {
        if (cancelled) return;
        setError(formatError(err));
        setLoading(false);
      },
    );

    return () => {
      cancelled = true;
    };
  }, [artifact, kind]);

  useEffect(() => {
    return () => {
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    };
  }, [blobUrl]);

  const loadMore = async () => {
    if (nextOffset === null) return;
    setLoading(true);
    try {
      const chunk = await readTextChunk(artifact.id, nextOffset);
      setText((current) => current + chunk.content);
      setNextOffset(chunk.nextOffset ?? null);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setLoading(false);
    }
  };

  if (typeof document === "undefined") return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[120] flex items-center justify-center bg-black/60 p-6"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div
        className="flex max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden border-2 border-black bg-brutal-cream shadow-brutal"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-4 border-b-2 border-black bg-white px-5 py-4">
          <div className="min-w-0">
            <div className="font-mono text-[11px] font-black uppercase text-black/45">
              {labelForKind(kind)} preview
            </div>
            <h2 className="mt-1 truncate text-xl font-black text-black">
              {artifact.name}
            </h2>
            <div className="mt-1 font-mono text-xs text-black/55">
              {artifact.mediaType || "unknown"} · {formatBytes(artifact.size)}
            </div>
          </div>
          <button
            type="button"
            title="Close"
            onClick={onClose}
            className="btn-brutal-sm bg-white p-1"
          >
            <X size={15} />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-auto p-5">
          {loading && !text && !blobUrl && (
            <div className="flex items-center gap-2 border-2 border-black bg-white px-4 py-3 text-sm font-bold text-black/55 shadow-brutal-sm">
              <Loader2 size={15} className="animate-spin" />
              Loading preview
            </div>
          )}

          {error && (
            <div className="border-2 border-black bg-white px-4 py-3 text-sm font-bold text-danger shadow-brutal-sm">
              {error}
            </div>
          )}

          {!loading && !error && kind === "image" && blobUrl && (
            <div className="flex min-h-[50vh] items-center justify-center">
              <img
                src={blobUrl}
                alt={artifact.name}
                className="max-h-[68vh] max-w-full border-2 border-black bg-white object-contain"
              />
            </div>
          )}

          {!loading && !error && kind === "pdf" && blobUrl && (
            <iframe
              src={blobUrl}
              title={artifact.name}
              className="h-[68vh] w-full border-2 border-black bg-white"
            />
          )}

          {!loading && !error && kind === "audio" && blobUrl && (
            <div className="border-2 border-black bg-white px-5 py-5 shadow-brutal-sm">
              <audio controls src={blobUrl} className="w-full" />
            </div>
          )}

          {!loading && !error && kind === "video" && blobUrl && (
            <video
              controls
              src={blobUrl}
              className="max-h-[68vh] w-full border-2 border-black bg-black"
            />
          )}

          {!error && kind === "text" && (
            <>
              <pre className="whitespace-pre-wrap break-words border-2 border-black bg-white px-4 py-3 font-mono text-xs leading-relaxed text-black shadow-brutal-sm">
                {text}
              </pre>
              {nextOffset !== null && (
                <div className="mt-3">
                  <button
                    type="button"
                    disabled={loading}
                    onClick={() => void loadMore()}
                    className="btn-brutal-sm gap-1 bg-brutal-yellow px-3 text-xs"
                  >
                    {loading && <Loader2 size={13} className="animate-spin" />}
                    Load more
                  </button>
                </div>
              )}
            </>
          )}

          {!loading && !error && !isPreviewable(artifact) && (
            <div className="border-2 border-black bg-white px-4 py-3 text-sm font-bold text-black/55 shadow-brutal-sm">
              No inline preview is available for this file type.
            </div>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

async function readTextChunk(artifactId: string, offset: number) {
  return ipc.artifactRead({
    artifactId,
    offset,
    maxBytes: TEXT_CHUNK_BYTES,
  });
}

async function readArtifactBytes(
  artifactId: string,
  maxBytes = Number.POSITIVE_INFINITY,
): Promise<Uint8Array> {
  const chunks: Uint8Array[] = [];
  let total = 0;
  let offset = 0;

  for (;;) {
    const res = await ipc.artifactRead({
      artifactId,
      offset,
      maxBytes: Math.min(DOWNLOAD_CHUNK_BYTES, maxBytes - total),
    });
    const bytes = new Uint8Array(res.bytes ?? []);
    chunks.push(bytes);
    total += bytes.byteLength;
    if (total > maxBytes || (res.truncated && total >= maxBytes)) {
      throw new Error(`preview exceeds ${formatBytes(maxBytes)}`);
    }
    if (!res.truncated || typeof res.nextOffset !== "number") break;
    offset = res.nextOffset;
  }

  const out = new Uint8Array(total);
  let cursor = 0;
  for (const chunk of chunks) {
    out.set(chunk, cursor);
    cursor += chunk.byteLength;
  }
  return out;
}

function saveBlob(bytes: Uint8Array, mediaType: string, name: string) {
  const url = URL.createObjectURL(
    new Blob([bytesToArrayBuffer(bytes)], { type: mediaType }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = name || "attachment";
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function bytesToArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
}

function artifactKind(artifact: Artifact): string {
  const metaKind = artifact._meta?.attachmentKind;
  if (typeof metaKind === "string" && metaKind) return metaKind;
  return classifyKind(artifact.mediaType, artifact.name);
}

function isPreviewable(artifact: Artifact): boolean {
  if (artifact._meta?.previewable === true) return true;
  return ["image", "audio", "video", "pdf", "text"].includes(
    artifactKind(artifact),
  );
}

function classifyKind(mediaType: string, name: string): string {
  const media = mediaType.toLowerCase();
  if (media.startsWith("image/")) return "image";
  if (media.startsWith("audio/")) return "audio";
  if (media.startsWith("video/")) return "video";
  if (media === "application/pdf") return "pdf";
  if (
    media.startsWith("text/") ||
    media === "application/json" ||
    media === "application/yaml" ||
    /\.(txt|md|markdown|json|yaml|yml|toml|ini|log|csv|ts|tsx|js|jsx|mjs|cjs|css|html|xml|rs|go|py)$/i.test(
      name,
    )
  ) {
    return "text";
  }
  if (/\.(zip|gz|tar|tgz|bz2|xz|7z|rar)$/i.test(name)) return "archive";
  return "file";
}

function iconForKind(kind: string) {
  if (kind === "image") return Image;
  if (kind === "text" || kind === "pdf") return FileText;
  if (kind === "archive") return FileArchive;
  if (kind === "audio") return Music;
  if (kind === "video") return Video;
  return File;
}

function labelForKind(kind: string): string {
  if (kind === "pdf") return "PDF";
  if (kind === "audio") return "Audio";
  if (kind === "video") return "Video";
  if (kind === "image") return "Image";
  if (kind === "archive") return "Archive";
  if (kind === "text") return "Text";
  return "File";
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) {
    return `${Math.max(0, Math.round(bytes))} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
