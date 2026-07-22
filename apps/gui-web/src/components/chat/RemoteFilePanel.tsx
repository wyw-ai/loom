import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import {
  CheckCircle,
  Download,
  ExternalLink,
  FileText,
  Loader2,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Artifact } from "@/ipc/types";
import { errorText, formatBytes, formatFileTimestamp } from "@/lib/format-utils";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";

interface RemoteFilePanelProps {
  channelId: string;
  machineId: string;
  dataRoot: string;
  threadId?: string;
  target: string;
  onClose: () => void;
}

type ScopeKind = "channel" | "thread";

function ArtifactEntry({ artifact }: { artifact: Artifact }) {
  const { isDownloaded, setDownloaded, clearDownloaded } = useDownloadedArtifacts();
  const localTempPath = isDownloaded(artifact.id);
  const [busy, setBusy] = useState<"download" | "open" | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function downloadFile() {
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

  async function openFile() {
    if (!localTempPath) return;
    setBusy("open");
    setError(null);
    try {
      const exists = await ipc.pathExists(localTempPath);
      if (!exists) {
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

  const name = artifact.name || artifact.id;
  const size = artifact.size;
  const modified = artifact.createdAt;

  return (
    <div className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[#f0f2f7]">
      <FileText size={16} className="shrink-0 text-[#667085]" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm text-[#303849]" title={name}>
          {name}
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
      {error && <span className="text-xs text-red-600">{error}</span>}
    </div>
  );
}

export function RemoteFilePanel({ channelId, threadId, target, onClose }: RemoteFilePanelProps) {
  const hasThread = Boolean(threadId);
  const [scope, setScope] = useState<ScopeKind>(hasThread ? "thread" : "channel");
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const activeTarget = scope === "thread" && threadId
    ? target
    : `#${channelId}`;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    ipc
      .listScopeAttachments({ target: activeTarget })
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
  }, [activeTarget]);

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
              {scope === "thread" ? `Thread ${threadId?.slice(-8) ?? ""}` : `#${channelId}`}
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

        {/* Scope Tabs */}
        {hasThread && (
          <div className="flex gap-1 border-b border-[#e4e7ef] px-4 py-2">
            <button
              type="button"
              className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                scope === "channel"
                  ? "bg-[#503ed4] text-white"
                  : "text-[#667085] hover:bg-[#f0f2f7]"
              }`}
              onClick={() => setScope("channel")}
            >
              Channel
            </button>
            <button
              type="button"
              className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                scope === "thread"
                  ? "bg-[#503ed4] text-white"
                  : "text-[#667085] hover:bg-[#f0f2f7]"
              }`}
              onClick={() => setScope("thread")}
            >
              Thread
            </button>
          </div>
        )}

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
                    .listScopeAttachments({ target: activeTarget })
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
          {!loading && !error && artifacts.length > 0 && (
            <>
              {artifacts.map((artifact) => (
                <ArtifactEntry key={artifact.id} artifact={artifact} />
              ))}
            </>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
