import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CheckCircle, Download, ExternalLink, Eye, FileText, FolderOpen, Loader2, Save, X } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Artifact, ArtifactReadResult } from "@/ipc/types";
import { errorText, formatBytes } from "@/lib/format-utils";
import { attachmentKind, attachmentTitle, metadataString } from "@/lib/message-utils";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";

type ArtifactPreviewState =
  | {
      mode: "text";
      artifact: Artifact;
      content: string;
      truncated: boolean;
    }
  | {
      mode: "image" | "pdf";
      artifact: Artifact;
      objectUrl: string;
    };

const artifactReadChunkBytes = 1024 * 1024;
const artifactPreviewTextBytes = 256 * 1024;
const artifactPreviewBinaryBytes = 25 * 1024 * 1024;

function AttachmentCard({ attachment }: { attachment: string }) {
  const [artifact, setArtifact] = useState<Artifact | null>(null);
  const [loading, setLoading] = useState(() => Boolean(artifactLookupParams(attachment)));
  const [busy, setBusy] = useState<"preview" | "download" | "open" | "reveal" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactPreviewState | null>(null);
  const { isDownloaded, setDownloaded, clearDownloaded } = useDownloadedArtifacts();
  const previewObjectUrlRef = useRef<string | null>(null);

  const title = artifact?.name || attachmentTitle(attachment);
  const localTempPath = artifact ? isDownloaded(artifact.id) : null;
  const previewMode = artifact ? artifactPreviewMode(artifact) : null;
  const detail = loading
    ? "Loading artifact..."
    : artifact
      ? `${artifactKindLabel(artifact)} - ${formatBytes(artifact.size)}`
      : error
        ? "Failed to load metadata"
        : attachmentKind(attachment);

  useEffect(() => {
    const params = artifactLookupParams(attachment);
    if (!params) {
      setArtifact(null);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    ipc
      .artifactGet(params)
      .then((result) => {
        if (!cancelled) setArtifact(result.artifact);
      })
      .catch((err) => {
        if (!cancelled) setError(errorText(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [attachment]);

  useEffect(() => {
    return () => {
      if (previewObjectUrlRef.current) {
        URL.revokeObjectURL(previewObjectUrlRef.current);
        previewObjectUrlRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    if (!artifact) return;
    let cancelled = false;
    ipc
      .artifactExists({ artifactId: artifact.id })
      .then(() => {
        // Server confirms artifact is accessible
      })
      .catch(() => {
        // Server API check failed — silent degradation, file still available via Download
        if (!cancelled) console.warn("artifactExists check failed for", artifact.id);
      });
    return () => {
      cancelled = true;
    };
  }, [artifact]);

  function closePreview() {
    if (previewObjectUrlRef.current) {
      URL.revokeObjectURL(previewObjectUrlRef.current);
      previewObjectUrlRef.current = null;
    }
    setPreview(null);
  }

  async function previewArtifact() {
    if (!artifact || !previewMode) return;
    setBusy("preview");
    setError(null);
    try {
      if (previewMode === "text") {
        const read = await ipc.artifactRead({
          artifactId: artifact.id,
          maxBytes: artifactPreviewTextBytes,
        });
        setPreview({
          mode: "text",
          artifact,
          content: read.content,
          truncated: read.truncated,
        });
        return;
      }
      if (artifact.size > artifactPreviewBinaryBytes) {
        throw new Error("Preview is too large. Download the artifact instead.");
      }
      const blob = await readArtifactBlob(artifact);
      const objectUrl = URL.createObjectURL(blob);
      closePreview();
      previewObjectUrlRef.current = objectUrl;
      setPreview({ mode: previewMode, artifact, objectUrl });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function saveAsFile() {
    if (!artifact) return;
    setBusy("save");
    setError(null);
    try {
      const blob = await readArtifactBlob(artifact);
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const result = await ipc.saveFileDialog(artifact.name || `${artifact.id}.bin`, bytes);
      if (!result) return; // user cancelled
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function openFile() {
    if (!artifact || !localTempPath) return;
    setBusy("open");
    setError(null);
    try {
      const exists = await ipc.pathExists(localTempPath);
      if (!exists) {
        // Temp file was cleaned by system — re-download
        clearDownloaded(artifact.id);
        await downloadArtifact();
        return;
      }
      await ipc.openFileDefault(localTempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function revealFile() {
    if (!artifact || !localTempPath) return;
    setBusy("reveal");
    setError(null);
    try {
      const exists = await ipc.pathExists(localTempPath);
      if (!exists) {
        clearDownloaded(artifact.id);
        await downloadArtifact();
        return;
      }
      await ipc.revealInFolder(localTempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function downloadArtifact() {
    if (!artifact) return;
    setBusy("download");
    setError(null);
    try {
      const tempPath = await ipc.downloadToTemp({
        artifactId: artifact.id,
        suggestedName: artifact.name || `${artifact.id}.bin`,
      });
      setDownloaded(artifact.id, tempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <div className={`attachment-card ${error ? "border-red-200 bg-red-50" : ""}`}>
        <div className="attachment-icon">
          {busy || loading ? <Loader2 className="animate-spin" size={18} /> : <FileText size={18} />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-bold text-[#303849]" title={title}>
            {title}
            {artifact && localTempPath && (
              <CheckCircle size={13} className="ml-1 inline-block shrink-0 text-emerald-500" aria-label="Downloaded to local temp" />
            )}
          </div>
          <div className={`mt-0.5 truncate text-xs font-medium ${error ? "text-red-700" : "text-[#667085]"}`}>
            {detail}
          </div>
        </div>
        <div className="attachment-file-badge" aria-hidden="true">
          {attachmentBadge(title, artifact)}
        </div>
        {artifact && (
          <div className="attachment-actions">
            {previewMode && localTempPath && (
              <button
                type="button"
                className="attachment-action"
                title="Preview artifact"
                aria-label={`Preview ${title}`}
                disabled={Boolean(busy)}
                onClick={previewArtifact}
              >
                {busy === "preview" ? <Loader2 className="animate-spin" size={14} /> : <Eye size={14} />}
              </button>
            )}
            {!localTempPath && (
              <button
                type="button"
                className="attachment-action"
                title="Download to local temp"
                aria-label={`Download ${title}`}
                disabled={Boolean(busy)}
                onClick={downloadArtifact}
              >
                {busy === "download" ? <Loader2 className="animate-spin" size={14} /> : <Download size={14} />}
              </button>
            )}
            {localTempPath && (
              <button
                type="button"
                className="attachment-action"
                title="Open with default app"
                aria-label={`Open ${title}`}
                disabled={Boolean(busy)}
                onClick={openFile}
              >
                {busy === "open" ? <Loader2 className="animate-spin" size={14} /> : <ExternalLink size={14} />}
              </button>
            )}
            {localTempPath && (
              <button
                type="button"
                className="attachment-action"
                title="Reveal in folder"
                aria-label={`Reveal ${title} in folder`}
                disabled={Boolean(busy)}
                onClick={revealFile}
              >
                {busy === "reveal" ? <Loader2 className="animate-spin" size={14} /> : <FolderOpen size={14} />}
              </button>
            )}
            {localTempPath && (
              <button
                type="button"
                className="attachment-action"
                title="Save as..."
                aria-label={`Save ${title} as`}
                disabled={Boolean(busy)}
                onClick={saveAsFile}
              >
                {busy === "save" ? <Loader2 className="animate-spin" size={14} /> : <Save size={14} />}
              </button>
            )}
          </div>
        )}
      </div>
      {preview && (
        <AttachmentPreviewModal
          preview={preview}
          onClose={closePreview}
          onDownload={saveAsFile}
        />
      )}
    </>
  );
}

export function AttachmentStack({ attachments }: { attachments: string[] }) {
  return (
    <div className="mt-3 grid w-full max-w-[640px] min-w-0 gap-2">
      {attachments.map((attachment, index) => (
        <AttachmentCard key={`${attachment}:${index}`} attachment={attachment} />
      ))}
    </div>
  );
}

function AttachmentPreviewModal({
  preview,
  onClose,
  onDownload,
}: {
  preview: ArtifactPreviewState;
  onClose: () => void;
  onDownload: () => void;
}) {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const title = preview.artifact.name || preview.artifact.id;
  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={`Preview ${title}`}
      onClick={onClose}
    >
      <section
        className="flex max-h-[86vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl bg-white shadow-soft"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="flex min-h-14 items-center gap-3 border-b border-[#e4e7ef] px-4">
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-bold text-[#111827]" title={title}>
              {title}
            </div>
            <div className="truncate text-xs font-medium text-[#667085]">
              {artifactKindLabel(preview.artifact)} - {formatBytes(preview.artifact.size)}
            </div>
          </div>
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-xs font-bold text-[#303849] hover:bg-[#f1efff] hover:text-[#503ed4]"
            onClick={onDownload}
          >
            <Download size={14} />
            Download
          </button>
          <button
            type="button"
            className="composer-icon h-8 min-w-8"
            title="Close"
            aria-label="Close preview"
            onClick={onClose}
          >
            <X size={15} />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-auto bg-[#f7f8fb] p-4">
          {preview.mode === "text" ? (
            <>
              <pre className="m-0 min-h-[360px] whitespace-pre-wrap break-words rounded-lg border border-[#dfe3ec] bg-white p-4 font-mono text-xs leading-5 text-[#111827]">
                {preview.content}
              </pre>
              {preview.truncated && (
                <div className="mt-2 text-xs font-medium text-[#667085]">
                  Preview truncated at {formatBytes(artifactPreviewTextBytes)}.
                </div>
              )}
            </>
          ) : preview.mode === "image" ? (
            <img
              src={preview.objectUrl}
              alt={title}
              className="mx-auto max-h-[72vh] max-w-full rounded-lg border border-[#dfe3ec] bg-white object-contain"
            />
          ) : (
            <iframe
              title={title}
              src={preview.objectUrl}
              className="h-[72vh] w-full rounded-lg border border-[#dfe3ec] bg-white"
            />
          )}
        </div>
      </section>
    </div>,
    document.body,
  );
}

function artifactLookupParams(value: string) {
  const clean = value.trim();
  if (!clean) return null;
  if (clean.startsWith("artifact://")) return { artifactUri: clean };
  if (/^art_[A-Za-z0-9_-]+$/.test(clean)) return { artifactId: clean };
  return null;
}

function artifactKindLabel(artifact: Artifact) {
  const metaKind = metadataString(artifact._meta, ["attachmentKind", "attachment_kind"]);
  if (metaKind) return titleCaseWords(metaKind.replace(/[_-]+/g, " "));
  const media = artifact.mediaType.toLowerCase();
  if (media.startsWith("image/")) return "Image";
  if (media === "application/pdf") return "PDF File";
  if (media.includes("json")) return "JSON File";
  if (media.includes("yaml") || media.includes("yml")) return "YAML File";
  if (media.includes("markdown")) return "Markdown File";
  if (media.startsWith("text/")) return "Text File";
  if (media.includes("zip") || media.includes("tar") || media.includes("gzip")) return "Archive";
  return attachmentKind(artifact.name);
}

function artifactPreviewMode(artifact: Artifact): ArtifactPreviewState["mode"] | null {
  const media = artifact.mediaType.toLowerCase();
  if (media.startsWith("image/")) return "image";
  if (media === "application/pdf") return "pdf";
  if (isTextLikeMedia(media) || textLikeExtension(artifact.name)) return "text";
  const previewable = artifact._meta?.previewable;
  if (previewable === true && !media.startsWith("audio/") && !media.startsWith("video/")) {
    return "text";
  }
  return null;
}

function isTextLikeMedia(media: string) {
  return (
    media.startsWith("text/") ||
    media.includes("json") ||
    media.includes("xml") ||
    media.includes("yaml") ||
    media.includes("toml") ||
    media.includes("javascript") ||
    media.includes("typescript") ||
    media.includes("markdown")
  );
}

function textLikeExtension(name: string) {
  return /\.(md|txt|json|ya?ml|toml|csv|tsv|html?|css|tsx?|jsx?|rs|py|go|java|c|cpp|h|hpp|sh|sql)$/i.test(
    name,
  );
}

function attachmentBadge(title: string, artifact: Artifact | null) {
  const ext = title.split(".").at(-1);
  if (ext && ext !== title && ext.length <= 5) return ext.toUpperCase();
  if (artifact?.mediaType.startsWith("image/")) return "IMG";
  if (artifact?.mediaType === "application/pdf") return "PDF";
  return "FILE";
}

function titleCaseWords(value: string) {
  return value
    .split(/\s+/)
    .filter(Boolean)
    .map((word) => word.slice(0, 1).toUpperCase() + word.slice(1))
    .join(" ");
}

function artifactReadBytes(read: ArtifactReadResult) {
  if (read.bytes && read.bytes.length > 0) return Uint8Array.from(read.bytes);
  if (read.content) return new TextEncoder().encode(read.content);
  return new Uint8Array();
}

async function readArtifactBlob(artifact: Artifact) {
  const chunks: Uint8Array[] = [];
  let offset = 0;
  let mediaType = artifact.mediaType;
  for (let chunkIndex = 0; chunkIndex < 512; chunkIndex += 1) {
    const read = await ipc.artifactRead({
      artifactId: artifact.id,
      offset,
      maxBytes: artifactReadChunkBytes,
    });
    mediaType = read.mediaType || mediaType;
    const bytes = artifactReadBytes(read);
    chunks.push(bytes);
    if (!read.truncated) {
      const parts = chunks.map((chunk) => {
        const copy = new Uint8Array(chunk.byteLength);
        copy.set(chunk);
        return copy.buffer;
      });
      return new Blob(parts, { type: mediaType || "application/octet-stream" });
    }
    const nextOffset = read.nextOffset ?? offset + bytes.byteLength;
    if (nextOffset <= offset) {
      throw new Error("Artifact read did not advance.");
    }
    offset = nextOffset;
  }
  throw new Error("Artifact is too large to download in one operation.");
}
