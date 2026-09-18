import { useCallback, useEffect, useState } from "react";
import { HardDrive, ImageIcon, FileIcon, FolderOpen, Loader2, Trash2 } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { errorText, formatBytes } from "@/lib/format-utils";
import { useDownloadedArtifacts } from "@/hooks/useDownloadedArtifacts";
import { clearAllObjectUrls } from "@/hooks/useAutoDownloadImage";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { useI18n } from "@/lib/i18n";

interface CacheBreakdown {
  images: { size: number; count: number };
  other: { size: number; count: number };
  total: { size: number; count: number };
  cachedIds: string[];
}

type ClearTarget = "images" | "other" | "all";

/**
 * Cache management section with per-category breakdown and clearing.
 *
 * ARCH D3 design:
 * - Displays cache size broken down by Images / Other Files
 * - Per-category clear buttons + full clear
 * - On clear: deletes disk files + syncs localStorage (clearedIds) + clears ObjectURLs
 */
export function CacheManagementSection() {
  const { t } = useI18n();
  const { clearDownloadedByIds, reconcileDownloaded } = useDownloadedArtifacts();
  const [breakdown, setBreakdown] = useState<CacheBreakdown | null>(null);
  const [loading, setLoading] = useState(false);
  const [clearing, setClearing] = useState<ClearTarget | null>(null);
  const [confirming, setConfirming] = useState<ClearTarget | null>(null);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const bd = await ipc.getAttachmentCacheBreakdown();
      setBreakdown(bd);
      // ARCH TODO#2 Tier 2: reconcile localStorage mappings against the
      // on-disk cache. Removes orphan entries whose backing file was
      // externally deleted (e.g. via file manager or disk cleanup).
      reconcileDownloaded(bd.cachedIds);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setLoading(false);
    }
  }, [reconcileDownloaded]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleClear(target: ClearTarget) {
    setClearing(target);
    setError(null);
    try {
      if (target === "all") {
        // AC-A6: clearAttachmentCache now returns { freedBytes, clearedIds };
        // consume clearedIds to remove only the actually-cleared localStorage
        // entries, matching the by-type path (symmetric contract).
        const result = await ipc.clearAttachmentCache();
        clearDownloadedByIds(result.clearedIds);
        clearAllObjectUrls();
      } else {
        const result = await ipc.clearAttachmentCacheByType(target);
        clearDownloadedByIds(result.clearedIds);
        clearAllObjectUrls();
      }
      setConfirming(null);
      await refresh();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setClearing(null);
    }
  }

  async function handleOpenFolder() {
    setOpening(true);
    setError(null);
    try {
      await ipc.openAttachmentCacheDirectory();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setOpening(false);
    }
  }

  const totalSize = breakdown?.total.size ?? null;
  const hasCache = totalSize !== null && totalSize > 0;

  return (
    <SettingsSection
      title={t("Cache Management")}
      detail={t("Downloaded attachments cached on disk for fast display.")}
      action={
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-[#dfe3ec] bg-white px-2.5 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
            title={t("Open cache folder in file manager")}
            onClick={() => void handleOpenFolder()}
            disabled={opening}
          >
            {opening ? <Loader2 className="animate-spin" size={13} /> : <FolderOpen size={13} />}
            {opening ? t("Opening…") : t("Open Folder")}
          </button>
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-[#dfe3ec] bg-white px-2.5 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
            title={t("Refresh cache info")}
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading ? <Loader2 className="animate-spin" size={13} /> : <HardDrive size={13} />}
            {loading ? t("Checking…") : `${totalSize !== null ? formatBytes(totalSize) : "—"}`}
          </button>
        </div>
      }
    >
      {error && (
        <div className="mb-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
          {error}
        </div>
      )}

      {/* Category rows */}
      <div className="space-y-2">
        <CategoryRow
          icon={<ImageIcon size={15} />}
          label={t("Images")}
          size={breakdown?.images.size ?? null}
          count={breakdown?.images.count ?? null}
          percent={calcPercent(breakdown?.images.size ?? null, totalSize)}
          confirming={confirming === "images"}
          clearing={clearing === "images"}
          disabled={!hasCache || (breakdown?.images.size ?? 0) === 0}
          onClear={() => setConfirming("images")}
          onConfirm={() => void handleClear("images")}
          onCancel={() => setConfirming(null)}
        />
        <CategoryRow
          icon={<FileIcon size={15} />}
          label={t("Other Files")}
          size={breakdown?.other.size ?? null}
          count={breakdown?.other.count ?? null}
          percent={calcPercent(breakdown?.other.size ?? null, totalSize)}
          confirming={confirming === "other"}
          clearing={clearing === "other"}
          disabled={!hasCache || (breakdown?.other.size ?? 0) === 0}
          onClear={() => setConfirming("other")}
          onConfirm={() => void handleClear("other")}
          onCancel={() => setConfirming(null)}
        />
      </div>

      {/* Full clear */}
      <div className="mt-4 border-t border-[#eef0f5] pt-3">
        {confirming === "all" ? (
          <div className="flex items-center gap-3">
            <span className="text-sm text-[#667085]">
              {t("Clear all cached files? They will be re-downloaded when viewed again.")}
            </span>
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1.5 rounded-lg bg-red-500 px-3 text-xs font-bold text-white hover:bg-red-600 disabled:opacity-50"
              onClick={() => void handleClear("all")}
              disabled={clearing !== null}
            >
              {clearing === "all" ? <Loader2 className="animate-spin" size={13} /> : <Trash2 size={13} />}
              {clearing === "all" ? t("Clearing…") : t("Confirm Clear All")}
            </button>
            <button
              type="button"
              className="inline-flex h-8 items-center rounded-lg border border-[#dfe3ec] bg-white px-3 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
              onClick={() => setConfirming(null)}
              disabled={clearing !== null}
            >
              {t("Cancel")}
            </button>
          </div>
        ) : (
          <button
            type="button"
            className="inline-flex h-9 items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-4 text-sm font-bold text-red-600 hover:bg-red-100 disabled:opacity-50"
            onClick={() => setConfirming("all")}
            disabled={!hasCache || clearing !== null}
          >
            <Trash2 size={15} />
            {t("Clear All Cache")}
          </button>
        )}
      </div>
    </SettingsSection>
  );
}

function CategoryRow({
  icon,
  label,
  size,
  count,
  percent,
  confirming,
  clearing,
  disabled,
  onClear,
  onConfirm,
  onCancel,
}: {
  icon: React.ReactNode;
  label: string;
  size: number | null;
  count: number | null;
  percent: number | null;
  confirming: boolean;
  clearing: boolean;
  disabled: boolean;
  onClear: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="flex items-center gap-3 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2.5">
      <span className="text-[#667085]">{icon}</span>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-semibold text-[#303849]">{label}</div>
        <div className="text-xs text-[#667085]">
          {size !== null ? formatBytes(size) : "—"}
          {percent !== null ? ` · ${percent}%` : ""}
          {count !== null && count > 0
            ? ` · ${t(count === 1 ? "{{count}} file" : "{{count}} files", { count })}`
            : ""}
        </div>
      </div>
      {confirming ? (
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="inline-flex h-7 items-center gap-1 rounded-md bg-red-500 px-2.5 text-xs font-bold text-white hover:bg-red-600 disabled:opacity-50"
            onClick={onConfirm}
            disabled={clearing}
          >
            {clearing ? <Loader2 className="animate-spin" size={12} /> : <Trash2 size={12} />}
            {clearing ? "…" : t("Clear")}
          </button>
          <button
            type="button"
            className="inline-flex h-7 items-center rounded-md border border-[#dfe3ec] bg-white px-2.5 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb]"
            onClick={onCancel}
            disabled={clearing}
          >
            {t("Cancel")}
          </button>
        </div>
      ) : (
        <button
          type="button"
          className="inline-flex h-7 items-center gap-1 rounded-md border border-[#dfe3ec] bg-white px-2.5 text-xs font-bold text-[#596174] hover:bg-[#f7f7fb] disabled:opacity-40"
          onClick={onClear}
          disabled={disabled}
          title={t("Clear {{category}}", { category: label.toLocaleLowerCase() })}
        >
          <Trash2 size={12} />
          {t("Clear")}
        </button>
      )}
    </div>
  );
}

/** Calculate integer percentage of category size relative to total. */
export function calcPercent(size: number | null, total: number | null): number | null {
  if (size === null || total === null || total <= 0) return null;
  return Math.round((size / total) * 100);
}
