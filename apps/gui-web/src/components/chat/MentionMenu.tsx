import { Users } from "lucide-react";
import type { MentionOption } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { ActorAvatar } from "@/components/agent/ActorAvatar";

export function MentionMenu({
  options,
  selectedIndex,
  onSelect,
}: {
  options: MentionOption[];
  selectedIndex: number;
  onSelect: (option: MentionOption) => void;
}) {
  return (
    <div className="absolute bottom-[calc(100%+8px)] left-0 z-20 w-full max-w-xl overflow-hidden rounded-xl border border-[#dfe3ec] bg-white shadow-soft">
      <div className="border-b border-[#edf0f5] px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        Mentions
      </div>
      <div className="max-h-64 overflow-y-auto py-1 scrollbar-thin">
        {options.map((option, index) => (
          <button
            key={`${option.kind}:${option.id}`}
            type="button"
            className={cn(
              "flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition-colors",
              index === selectedIndex
                ? "bg-[#f1efff] text-[#5843d7]"
                : "hover:bg-[#f7f8fb]",
            )}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(option);
            }}
          >
            {option.actor ? (
              <ActorAvatar actor={option.actor} fallback={option.actor.id} small />
            ) : (
              <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-primary/15 text-primary">
                <Users size={14} />
              </span>
            )}
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium">{option.title}</span>
              <span className="block truncate text-xs text-muted-foreground">
                {option.detail}
              </span>
            </span>
            <span className="font-mono text-xs text-muted-foreground">
              {option.token}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
