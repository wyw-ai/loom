import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { createPortal } from "react-dom";
import { HostDetailSection, HostInfoRow, ProviderBadge } from "@/components/shared/UIComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { capitalize, statusDotClass } from "@/lib/format-utils";
import { useI18n } from "@/lib/i18n";
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
  const { t } = useI18n();
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
          {t("{{providers}} runtimes · {{online}}/{{agents}} agents online", {
            providers: machine.providers.length,
            online: machine.onlineAgentCount,
            agents: machine.agentCount,
          })}
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
  const { t } = useI18n();
  return (
    <div className="flex min-h-full items-center justify-center bg-white px-6 py-10">
      <div className="w-full max-w-xl rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-6 text-center">
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-[#f1efff] text-[#503ed4]">
          <Server size={22} />
        </div>
        <h2 className="mt-4 text-lg font-bold text-[#111827]">{t("Register a Host")}</h2>
        <p className="mt-2 text-sm leading-6 text-[#667085]">
          {t("Prepare a daemon registration for this space, then start the generated command so the host can publish its runtime inventory.")}
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
          {t("Register Host")}
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
  const { t } = useI18n();
  const [name, setName] = useState(() => t("Local Host"));
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
              {t("Register Host")}
            </div>
            <div className="mt-2 text-sm text-[#667085]">
              {t("Create a daemon launch profile for the active space.")}
            </div>
          </div>
          <button
            type="button"
            className="composer-icon h-8 min-w-8"
            title={t("Close")}
            onClick={onClose}
            disabled={creating}
          >
            <X size={15} />
          </button>
        </div>

        <div className="space-y-4 p-5">
          <label className="block">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              {t("Display Name")}
            </span>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="mt-2"
              placeholder={t("Local Host")}
              autoFocus
            />
          </label>
          <label className="block">
            <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
              {t("Data Root")}
            </span>
            <Input
              value={dataRoot}
              onChange={(event) => setDataRoot(event.target.value)}
              className="mt-2 font-mono text-xs"
              placeholder={t("Use Loom default")}
            />
            <span className="mt-2 block text-xs leading-5 text-[#667085]">
              {t("Leave empty unless this host should store agent profiles under a specific path.")}
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
            {t("Cancel")}
          </Button>
          <Button type="submit" disabled={!canSubmit} className="rounded-lg">
            {creating ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            {t("Prepare Host")}
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
  const { t } = useI18n();
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
                <span>{t(capitalize(machine.connectionStatus))}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{machine.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant="secondary">{t(machine.setupStatus)}</Badge>
                <Badge variant="outline">{t(machine.kind)}</Badge>
                {machine.readOnly && <Badge variant="warning">{t("read only")}</Badge>}
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-start gap-6">
            <HostMetric label={t("Runtimes")} value={machine.providers.length} />
            <HostMetric label={t("Agents")} value={machine.agentCount} />
            <HostMetric label={t("Online")} value={machine.onlineAgentCount} />
            {machine.canOpenLocalPath && (
              <Button
                variant="outline"
                size="sm"
                title={t("Open data root")}
                onClick={() => onOpenLocalPath(machine.dataRoot)}
                className="rounded-lg border-[#dfe3ec] bg-white"
              >
                <HardDrive size={15} />
                {t("Open Data")}
              </Button>
            )}
          </div>
        </div>
      </section>

      <HostDetailSection title={t("Name")}>
        <div className="text-sm font-semibold text-[#111827]">{machine.name}</div>
      </HostDetailSection>

      <HostDetailSection title={t("Info")}>
        <div className="divide-y divide-[#edf0f5]">
          <HostInfoRow label={t("Source")}>
            {machine.source ? t(machine.source) : t("Not set")}
          </HostInfoRow>
          <HostInfoRow label={t("Data Root")} mono>
            {machine.dataRoot || t("Not set")}
          </HostInfoRow>
          <HostInfoRow label={t("Config Dir")} mono>
            {machine.configDir || t("Not set")}
          </HostInfoRow>
          <HostInfoRow label={t("Connection Actor")} mono>
            {machine.connectionActorId || t("Not set")}
          </HostInfoRow>
          <HostInfoRow label={t("Serve Command")} mono>
            {machine.serveCommand || t("Not set")}
          </HostInfoRow>
          <HostInfoRow label={t("Detected Runtimes")}>
            <div className="flex flex-wrap gap-2">
              {machine.providers.length === 0 ? (
                <Badge variant="warning">{t("no runtimes detected")}</Badge>
              ) : (
                machine.providers.map((provider) => (
                  <ProviderBadge key={provider.id} provider={provider} />
                ))
              )}
            </div>
          </HostInfoRow>
          <HostInfoRow label={t("Inventory")}>
            {t("Revision {{revision}}", { revision: machine.inventoryRevision })}
            {machine.inventoryObservedAt
              ? ` · ${formatTime(machine.inventoryObservedAt)}`
              : ""}
          </HostInfoRow>
        </div>
      </HostDetailSection>

      <HostDetailSection title={t("Actions")}>
        {isLocalRegistration ? (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#111827]">{t("Start Host")}</div>
                <div className="mt-1 text-sm text-[#667085]">
                  {t("Run the serve command above, then refresh hosts after the daemon connects.")}
                </div>
              </div>
              <Badge variant="warning">{t("pending daemon")}</Badge>
            </div>
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#991b1b]">{t("Discard Registration")}</div>
                <div className="mt-1 text-sm text-[#b91c1c]">
                  {t("Remove this pending host registration. This will delete the local config and data directories.")}
                </div>
              </div>
              <Button
                variant="destructive"
                size="sm"
                title={t("Discard pending registration")}
                onClick={() => onRemove(machine.id)}
                disabled={busy === `machine:remove:${machine.id}`}
                className="rounded-lg"
              >
                <Trash2 size={15} />
                {t("Discard")}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
            <div className="min-w-0">
              <div className="text-sm font-bold text-[#111827]">{t("Delete Host")}</div>
              <div className="mt-1 text-sm text-[#667085]">
                {t("Permanently remove this host after its agents are deleted.")}
              </div>
            </div>
            {canRemoveMachine ? (
              <Button
                variant="destructive"
                size="sm"
                title={t("Remove host")}
                onClick={() => onRemove(machine.id)}
                disabled={busy === `machine:remove:${machine.id}`}
                className="rounded-lg"
              >
                <Trash2 size={15} />
                {t("Delete Host")}
              </Button>
            ) : (
              <Badge variant="warning">{t("managed by server")}</Badge>
            )}
          </div>
        )}
      </HostDetailSection>
    </div>
  );
}
