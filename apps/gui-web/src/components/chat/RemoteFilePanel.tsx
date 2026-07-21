import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import {
  ArrowLeft,
  CheckCircle,
  Download,
  ExternalLink,
  FileText,
  Folder,
  Loader2,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { MachineDirListResult } from "@/ipc/types";
import { errorText } from "@/lib/format-utils";

interface RemoteFilePanelProps {
  channelId: string;
  machineId: string;
  dataRoot: string;
  threadId?: string;
  onClose: () => void;
}

type ScopeKind = "channel" | "thread";

function FileEntry({
  name,
  path,
}: {
  name: string;
  path: string;
}) {
  const [localTempPath, setLocalTempPath] = useState<string | null>(null);
  const [busy, setBusy] = useState<"download" | "open" | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function downloadFile() {
    setBusy("download");
    setError(null);
    try {
      const tempPath = await ipc.downloadToTemp({
        artifactId: path,
        suggestedName: name,
      });
      setLocalTempPath(tempPath);
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
      await ipc.openFileDefault(localTempPath);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[#f0f2f7]">
      <FileText size={16} className="shrink-0 text-[#667085]" />
      <span className="min-w-0 flex-1 truncate text-sm text-[#303849]" title={name}>
        {name}
      </span>
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

export function RemoteFilePanel({ channelId, machineId, dataRoot, threadId, onClose }: RemoteFilePanelProps) {
  const hasThread = Boolean(threadId);
  const [scope, setScope] = useState<ScopeKind>(hasThread ? "thread" : "channel");
  const [dirList, setDirList] = useState<MachineDirListResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState<string | null>(null);

  const sep = dataRoot.includes("\\") && !dataRoot.includes("/") ? "\\" : "/";
  const channelRoot = `${dataRoot}${sep}channels${sep}${channelId}${sep}`;
  const threadRoot = threadId
    ? `${channelRoot}threads${sep}${threadId}${sep}`
    : null;

  const scopeRoot = scope === "thread" && threadRoot ? threadRoot : channelRoot;

  useEffect(() => {
    loadDir(scopeRoot);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeRoot]);

  async function loadDir(path: string) {
    setLoading(true);
    setError(null);
    try {
      const result = await ipc.machineDirList({ machineId, path, includeFiles: true });
      setDirList(result);
      setCurrentPath(result.path);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setLoading(false);
    }
  }

  const breadcrumbs = currentPath ? buildBreadcrumbs(currentPath, scopeRoot, sep) : [];

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
            <div className="text-sm font-bold text-[#111827]">Shared Files</div>
            <div className="mt-0.5 truncate text-xs text-[#667085]">#{channelId}</div>
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

        {/* Breadcrumbs */}
        {breadcrumbs.length > 0 && (
          <div className="flex flex-wrap items-center gap-1 border-b border-[#e4e7ef] px-4 py-2">
            {breadcrumbs.map((crumb, i) => (
              <span key={i} className="flex items-center gap-1">
                {i > 0 && <span className="text-xs text-[#667085]">{sep}</span>}
                <button
                  type="button"
                  className={`text-xs hover:underline ${i === breadcrumbs.length - 1 ? "font-bold text-[#111827]" : "text-[#667085]"}`}
                  onClick={() => loadDir(crumb.path)}
                  disabled={loading}
                >
                  {crumb.label}
                </button>
              </span>
            ))}
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
                onClick={() => loadDir(currentPath || scopeRoot)}
              >
                Retry
              </button>
            </div>
          )}
          {!loading && !error && dirList && (
            <>
              {dirList.parent && (
                <button
                  type="button"
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-[#667085] hover:bg-[#f0f2f7]"
                  onClick={() => loadDir(dirList.parent!)}
                  disabled={loading}
                >
                  <ArrowLeft size={16} />
                  ..
                </button>
              )}
              {dirList.entries.length === 0 && (
                <div className="py-8 text-center text-sm text-[#667085]">
                  No items in this directory.
                </div>
              )}
              {dirList.entries.map((entry) => {
                if (entry.kind === "directory") {
                  return (
                    <button
                      key={entry.path}
                      type="button"
                      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-[#303849] hover:bg-[#f0f2f7]"
                      onClick={() => loadDir(entry.path)}
                      disabled={loading}
                    >
                      <Folder size={16} className="shrink-0 text-[#f0a020]" />
                      <span className="min-w-0 flex-1 truncate text-left" title={entry.name}>
                        {entry.name}
                      </span>
                    </button>
                  );
                }
                return (
                  <FileEntry
                    key={entry.path}
                    name={entry.name}
                    path={entry.path}
                  />
                );
              })}
              {dirList.truncated && (
                <div className="px-2 py-2 text-xs text-[#667085]">List truncated — too many entries.</div>
              )}
            </>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

function buildBreadcrumbs(currentPath: string, rootPath: string, sep: string) {
  const rootParts = rootPath.split(sep).filter(Boolean);
  const crumbs: { label: string; path: string }[] = [];

  if (rootParts.length > 0) {
    crumbs.push({ label: rootParts[rootParts.length - 1] || "root", path: rootPath });
  }

  const normalizedCurrent = currentPath.replace(/[/\\]+$/, "");
  const normalizedRoot = rootPath.replace(/[/\\]+$/, "");
  if (normalizedCurrent.length > normalizedRoot.length) {
    const after = normalizedCurrent.slice(normalizedRoot.length).replace(/^[/\\]+/, "");
    const subParts = after.split(sep).filter(Boolean);
    let acc = normalizedRoot;
    for (const part of subParts) {
      acc = `${acc}${sep}${part}`;
      crumbs.push({ label: part, path: `${acc}${sep}` });
    }
  }

  return crumbs;
}
