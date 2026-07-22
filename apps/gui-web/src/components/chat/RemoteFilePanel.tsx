import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import {
  CheckCircle,
  Download,
  ExternalLink,
  Eye,
  FileText,
  FolderOpen,
  Loader2,
  Save,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { errorText, formatBytes, formatFileTimestamp } from "@/lib/format-utils";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";
import {
  AttachmentPreviewModal,
  artifactPreviewMode,
  artifactPreviewTextBytes,
  artifactPreviewBinaryBytes,
  readArtifactBlob,
  type ArtifactPreviewState,
} from "@/components/chat/AttachmentStack";

interface RemoteFilePanelProps {
  channelId: string;
  target: string;
  onClose: () => void;
}

function insertSuffix(filename: string, suffix: string): string {
  const dotIndex = filename.lastIndexOf(".");
  if (dotIndex > 0) {
    return `${filename.slice(0, dotIndex)}${suffix}${filename.slice(dotIndex)}`;
  }
  return `${filename}${suffix}`;
}

function ArtifactEntry({ artifact, disambigSuffix }: { artifact: Artifact; disambigSuffix?: string }) {
  const { isDownloaded, setDownloaded, clearDownloaded } = useDownloadedArtifacts();
  const localTempPath = isDownloaded(artifact.id);
  const [busy, setBusy] = useState<"download" | "open" | "reveal" | "save" | "preview" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactPreviewState | null>(null);
  const previewMode = artifactPreviewMode(artifact);

  async function downloadFile() {
    setBusy("download");
    setError(null);
    try {
      const baseName = artifact.name || `${artifact.id}.bin`;
      const downloadName = disambigSuffix ? insertSuffix(baseName, disambigSuffix) : baseName;
      const tempPath = await ipc.downloadToTemp({
        artifactId: artifact.id,
        suggestedName: downloadName,
      });
      setDownloaded(artifact.id, tempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function openFile() {
    if (!localTempPath) return;
    setBusy("open");
    setError(null);
    try {
      const exists = await ipc.pathExists(localTempPath);
      if (!exists) {
        setError("Local file deleted, re-downloading...");
        clearDownloaded(artifact.id);
        await downloadFile();
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
    if (!localTempPath) return;
    setBusy("reveal");
    setError(null);
    try {
      const exists = await ipc.pathExists(localTempPath);
      if (!exists) {
        setError("Local file deleted, re-downloading...");
        clearDownloaded(artifact.id);
        await downloadFile();
        return;
      }
      await ipc.revealInFolder(localTempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function saveAsFile() {
    setBusy("save");
    setError(null);
    try {
      const blob = await readArtifactBlob(artifact);
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const baseName = artifact.name || `${artifact.id}.bin`;
      const saveName = disambigSuffix ? insertSuffix(baseName, disambigSuffix) : baseName;
      const result = await ipc.saveFileDialog(saveName, bytes);
      if (!result) return; // user cancelled
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  async function previewArtifact() {
    if (!previewMode) return;
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
      setPreview({ mode: previewMode, artifact, objectUrl });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  function closePreview() {
    if (preview?.mode === "image" || preview?.mode === "pdf") {
      URL.revokeObjectURL(preview.objectUrl);
    }
    setPreview(null);
  }

  const baseName = artifact.name || artifact.id;
  const displayName = disambigSuffix ? insertSuffix(baseName, disambigSuffix) : baseName;
  const size = artifact.size;
  const modified = artifact.createdAt;

  return (
    <div className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[#f0f2f7]">
      <FileText size={16} className="shrink-0 text-[#667085]" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm text-[#303849]" title={displayName}>
          {displayName}
          {disambigSuffix && <span className="ml-1 text-xs text-[#98a2b3]">{disambigSuffix}</span>}
        </div>
        {(size != null || modified) && (
          <div className="truncate text-xs text-[#98a2b3]">
            {size != null && <span>{formatBytes(size)}</span>}
            {size != null && modified && <span className="mx-1">·</span>}
            {modified && <span>{formatFileTimestamp(modified)}</span>}
          </div>
        )}
      </div>
      {localTempPath && <CheckCircle size={14} className="shrink-0 text-emerald-500" />}
      {!localTempPath && (
        <button
          type="button"
          className="rounded p-1 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#1d2939] disabled:opacity-40"
          title="Download to local temp"
          disabled={Boolean(busy)}
          onClick={downloadFile}
        >
          {busy === "download" ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
        </button>
      )}
      {localTempPath && previewMode && (
        <button
          type="button"
          className="rounded p-1 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#1d2939] disabled:opacity-40"
          title="Preview"
          disabled={Boolean(busy)}
          onClick={previewArtifact}
        >
          {busy === "preview" ? <Loader2 size={14} className="animate-spin" /> : <Eye size={14} />}
        </button>
      )}
      {localTempPath && (
        <button
          type="button"
          className="rounded p-1 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#1d2939] disabled:opacity-40"
          title="Open with default app"
          disabled={Boolean(busy)}
          onClick={openFile}
        >
          {busy === "open" ? <Loader2 size={14} className="animate-spin" /> : <ExternalLink size={14} />}
        </button>
      )}
      {localTempPath && (
        <button
          type="button"
          className="rounded p-1 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#1d2939] disabled:opacity-40"
          title="Reveal in folder"
          disabled={Boolean(busy)}
          onClick={revealFile}
        >
          {busy === "reveal" ? <Loader2 size={14} className="animate-spin" /> : <FolderOpen size={14} />}
        </button>
      )}
      {localTempPath && (
        <button
          type="button"
          className="rounded p-1 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#1d2939] disabled:opacity-40"
          title="Save as..."
          disabled={Boolean(busy)}
          onClick={saveAsFile}
        >
          {busy === "save" ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
        </button>
      )}
      {error && <span className="text-xs text-red-600">{error}</span>}
      {preview && (
        <AttachmentPreviewModal preview={preview} onClose={closePreview} onDownload={saveAsFile} />
      )}
    </div>
  );
}

// I3: Compute disambiguation suffixes based on checksum comparison
function computeDisambiguation(artifacts: Artifact[]): Map<string, string | undefined> {
  const nameGroups = new Map<string, Artifact[]>();
  for (const art of artifacts) {
    const name = (art.name || art.id).toLowerCase();
    if (!nameGroups.has(name)) nameGroups.set(name, []);
    nameGroups.get(name)!.push(art);
  }

  const result = new Map<string, string | undefined>();
  for (const [, group] of nameGroups) {
    if (group.length === 1) {
      result.set(group[0].id, undefined);
      continue;
    }
    const uniqueChecksums = new Set(group.map((g) => g.checksum).filter(Boolean));
    if (uniqueChecksums.size <= 1) {
      // All same content — no suffix needed
      for (const art of group) result.set(art.id, undefined);
      continue;
    }
    // Different content — assign index per unique checksum
    const checksumOrder = new Map<string, number>();
    let nextIndex = 1;
    for (const art of group) {
      if (!checksumOrder.has(art.checksum)) {
        checksumOrder.set(art.checksum, nextIndex++);
      }
    }
    for (const art of group) {
      const idx = checksumOrder.get(art.checksum) ?? 1;
      result.set(art.id, idx > 1 ? `(${idx})` : undefined);
    }
  }
  return result;
}

export function RemoteFilePanel({ target, onClose }: RemoteFilePanelProps) {
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    ipc
      .listScopeAttachments({ target })
      .then((results) => {
        if (!cancelled) {
          setArtifacts(results.map((r) => r.artifact));
        }
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
  }, [target]);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="Shared files"
      onMouseDown={onClose}
    >
      <div
        className="flex max-h-[80vh] w-full max-w-2xl flex-col rounded-lg border border-[#dfe3ec] bg-white shadow-soft"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[#e4e7ef] px-4 py-3">
          <div className="min-w-0">
            <div className="text-sm font-bold text-[#111827]">Attachments</div>
            <div className="mt-0.5 truncate text-xs text-[#667085]">
              {target}
            </div>
          </div>
          <button
            className="composer-icon h-7 min-w-7"
            type="button"
            title="Close"
            onClick={onClose}
          >
            <X size={13} />
          </button>
        </div>

        {/* Content */}
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {loading && (
            <div className="flex items-center justify-center py-8 text-sm text-[#667085]">
              <Loader2 size={18} className="mr-2 animate-spin" />
              Loading...
            </div>
          )}
          {error && (
            <div className="px-3 py-4 text-sm text-red-600">
              Error: {error}
              <button
                type="button"
                className="ml-2 text-xs text-[#667085] hover:underline"
                onClick={() => {
                  setLoading(true);
                  setError(null);
                  ipc
                    .listScopeAttachments({ target })
                    .then((results) => setArtifacts(results.map((r) => r.artifact)))
                    .catch((err) => setError(errorText(err)))
                    .finally(() => setLoading(false));
                }}
              >
                Retry
              </button>
            </div>
          )}
          {!loading && !error && artifacts.length === 0 && (
            <div className="py-8 text-center text-sm text-[#667085]">
              No attachments
            </div>
          )}
          {!loading && !error && artifacts.length > 0 && (() => {
            const disambig = computeDisambiguation(artifacts);
            return (
              <>
                {artifacts.map((artifact) => (
                  <ArtifactEntry
                    key={artifact.id}
                    artifact={artifact}
                    disambigSuffix={disambig.get(artifact.id)}
                  />
                ))}
              </>
            );
          })()}
        </div>
      </div>
    </div>,
    document.body,
  );
}
