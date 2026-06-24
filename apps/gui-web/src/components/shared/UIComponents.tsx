import type { ReactNode, SelectHTMLAttributes } from "react";
import { AgentProviderIcon, agentProviderIconKey } from "@/components/agent/AgentProviderIcon";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { ChevronDown } from "lucide-react";
import type { MachineAgentProviderInfo } from "@/ipc/types";

export function StyledSelect({
  className,
  disabled,
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className={cn("relative min-w-0", disabled && "opacity-75")}>
      <select
        {...props}
        disabled={disabled}
        className={cn(
          "h-10 w-full appearance-none rounded-lg border border-[#dfe3ec] bg-white px-3 pr-9 text-sm font-medium text-[#303849] shadow-none outline-none transition-colors",
          "hover:border-[#c8c1ff] focus:border-[#8f82ff] focus:ring-2 focus:ring-[#ece8ff]",
          "disabled:cursor-not-allowed disabled:bg-[#f6f7fb] disabled:text-[#9aa1ae]",
          className,
        )}
      >
        {children}
      </select>
      <ChevronDown
        size={15}
        className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-[#667085]"
      />
    </div>
  );
}


export function HostDetailSection({
  title,
  count,
  action,
  children,
}: {
  title: string;
  count?: number;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="border-b border-[#dfe3ec] px-6 py-5 lg:px-8">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
            {title}
          </div>
          {typeof count === "number" && (
            <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
              {count}
            </span>
          )}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}


export function ProviderBadge({ provider }: { provider: MachineAgentProviderInfo }) {
  const iconKey = agentProviderIconKey(provider.id, provider.name);
  return (
    <Badge variant="outline" className="gap-1.5">
      {iconKey && <AgentProviderIcon iconKey={iconKey} className="h-3.5 w-3.5" />}
      {provider.name}
      {provider.actorCount > 0 ? ` (${provider.actorCount})` : ""}
    </Badge>
  );
}


export function HostInfoRow({
  label,
  children,
  mono,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="grid gap-2 py-3 first:pt-0 last:pb-0 sm:grid-cols-[160px_minmax(0,1fr)]">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div
        className={cn(
          "min-w-0 text-sm text-[#303849]",
          mono && "break-all font-mono text-xs text-[#485063]",
        )}
      >
        {children}
      </div>
    </div>
  );
}


