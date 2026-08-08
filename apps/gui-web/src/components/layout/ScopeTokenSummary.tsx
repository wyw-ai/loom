import { useState, useRef, useEffect } from "react";
import { ChevronDown, Coins } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatCompact } from "@/lib/agent-usage-display";
import { useScopeUsageSummary } from "@/store/usageStore";
import type { Actor } from "@/ipc/types";
import { displayName } from "@/lib/format-utils";

/**
 * L1 top-bar token summary + L2 dropdown per-agent detail (Iter#5 Part E §E2).
 *
 * - L1: compact total + expand icon, silent-hidden when summary is null.
 * - L2: dropdown listing per-agent total / input / output / cache + share%.
 * - <xl breakpoint: degrades to icon + tooltip (STRAT R1 risk mitigation).
 *
 * Reused by ChatHeader (channel scope) and ThreadPanel (thread scope).
 */
export function ScopeTokenSummary({
  scopeId,
  actors,
}: {
  scopeId: string | null | undefined;
  actors: Record<string, Actor>;
}) {
  const summary = useScopeUsageSummary(scopeId);
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);

  if (!summary) return null;

  return (
    <div ref={containerRef} className="relative flex items-center">
      {/* L1 compact summary — visible at all breakpoints */}
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 rounded-lg px-2 py-1 text-xs font-semibold text-[#667085] transition-colors hover:bg-[#f5f5fa] xl:px-2.5"
        title={`Scope tokens: ${formatCompact(summary.totalTokens)} (${summary.byActor.length} agent${summary.byActor.length > 1 ? "s" : ""})`}
        aria-expanded={open}
        aria-label="Scope token summary"
      >
        <Coins size={14} className="text-[#503ed4]" />
        {/* Full label visible at xl+, icon-only below */}
        <span className="hidden xl:inline">
          <span className="text-[#503ed4]">
            {summary.estimatedTokens > 0 ? "≈" : ""}
            {formatCompact(summary.totalTokens)}
          </span>
          <span className="ml-0.5 text-[#98a2b3]">tokens</span>
        </span>
        <ChevronDown
          size={13}
          className={cn("text-[#98a2b3] transition-transform hidden xl:block", open && "rotate-180")}
        />
      </button>

      {/* L2 dropdown per-agent detail — only at xl+ */}
      {open && (
        <div className="absolute right-0 top-full z-50 mt-1 w-72 rounded-xl border border-[#e2e6ef] bg-white p-3 shadow-lg xl:block">
          <div className="mb-2 flex items-center justify-between border-b border-[#eef0f5] pb-2">
            <span className="text-xs font-bold text-[#111827]">Scope Token Usage</span>
            <span className="text-xs font-semibold text-[#503ed4]">
              {formatCompact(summary.totalTokens)} total
            </span>
          </div>
          <ul className="space-y-1.5">
            {summary.byActor.map((entry) => {
              const actor = actors[entry.actorId];
              const name = actor ? displayName(actor) : entry.actorId.slice(0, 12);
              const sharePct = summary.totalTokens > 0
                ? Math.round((entry.totalTokens / summary.totalTokens) * 100)
                : 0;
              return (
                <li key={entry.actorId} className="flex flex-col gap-0.5">
                  <div className="flex items-center justify-between">
                    <span className="truncate text-xs font-medium text-[#303849]">{name}</span>
                    <span className="ml-2 shrink-0 text-xs font-semibold text-[#503ed4]">
                      {entry.estimated ? "≈" : ""}
                      {formatCompact(entry.totalTokens)}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <div className="h-1 flex-1 overflow-hidden rounded-full bg-[#eef0f5]">
                      <div
                        className="h-full rounded-full bg-[#a981e6]"
                        style={{ width: `${sharePct}%` }}
                      />
                    </div>
                    <span className="w-8 shrink-0 text-right text-[10px] text-[#98a2b3]">{sharePct}%</span>
                  </div>
                  <div className="flex gap-2 text-[10px] text-[#98a2b3]">
                    <span>in {formatCompact(entry.inputTokens)}</span>
                    <span>out {formatCompact(entry.outputTokens)}</span>
                  </div>
                </li>
              );
            })}
          </ul>
          {(summary.cacheReadTokens > 0 || summary.estimatedTokens > 0) && (
            <div className="mt-2 border-t border-[#eef0f5] pt-2 text-[10px] text-[#98a2b3]">
              {summary.cacheReadTokens > 0 && (
                <span>Cache read: {formatCompact(summary.cacheReadTokens)} tokens</span>
              )}
              {summary.cacheReadTokens > 0 && summary.estimatedTokens > 0 && <span> · </span>}
              {summary.estimatedTokens > 0 && (
                <span>≈{formatCompact(summary.estimatedTokens)} estimated (provider reported no usage)</span>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
