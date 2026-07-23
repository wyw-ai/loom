import { FileText, X } from "lucide-react";
import type { PendingAttachment } from "@/lib/attachment-utils";
import { formatFileSize, copyAttachmentToClipboard } from "@/lib/attachment-utils";

interface AttachmentPreviewBarProps {
  attachments: PendingAttachment[];
  onRemove: (id: string) => void;
  /** IDs of attachments added most recently (for paste highlight animation). */
  recentlyAddedIds?: string[];
}

export function AttachmentPreviewBar({ attachments, onRemove, recentlyAddedIds }: AttachmentPreviewBarProps) {
  if (attachments.length === 0) return null;
  return (
    <div className="composer-attachment-bar mb-2 flex shrink-0 gap-2 overflow-x-auto">
      {attachments.map((attachment) => {
        const isHighlighted = recentlyAddedIds?.includes(attachment.id) ?? false;
        return (
          <div
            key={attachment.id}
            className={`composer-attachment-chip flex items-center gap-2 rounded-lg border border-[#dfe3ec] bg-[#f7f8fb] px-2.5 py-1.5${isHighlighted ? " animate-paste-highlight" : ""}`}
            tabIndex={0}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "c") {
                e.preventDefault();
                const bytes = new Uint8Array(attachment.bytes);
                const blob = new Blob([bytes.buffer as ArrayBuffer], { type: attachment.mediaType });
                copyAttachmentToClipboard(blob, attachment.mediaType);
              }
            }}
            title={`${attachment.name} — Ctrl+C to copy`}
          >
            <FileText size={14} className="shrink-0 text-[#667085]" />
            <div className="min-w-0 max-w-[160px]">
              <div className="truncate text-xs font-bold text-[#303849]" title={attachment.name}>
                {attachment.name}
              </div>
              <div className="text-[10px] font-medium text-[#667085]">
                {formatFileSize(attachment.size)}
              </div>
            </div>
            <button
              type="button"
              className="shrink-0 rounded p-0.5 text-[#667085] hover:bg-[#e4e7ef] hover:text-[#303849]"
              onClick={() => onRemove(attachment.id)}
              aria-label={`Remove ${attachment.name}`}
            >
              <X size={13} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
