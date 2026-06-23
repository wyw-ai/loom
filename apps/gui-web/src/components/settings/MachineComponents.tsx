import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { createPortal } from "react-dom";
import { HostDetailSection, HostInfoRow, ProviderBadge } from "@/components/shared/UIComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { capitalize, statusDotClass } from "@/lib/format-utils";
import { cn, formatTime } from "@/lib/utils";
import type { MachineInfo } from "@/ipc/types";
import { Check, HardDrive, Loader2, Plus, Server, Trash2, X } from "lucide-react";

export function HostMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="min-w-20 border-l border-[#dfe3ec] pl-4">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div className="mt-1 text-xl font-bold text-[#111827]">{value}</div>
    </div>
  );
}

export function HostListItem({
  machine,
  selected,
  onSelect,
}: {
  machine: MachineInfo;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl border px-3 py-3 text-left transition-colors",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
          : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
      )}
      onClick={onSelect}
    >
      <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[#6f83f7] to-[#4e3ad5] text-white">
        <Server size={17} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-bold text-[#111827]">{machine.name}</span>
          <span className={cn("h-2 w-2 shrink-0 rounded-full", statusDotClass(machine.connectionStatus))} />
        </span>
        <span className="mt-1 block truncate text-xs text-[#667085]">
          {machine.providers.length} runtimes · {machine.onlineAgentCount}/{machine.agentCount} agents online
        </span>
      </span>
    </button>
  );
}

export function RegisteredHostsEmpty({
  busy,
  onOpenRegisterHost,
}: {
  busy: string | null;
  onOpenRegisterHost: () => void;
}) {
  return (
    <div className="flex min-h-full items-center justify-center bg-white px-6 py-10">
      <div className="w-full max-w-xl rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-6 text-center">
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-[#f1efff] text-[#503ed4]">
          <Server size={22} />
        </div>
        <h2 className="mt-4 text-lg font-bold text-[#111827]">Register a Host</h2>
        <p className="mt-2 text-sm leading-6 text-[#667085]">
          Prepare a daemon registration for this space, then start the generated command so
          the host can publish its runtime inventory.
        </p>
        <Button
          className="mt-5 rounded-lg"
          onClick={onOpenRegisterHost}
          disabled={busy === "machine:create"}
        >
          {busy === "machine:create" ? (
            <Loader2 className="animate-spin" size={15} />
          ) : (
            <Plus size={15} />
          )}
          Register Host
        </Button>
      </div>
    </div>
  );
}

export function HostRegisterDialog({
  busy,
  onCreateMachine,
  onCreated,
  onClose,
}: {
  busy: string | null;
  onCreateMachine: (args: {
    name: string;
    dataRoot?: string;
  }) => Promise<MachineInfo | null> | MachineInfo | null;
  onCreated: (machine: MachineInfo) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("Local Host");
  const [dataRoot, setDataRoot] = useState("");
  const creating = busy === "machine:create";
  const canSubmit = Boolean(name.trim()) && !creating;

  useEffect(() => {
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && !creating) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [creating, onClose]);

  async function submitHost(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSubmit) return;
    const machine = await onCreateMachine({
      name,
      dataRoot: dataRoot.trim() || undefined,
    });
    if (!machine) return;
    onCreated(machine);
    onClose();
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="host-register-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !creating) onClose();
      }}
    >
      <form
        className="flex w-full max-w-xl flex-col rounded-2xl border border-[#dfe3ec] bg-white shadow-[0_28px_80px_rgb(16_24_40_/_0.22)]"
        onSubmit={submitHost}
      >
        <div className="flex items-start justify-between gap-4 border-b border-[#edf0f5] px-5 py-4">
          <div className="min-w-0">
            <div
              id="host-register-title"
              className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-[#596174]"
            >
              <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
                <Server size={15} />
              </span>
              Register Host
            </div>
            <div className="mt-2 text-sm text-[#667085]">
              Create a daemon launch profile for the active space.
            </div>
          </div>
          <button
            type="button"
            className="composer-icon h-8 min-w-8"
            title="Close"
            onClick={onClose}
            disabled={creating}
          >
            <X size={15} />
          </button>
        </div>

        <div className="space-y-4 p-5">
          <label className="block">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              Display Name
            </span>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="mt-2"
              placeholder="Local Host"
              autoFocus
            />
          </label>
          <label className="block">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              Data Root
            </span>
            <Input
              value={dataRoot}
              onChange={(event) => setDataRoot(event.target.value)}
              className="mt-2 font-mono text-xs"
              placeholder="Use Loom default"
            />
            <span className="mt-2 block text-xs leading-5 text-[#667085]">
              Leave empty unless this host should store agent profiles under a specific path.
            </span>
          </label>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-3 border-t border-[#edf0f5] px-5 py-4">
          <Button
            type="button"
            variant="outline"
            onClick={onClose}
            disabled={creating}
            className="rounded-lg border-[#dfe3ec] bg-white"
          >
            Cancel
          </Button>
          <Button type="submit" disabled={!canSubmit} className="rounded-lg">
            {creating ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            Prepare Host
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}

export function MachineCard({
  machine,
  busy,
  onRemove,
  onOpenLocalPath,
}: {
  machine: MachineInfo;
  busy: string | null;
  onRemove: (machineId: string) => void;
  onOpenLocalPath: (path: string) => void;
}) {
  const canRemoveMachine = machine.capabilities.includes("machine.remove");
  const isLocalRegistration = machine.source === "local_registration";

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="flex min-w-0 items-start gap-4">
            <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f83f7] to-[#4e3ad5] text-white shadow-sm">
              <HardDrive size={25} />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold text-[#111827]">
                {machine.name}
              </h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
                <span
                  className={cn(
                    "h-2 w-2 rounded-full",
                    statusDotClass(machine.connectionStatus),
                  )}
                />
                <span>{capitalize(machine.connectionStatus)}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{machine.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant="secondary">{machine.setupStatus}</Badge>
                <Badge variant="outline">{machine.kind}</Badge>
                {machine.readOnly && <Badge variant="warning">read only</Badge>}
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-start gap-6">
            <HostMetric label="Runtimes" value={machine.providers.length} />
            <HostMetric label="Agents" value={machine.agentCount} />
            <HostMetric label="Online" value={machine.onlineAgentCount} />
            {machine.canOpenLocalPath && (
              <Button
                variant="outline"
                size="sm"
                title="Open data root"
                onClick={() => onOpenLocalPath(machine.dataRoot)}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                <HardDrive size={15} />
                Open Data
              </Button>
            )}
          </div>
        </div>
      </section>

      <HostDetailSection title="Name">
        <div className="text-sm font-semibold text-[#111827]">{machine.name}</div>
      </HostDetailSection>

      <HostDetailSection title="Info">
        <div className="divide-y divide-[#edf0f5]">
          <HostInfoRow label="Source">
            {machine.source || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Data Root" mono>
            {machine.dataRoot || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Config Dir" mono>
            {machine.configDir || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Connection Actor" mono>
            {machine.connectionActorId || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Serve Command" mono>
            {machine.serveCommand || "Not set"}
          </HostInfoRow>
          <HostInfoRow label="Detected Runtimes">
            <div className="flex flex-wrap gap-2">
              {machine.providers.length === 0 ? (
                <Badge variant="warning">no runtimes detected</Badge>
              ) : (
                machine.providers.map((provider) => (
                  <ProviderBadge key={provider.id} provider={provider} />
                ))
              )}
            </div>
          </HostInfoRow>
          <HostInfoRow label="Inventory">
            Revision {machine.inventoryRevision}
            {machine.inventoryObservedAt
              ? ` · observed ${formatTime(machine.inventoryObservedAt)}`
              : ""}
          </HostInfoRow>
        </div>
      </HostDetailSection>

      <HostDetailSection title="Actions">
        {isLocalRegistration ? (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#111827]">Start Host</div>
                <div className="mt-1 text-sm text-[#667085]">
                  Run the serve command above, then refresh hosts after the daemon connects.
                </div>
              </div>
              <Badge variant="warning">pending daemon</Badge>
            </div>
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#991b1b]">Discard Registration</div>
                <div className="mt-1 text-sm text-[#b91c1c]">
                  Remove this pending host registration. This will delete the local config and data directories.
                </div>
              </div>
              <Button
                variant="destructive"
                size="sm"
                title="Discard pending registration"
                onClick={() => onRemove(machine.id)}
                disabled={busy === `machine:remove:${machine.id}`}
                className="rounded-lg"
              >
                <Trash2 size={15} />
                Discard
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
            <div className="min-w-0">
              <div className="text-sm font-bold text-[#111827]">Delete Host</div>
              <div className="mt-1 text-sm text-[#667085]">
                Permanently remove this host after its agents are deleted.
              </div>
            </div>
            {canRemoveMachine ? (
              <Button
                variant="destructive"
                size="sm"
                title="Remove host"
                onClick={() => onRemove(machine.id)}
                disabled={busy === `machine:remove:${machine.id}`}
                className="rounded-lg"
              >
                <Trash2 size={15} />
                Delete Host
              </Button>
            ) : (
              <Badge variant="warning">managed by server</Badge>
            )}
          </div>
        )}
      </HostDetailSection>
    </div>
  );
}
