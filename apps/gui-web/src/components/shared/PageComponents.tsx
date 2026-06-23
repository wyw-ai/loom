import { Button } from "@/components/ui/button";
import { localServerCommand } from "@/lib/constants";
import { Server } from "lucide-react";

export function PageHeader({ title, detail }: { title: string; detail: string }) {
  return (
    <header className="flex h-[86px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
      <h1 className="text-[22px] font-bold text-[#111827]">{title}</h1>
      <span className="text-sm font-medium text-[#667085]">{detail}</span>
    </header>
  );
}


export function ErrorBanner({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <div className="border-b border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700">
      {error}
    </div>
  );
}


export function NoSpaceConnectionGuide({
  onUseLocalServer,
}: {
  onUseLocalServer: () => void;
}) {
  return (
    <div className="mt-4 space-y-3">
      <div className="rounded-lg border border-[#dfe3ec] bg-white px-3 py-2 text-left">
        <div className="text-xs font-semibold uppercase tracking-wide text-[#667085]">
          Make a machine the server
        </div>
        <div className="mt-1 text-xs font-medium text-[#667085]">
          Run this on the machine that should host Loom.
        </div>
        <code className="mt-1 block truncate font-mono text-xs font-semibold text-[#303849]">
          {localServerCommand}
        </code>
      </div>
      <Button
        size="sm"
        onClick={onUseLocalServer}
        className="rounded-lg"
      >
        <Server size={14} />
        Prepare Space Connection
      </Button>
    </div>
  );
}


