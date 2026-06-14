import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { createPortal } from "react-dom";
import { AgentProviderIcon, agentProviderIconKey } from "@/components/agent/AgentProviderIcon";
import { StyledSelect } from "@/components/shared/UIComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { defaultProviderManifestText } from "@/lib/constants";
import { errorText } from "@/lib/format-utils";
import type { ProviderAvailabilityGroup } from "@/lib/types";
import type { MachineInfo } from "@/ipc/types";
import * as ipc from "@/ipc/bridge";
import { Bot, Check, Loader2, X } from "lucide-react";

export function ProviderAvailabilityRow({ group }: { group: ProviderAvailabilityGroup }) {
  const iconKey = agentProviderIconKey(group.id, group.name);
  const hostNames = group.hosts.map(({ machine }) => machine.name);
  const modelLabel =
    group.defaultModels.length === 0
      ? "default model"
      : group.defaultModels.length === 1
        ? group.defaultModels[0]
        : `${group.defaultModels.length} model defaults`;

  return (
    <div className="grid min-h-[58px] grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg border border-[#edf0f5] bg-white px-3 py-2.5">
      <div className="flex min-w-0 items-center gap-2.5">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[#edf0f5] bg-white text-[#503ed4]">
          {iconKey ? (
            <AgentProviderIcon iconKey={iconKey} className="h-4 w-4" />
          ) : (
            <Bot size={15} />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-bold text-[#111827]">
            {group.name}
          </div>
          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-[#667085]">
            <span className="min-w-0 truncate">{hostNames.join(", ")}</span>
            <span className="shrink-0 text-[#a0a6b3]">/</span>
            <span className="min-w-0 truncate font-mono">
              {modelLabel}
            </span>
          </div>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <Badge variant="outline">{group.hosts.length} hosts</Badge>
        {group.actorCount > 0 && (
          <Badge variant="secondary">{group.actorCount} agents</Badge>
        )}
      </div>
    </div>
  );
}

export function ProviderAddDialog({
  machines,
  preferredMachineId,
  onCheckMachines,
  onClose,
}: {
  machines: MachineInfo[];
  preferredMachineId: string | null;
  onCheckMachines: () => void;
  onClose: () => void;
}) {
  const writableMachines = machines.filter(
    (machine) =>
      machine.canCommand &&
      !machine.readOnly &&
      machine.capabilities.includes("provider.add"),
  );
  const initialMachine =
    writableMachines.find((machine) => machine.id === preferredMachineId) ??
    writableMachines[0] ??
    machines.find((machine) => machine.id === preferredMachineId) ??
    machines[0];
  const [machineId, setMachineId] = useState(initialMachine?.id ?? "");
  const [manifestText, setManifestText] = useState(defaultProviderManifestText);
  const [replace, setReplace] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const selectedMachine = machines.find((machine) => machine.id === machineId);
  const canSubmit =
    Boolean(selectedMachine?.capabilities.includes("provider.add")) &&
    Boolean(selectedMachine?.canCommand) &&
    !selectedMachine?.readOnly &&
    manifestText.trim().length > 0;

  useEffect(() => {
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  async function submitProvider(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedMachine || !canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      const parsed = JSON.parse(manifestText) as unknown;
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("manifest must be a JSON object");
      }
      await ipc.providerAdd({
        machineId: selectedMachine.id,
        manifest: parsed as Record<string, unknown>,
        replace,
      });
      onCheckMachines();
      onClose();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="provider-add-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <form
        className="flex max-h-[min(760px,calc(100vh-48px))] w-full max-w-3xl flex-col rounded-2xl border border-[#dfe3ec] bg-white shadow-[0_28px_80px_rgb(16_24_40_/_0.22)]"
        onSubmit={submitProvider}
      >
        <div className="flex items-start justify-between gap-4 border-b border-[#edf0f5] px-5 py-4">
          <div className="min-w-0">
            <div
              id="provider-add-title"
              className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-[#596174]"
            >
              <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
                <Bot size={15} />
              </span>
              Add Provider
            </div>
            <div className="mt-2 text-sm text-[#667085]">
              {selectedMachine?.name ?? "No host selected"}
            </div>
          </div>
          <button
            type="button"
            className="composer-icon h-8 min-w-8"
            title="Close"
            onClick={onClose}
          >
            <X size={15} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
          <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
            <StyledSelect
              value={machineId}
              onChange={(event) => setMachineId(event.target.value)}
            >
              {machines.length === 0 ? (
                <option value="">No registered hosts</option>
              ) : (
                machines.map((machine) => (
                  <option key={machine.id} value={machine.id}>
                    {machine.name}
                  </option>
                ))
              )}
            </StyledSelect>
            <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
              <input
                type="checkbox"
                checked={replace}
                onChange={(event) => setReplace(event.target.checked)}
              />
              Replace
            </label>
          </div>
          <Textarea
            value={manifestText}
            onChange={(event) => setManifestText(event.target.value)}
            className="mt-3 min-h-[360px] rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
            spellCheck={false}
          />
          {error && (
            <div className="mt-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {error}
            </div>
          )}
          {selectedMachine && !canSubmit && (
            <div className="mt-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm font-medium text-amber-800">
              This host cannot write provider manifests.
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[#edf0f5] px-5 py-4">
          <div className="text-xs font-medium text-[#667085]">
            {selectedMachine?.configDir || "No config directory"}
          </div>
          <Button
            type="submit"
            disabled={saving || !canSubmit}
            className="rounded-lg"
          >
            {saving ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            Add Provider
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
