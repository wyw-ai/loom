import type { ReactNode } from "react";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

export function CoachMarkTooltip({
  children,
  className,
  onDismiss,
  title,
}: {
  children: ReactNode;
  className?: string;
  onDismiss: () => void;
  title?: string;
}) {
  return (
    <div
      role="note"
      className={cn(
        "relative rounded-xl border border-[#c8c1ff] bg-[#f7f5ff] px-4 py-3 pr-10 text-left text-sm text-[#3f357c] shadow-sm",
        className,
      )}
    >
      <div className="absolute -top-1.5 left-5 h-3 w-3 rotate-45 border-l border-t border-[#c8c1ff] bg-[#f7f5ff]" />
      {title ? (
        <div className="mb-1 text-xs font-bold uppercase tracking-wide text-[#5843d7]">
          {title}
        </div>
      ) : null}
      <div className="leading-5">{children}</div>
      <button
        type="button"
        aria-label="Dismiss tip"
        className="absolute right-2 top-2 flex h-7 w-7 items-center justify-center rounded-md text-[#6f61d9] transition-colors hover:bg-white/70 hover:text-[#3f2bb5]"
        onClick={onDismiss}
      >
        <X size={14} />
      </button>
    </div>
  );
}
