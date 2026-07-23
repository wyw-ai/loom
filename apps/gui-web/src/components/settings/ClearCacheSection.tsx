import { useCallback, useEffect, useState } from "react";
import { HardDrive, Loader2, Trash2 } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { errorText } from "@/lib/format-utils";
import { formatBytes } from "@/lib/format-utils";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";
import { clearAllObjectUrls } from "@/hooks/useAutoDownloadImage";
import { SettingsSection } from "@/components/settings/SettingsSection";

/**
 * Settings panel section for clearing the attachment image cache.
 *
 * ARCH D3-r1 design (AC-P0-10):
 * - Displays current cache size
 * - Confirm dialog before clearing
 * - On clear: deletes disk files + clears localStorage + clears ObjectURL memory
 * - Components auto re-render and re-download visible images
 */
export function ClearCacheSection() {
  const { clearAllDownloaded } = useDownloadedArtifacts();
  const [cacheSize, setCacheSize] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshSize = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const size = await ipc.getAttachmentCacheSize();
      setCacheSize(size);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshSize();
  }, [refreshSize]);

  async function handleClear() {
    setClearing(true);
    setError(null);
    try {
      await ipc.clearAttachmentCache();
      clearAllDownloaded();
      clearAllObjectUrls();
      setCacheSize(0);
      setConfirming(false);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setClearing(false);
    }
  }

  return (
    <SettingsSection
      title="Attachment Cache"
      detail="Downloaded image attachments cached on disk for fast display."
      action={
        <button
          type="button"
          className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-[#dfe3ec] bg-white px-2.5 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
          title="Refresh cache size"
          onClick={() => void refreshSize()}
          disabled={loading}
        >
          {loading ? <Loader2 className="animate-spin" size={13} /> : <HardDrive size={13} />}
          {loading ? "Checking…" : `${cacheSize !== null ? formatBytes(cacheSize) : "—"}`}
        </button>
      }
    >
      {error && (
        <div className="mb-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
          {error}
        </div>
      )}
      {confirming ? (
        <div className="flex items-center gap-3">
          <span className="text-sm text-[#667085]">
            Clear all cached images? They will be re-downloaded when viewed again.
          </span>
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-lg bg-red-500 px-3 text-xs font-bold text-white hover:bg-red-600 disabled:opacity-50"
            onClick={() => void handleClear()}
            disabled={clearing}
          >
            {clearing ? <Loader2 className="animate-spin" size={13} /> : <Trash2 size={13} />}
            {clearing ? "Clearing…" : "Confirm Clear"}
          </button>
          <button
            type="button"
            className="inline-flex h-8 items-center rounded-lg border border-[#dfe3ec] bg-white px-3 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
            onClick={() => setConfirming(false)}
            disabled={clearing}
          >
            Cancel
          </button>
        </div>
      ) : (
        <button
          type="button"
          className="inline-flex h-9 items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-4 text-sm font-bold text-red-600 hover:bg-red-100 disabled:opacity-50"
          onClick={() => setConfirming(true)}
          disabled={clearing || cacheSize === 0}
        >
          <Trash2 size={15} />
          Clear Cache
        </button>
      )}
    </SettingsSection>
  );
}
