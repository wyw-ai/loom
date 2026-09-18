import { Loader2, Paperclip, Send } from "lucide-react";
import { Button } from "@/components/ui/button";

export function ComposerActions({
  attachLabel,
  attachDisabled,
  busy,
  sendDisabled,
  sendLabel,
  onAttach,
  onSend,
}: {
  attachLabel: string;
  attachDisabled: boolean;
  busy: boolean;
  sendDisabled: boolean;
  sendLabel: string;
  onAttach: () => void;
  onSend: () => void;
}) {
  return (
    <div className="composer-action-row">
      <button
        type="button"
        onClick={onAttach}
        disabled={attachDisabled}
        title={attachLabel}
        aria-label={attachLabel}
        className="composer-action-attach flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-[#667085] transition-colors hover:bg-[#f0f2f7] hover:text-[#1d2939] disabled:cursor-not-allowed disabled:opacity-40"
      >
        <Paperclip size={16} />
      </button>
      <Button
        size="icon"
        onClick={onSend}
        disabled={sendDisabled || busy}
        aria-label={sendLabel}
        className="composer-action-send h-9 w-9 shrink-0 rounded-lg bg-[#503ed4] text-white hover:bg-[#4635c5]"
      >
        {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
      </Button>
    </div>
  );
}
