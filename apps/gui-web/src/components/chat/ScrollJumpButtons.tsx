import { ChevronUp, ChevronDown } from "lucide-react";

type ScrollJumpButtonsProps = {
  showJumpToTop: boolean;
  showJumpToBottom: boolean;
  onJumpToTop: () => void;
  onJumpToBottom: () => void;
};

export function ScrollJumpButtons({
  showJumpToTop,
  showJumpToBottom,
  onJumpToTop,
  onJumpToBottom,
}: ScrollJumpButtonsProps) {
  if (!showJumpToTop && !showJumpToBottom) return null;

  return (
    <div className="absolute bottom-4 right-4 z-10 flex flex-col gap-2 opacity-80 transition-opacity hover:opacity-100">
      {showJumpToTop && (
        <button
          type="button"
          onClick={onJumpToTop}
          className="flex h-9 w-9 items-center justify-center rounded-full border border-[#dfe3ec] bg-white text-[#667085] shadow-md transition-colors hover:bg-[#f3f4f6] hover:text-[#1f2937]"
          aria-label="Jump to top"
        >
          <ChevronUp className="h-5 w-5" />
        </button>
      )}
      {showJumpToBottom && (
        <button
          type="button"
          onClick={onJumpToBottom}
          className="flex h-9 w-9 items-center justify-center rounded-full border border-[#dfe3ec] bg-white text-[#667085] shadow-md transition-colors hover:bg-[#f3f4f6] hover:text-[#1f2937]"
          aria-label="Jump to bottom"
        >
          <ChevronDown className="h-5 w-5" />
        </button>
      )}
    </div>
  );
}
