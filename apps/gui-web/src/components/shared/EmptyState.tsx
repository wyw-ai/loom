import type { ComponentType } from "react";

export function EmptyState({
  icon: Icon,
  text,
}: {
  icon: ComponentType<{ size?: string | number; className?: string }>;
  text: string;
}) {
  return (
    <div className="flex min-h-80 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] text-[#667085]">
      <Icon size={28} />
      <div className="text-sm">{text}</div>
    </div>
  );
}
