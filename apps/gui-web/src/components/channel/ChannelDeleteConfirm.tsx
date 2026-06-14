import type { Channel } from "@/ipc/types";
import { Loader2, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";

export function ChannelDeleteConfirm({
  channel,
  compact = false,
  deleteBusy,
  onCancel,
  onConfirm,
}: {
  channel: Channel;
  compact?: boolean;
  deleteBusy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (compact) {
    return (
      <div
        className="mb-2 mt-1 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-red-900"
        role="alertdialog"
        aria-label={`Delete #${channel.title}`}
      >
        <div className="flex min-w-0 items-center gap-2">
          <div className="min-w-0 flex-1">
            <div className="truncate text-xs font-bold" title={`Delete #${channel.title}?`}>
              Delete channel?
            </div>
            <div className="truncate text-[11px] font-medium text-red-700">
              Threads included.
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button
              type="button"
              className="composer-icon h-7 min-w-7 text-red-700 hover:text-red-800"
              title="Cancel"
              aria-label="Cancel channel delete"
              onClick={onCancel}
            >
              <X size={13} />
            </button>
            <Button
              type="button"
              size="sm"
              disabled={deleteBusy}
              className="h-7 px-2 text-[11px] bg-red-600 text-white hover:bg-red-700"
              onClick={onConfirm}
            >
              {deleteBusy ? <Loader2 className="animate-spin" size={12} /> : "Delete"}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="my-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-900">
      <div className="flex items-start gap-3">
        <Trash2 className="mt-0.5 shrink-0 text-red-500" size={16} />
        <div className="min-w-0 flex-1">
          <div className="font-bold">Delete #{channel.title}?</div>
          <div className="mt-1 text-xs font-medium text-red-700">
            This will delete the channel and all threads in this channel.
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button type="button" variant="outline" size="sm" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={deleteBusy}
            className="bg-red-600 text-white hover:bg-red-700"
            onClick={onConfirm}
          >
            {deleteBusy ? <Loader2 className="animate-spin" size={13} /> : <Trash2 size={13} />}
            Delete
          </Button>
        </div>
      </div>
    </div>
  );
}
