import { ChevronUp, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

type ScrollJumpButtonsProps = {
  showJumpToTop: boolean;
  showJumpToBottom: boolean;
  newMessageCount: number;
  onJumpToTop: () => void;
  onJumpToBottom: () => void;
};

function JumpButton({
  label,
  icon,
  highlighted,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  highlighted: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-xs font-medium shadow-md transition-colors",
        highlighted
          ? "border-[#503ed4] bg-[#503ed4] text-white"
          : "border-[#dfe3ec] bg-white text-[#485063] hover:bg-[#f3f4f6]",
      )}
    >
      {icon}
      {label}
    </button>
  );
}

export function ScrollJumpButtons({
  showJumpToTop,
  showJumpToBottom,
  newMessageCount,
  onJumpToTop,
  onJumpToBottom,
}: ScrollJumpButtonsProps) {
  const visible = showJumpToTop || showJumpToBottom;

  return (
    <div
      className={cn(
        "absolute bottom-4 right-4 z-50 flex flex-col items-end gap-2 transition-opacity duration-200",
        visible ? "opacity-100" : "pointer-events-none opacity-0",
      )}
    >
      {showJumpToTop && (
        <JumpButton
          label="Back to top"
          icon={<ChevronUp size={16} />}
          highlighted={false}
          onClick={onJumpToTop}
        />
      )}
      {showJumpToBottom && (
        <JumpButton
          label={
            newMessageCount > 0
              ? `${newMessageCount} new message${newMessageCount > 1 ? "s" : ""}`
              : "Latest messages"
          }
          icon={<ChevronDown size={16} />}
          highlighted={newMessageCount > 0}
          onClick={onJumpToBottom}
        />
      )}
    </div>
  );
}
