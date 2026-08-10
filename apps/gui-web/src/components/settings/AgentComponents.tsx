import {
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import type { ComponentType, FormEvent } from "react";
import { createPortal } from "react-dom";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { AgentProviderIcon, agentProviderIconKey } from "@/components/agent/AgentProviderIcon";
import { HostDetailSection, HostInfoRow, StyledSelect } from "@/components/shared/UIComponents";
import { ProviderAvailabilityRow } from "@/components/settings/ProviderComponents";
import { AgentPromptStudio } from "@/components/views/AgentPromptStudio";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { machineDirList } from "@/ipc/bridge";
import { actorAvatarUrl, agentDisplayName, agentModelValue, agentSettingsDraft, getActorRunContext, providerAvailabilityGroups, providerForAgent, runStatusAnimationName, runStatusDotClass, runStatusFullLabel } from "@/lib/agent-utils";
import { avatarLibraryUrls, reasoningEffortChoices } from "@/lib/constants";
import { agentFormForMachine, capitalize, errorText, machineCanCreateAgent, machineCanRunCommands, resolveAgentProvider, statusDotClass } from "@/lib/format-utils";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { applyWakePreset, wakePresetIdFor, WAKE_PRESETS, type WakePresetId } from "@/lib/wake-utils";
import { ArrowLeft, Bot, Check, ChevronLeft, FileText, Folder, HardDrive, Loader2, Plus, RefreshCw, Settings, Trash2, Wrench, X } from "lucide-react";
import type { MachineAgentProviderInfo, MachineDirListResult, MachineInfo, Run } from "@/ipc/types";
import type { AgentDetailTab, AgentFormState, AgentMemberEntry, AgentSettingsDraft, AgentUpdatePatch } from "@/lib/types";

const customModelOptionValue = "__loom_custom_model__";
export function MemberListItem({
  entry,
  selected,
  runs,
  onSelect,
}: {
  entry: AgentMemberEntry;
  selected: boolean;
  runs: Record<string, Run>;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  const actor = entry.agent.spec.actor;
  const ctx = getActorRunContext(runs, actor.id);
  const working = ctx != null && !ctx.isTerminal;
  const label = working ? t(runStatusFullLabel(ctx) ?? "Processing…") : entry.machine.name;
  const animationName = runStatusAnimationName(ctx);

  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-colors",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
          : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
      )}
      onClick={onSelect}
    >
      <ActorAvatar actor={actor} fallback={actor.id} small />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-bold text-[#111827]">
            {agentDisplayName(entry.agent)}
          </span>
          <span
            className={cn(
              "h-2 w-2 shrink-0 rounded-full",
              working || ctx?.isTerminal
                ? runStatusDotClass(ctx)
                : statusDotClass(entry.agent.status),
            )}
            style={animationName ? { animation: `${animationName} 1.5s ease-in-out infinite` } : undefined}
          />
        </span>
        <span className="mt-0.5 block truncate text-xs text-[#667085]">
          {label}
        </span>
      </span>
    </button>
  );
}


export function AgentRosterOverview({
  machines,
  entries,
  busy,
  onOpenCreateAgent,
  onOpenProviderAdd,
  onSelectAgent,
  onRemoveAgent,
}: {
  machines: MachineInfo[];
  entries: AgentMemberEntry[];
  busy: string | null;
  onOpenCreateAgent: (machine?: MachineInfo | null) => void;
  onOpenProviderAdd: () => void;
  onSelectAgent: (entry: AgentMemberEntry) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const { t } = useI18n();
  const providerGroups = providerAvailabilityGroups(machines);
  const hostRows = machines.map((machine) => ({
    machine,
    canCreate: machineCanCreateAgent(machine) && machine.providers.length > 0,
  }));
  const canCreateAgent = hostRows.some((row) => row.canCreate);
  const onlineAgents = entries.filter((entry) => entry.agent.status === "online").length;

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="min-w-0">
            <h2 className="text-xl font-bold text-[#111827]">{t("Agents")}</h2>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
              <span>{t("{{count}} registered", { count: entries.length })}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{t("{{count}} online", { count: onlineAgents })}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{t("{{count}} provider types", { count: providerGroups.length })}</span>
            </div>
          </div>
          <Button
            onClick={() => onOpenCreateAgent()}
            disabled={!canCreateAgent}
            className="rounded-lg"
          >
            <Plus size={15} />
            {t("Create Agent")}
          </Button>
        </div>
      </section>

      <HostDetailSection title={t("Agent Roster")} count={entries.length}>
        <div className="space-y-2">
          {entries.length === 0 ? (
            <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
              <div>{t("No agents registered.")}</div>
              <Button
                size="sm"
                className="mt-3 rounded-lg"
                onClick={() => onOpenCreateAgent()}
                disabled={!canCreateAgent}
              >
                <Plus size={14} />
                {t("Create Agent")}
              </Button>
            </div>
          ) : (
            entries.map((entry) => (
              <AgentRosterRow
                key={`${entry.machine.id}:${entry.agent.spec.actor.id}`}
                entry={entry}
                selected={false}
                busy={busy}
                onSelect={() => onSelectAgent(entry)}
                onRemoveAgent={onRemoveAgent}
              />
            ))
          )}
        </div>
      </HostDetailSection>

      <HostDetailSection
        title={t("Create Readiness")}
        action={
          <Button
            variant="outline"
            size="sm"
            onClick={onOpenProviderAdd}
            disabled={machines.length === 0}
            className="h-8 rounded-lg border-[#dfe3ec] bg-white"
          >
            <Plus size={14} />
            {t("Add Provider")}
          </Button>
        }
      >
        <div className="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
          <div className="min-w-0 rounded-xl border border-[#edf0f5] bg-white p-3">
            <div className="mb-3 flex items-center gap-2">
              <div className="flex items-center gap-2">
                <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                  {t("Hosts")}
                </div>
                <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
                  {hostRows.length}
                </span>
              </div>
            </div>
            <div className="grid gap-2">
              {hostRows.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                  {t("No managed hosts.")}
                </div>
              ) : (
                hostRows.map(({ machine, canCreate }) => {
                  const reason = machine.connectionStatus !== "online"
                    ? "offline"
                    : !machineCanRunCommands(machine)
                      ? "unavailable"
                    : machine.providers.length === 0
                      ? "no runtime"
                      : "ready";
                  return (
                    <button
                      key={machine.id}
                      type="button"
                      className="min-w-0 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] px-3 py-3 text-left transition-colors hover:border-[#c8c1ff] hover:bg-white disabled:cursor-not-allowed disabled:opacity-60"
                      onClick={() => onOpenCreateAgent(machine)}
                      disabled={!canCreate}
                    >
                      <div className="flex items-center gap-2">
                        <span
                          className={cn(
                            "h-2 w-2 shrink-0 rounded-full",
                            statusDotClass(machine.connectionStatus),
                          )}
                        />
                        <span className="min-w-0 flex-1 truncate text-sm font-bold text-[#111827]">
                          {machine.name}
                        </span>
                        <Badge variant={canCreate ? "outline" : "warning"}>{t(reason)}</Badge>
                      </div>
                      <div className="mt-2 truncate text-xs text-[#667085]">
                        {t("{{providers}} providers / {{online}}/{{agents}} online", {
                          providers: machine.providers.length,
                          online: machine.onlineAgentCount,
                          agents: machine.agentCount,
                        })}
                      </div>
                    </button>
                  );
                })
              )}
            </div>
          </div>

          <div className="min-w-0 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-3">
            <div className="mb-3 flex items-center gap-2">
              <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                {t("Provider Availability")}
              </div>
              <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
                {providerGroups.length}
              </span>
            </div>
            <div className="max-h-60 space-y-2 overflow-y-auto soft-scrollbar">
              {providerGroups.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[#dfe3ec] bg-white p-3 text-sm text-[#667085]">
                  {t("No providers detected.")}
                </div>
              ) : (
                providerGroups.map((group) => (
                  <ProviderAvailabilityRow key={group.key} group={group} />
                ))
              )}
            </div>
          </div>
        </div>
      </HostDetailSection>
    </div>
  );
}


export function AgentRosterRow({
  entry,
  selected,
  busy,
  onSelect,
  onRemoveAgent,
}: {
  entry: AgentMemberEntry;
  selected: boolean;
  busy: string | null;
  onSelect: () => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const { t } = useI18n();
  const actor = entry.agent.spec.actor;
  const provider = providerForAgent(entry.machine, entry.agent);

  return (
    <div
      className={cn(
        "grid min-h-[66px] grid-cols-[minmax(0,1fr)_auto] items-center gap-4 rounded-xl border px-4 py-3",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff]"
          : "border-[#edf0f5] bg-[#fbfbfd]",
      )}
    >
      <button
        type="button"
        className="flex min-w-0 items-center gap-3 text-left"
        onClick={onSelect}
      >
        <ActorAvatar actor={actor} fallback={actor.id} small />
        <span className="min-w-0">
          <span className="block truncate text-sm font-bold text-[#111827]">
            {agentDisplayName(entry.agent)}
          </span>
          <span className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-[#667085]">
            <span>{entry.machine.name}</span>
            <span className="text-[#a0a6b3]">/</span>
            <span>{provider?.name ?? entry.agent.spec.providerRef.id}</span>
            <span className="text-[#a0a6b3]">/</span>
            <span className="font-mono">{agentModelValue(entry.agent) || t("default")}</span>
          </span>
        </span>
      </button>
      <div className="flex items-center gap-2">
        <Badge variant={entry.agent.status === "online" ? "success" : "outline"}>
          {t(entry.agent.status)}
        </Badge>
        {machineCanRunCommands(entry.machine) && (
          <Button
            variant="ghost"
            size="icon"
            title={t("Remove agent")}
            onClick={() => onRemoveAgent(entry.machine.id, actor.id)}
            disabled={busy === `agent:remove:${actor.id}`}
            className="rounded-lg"
          >
            <Trash2 size={15} />
          </Button>
        )}
      </div>
    </div>
  );
}


export function AgentCreateDialog({
  agentForm,
  busy,
  machine,
  machines,
  setAgentForm,
  onSelectMachine,
  onAddAgent,
  onClose,
}: {
  agentForm: AgentFormState;
  busy: string | null;
  machine: MachineInfo;
  machines: MachineInfo[];
  setAgentForm: (form: AgentFormState) => void;
  onSelectMachine: (machineId: string) => void;
  onAddAgent: (form: AgentFormState) => Promise<boolean> | boolean;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<AgentFormState>(() =>
    agentFormForMachine(agentForm, machine),
  );
  const selectedProvider = resolveAgentProvider(draft, machine);
  const modelChoices = selectedProvider?.modelChoices ?? [];
  const [customModelActive, setCustomModelActive] = useState(false);
  const canCreateAgent = machineCanCreateAgent(machine);
  const creating = busy === "agent:create";
  const agentReady = Boolean(
    canCreateAgent && selectedProvider && draft.name.trim(),
  );
  const modelValue = draft.model || selectedProvider?.defaultModel || "";
  const modelIsKnown =
    !modelValue || modelChoices.some((choice) => choice.id === modelValue);
  const showCustomModel =
    modelChoices.length === 0 || customModelActive || !modelIsKnown;
  const modelSelectValue = showCustomModel ? customModelOptionValue : modelValue;
  const writableHosts = machines.filter(machineCanCreateAgent);
  const readyHosts = machines.filter(
    (item) => machineCanCreateAgent(item) && item.providers.length > 0,
  );
  const createStatusText = machine.connectionStatus !== "online"
    ? t("Start the host daemon before creating agents.")
    : !machineCanRunCommands(machine)
      ? t("This host cannot run agent commands for the current account.")
    : !selectedProvider
      ? t("No runtime detected for this host.")
      : t("{{provider}} on {{host}}", { provider: selectedProvider.name, host: machine.name });
  const createStatusBadge = machine.connectionStatus !== "online"
    ? "offline"
    : !machineCanRunCommands(machine)
      ? "unavailable"
    : selectedProvider
      ? "ready"
      : "no runtime";

  useEffect(() => {
    setDraft((current) => agentFormForMachine(current, machine));
    setCustomModelActive(false);
  }, [machine.id]);

  useEffect(() => {
    setAgentForm(draft);
  }, [draft, setAgentForm]);

  useEffect(() => {
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  function updateAgentForm(patch: Partial<AgentFormState>) {
    setDraft((current) => ({
      ...current,
      machineId: machine.id,
      providerId: selectedProvider?.id ?? current.providerId,
      ...patch,
    }));
  }

  function selectProvider(provider: MachineAgentProviderInfo) {
    setCustomModelActive(false);
    setDraft((current) => ({
      ...current,
      machineId: machine.id,
      providerId: provider.id,
      model: provider.defaultModel || provider.modelChoices[0]?.id || "",
    }));
  }

  function selectMachine(machineId: string) {
    const nextMachine = machines.find((item) => item.id === machineId);
    if (!nextMachine) return;
    setDraft((current) => agentFormForMachine(current, nextMachine));
    setCustomModelActive(false);
    onSelectMachine(nextMachine.id);
  }

  function addEnvEntry() {
    setDraft((current) => ({
      ...current,
      env: { ...current.env, "": "" },
    }));
  }

  function updateEnvEntry(index: number, key: string, value: string) {
    const entries = Object.entries(draft.env);
    entries[index] = [key, value];
    setDraft((current) => ({
      ...current,
      env: Object.fromEntries(entries),
    }));
  }

  function removeEnvEntry(index: number) {
    const entries = Object.entries(draft.env);
    entries.splice(index, 1);
    setDraft((current) => ({
      ...current,
      env: Object.fromEntries(entries),
    }));
  }

  const envEntries = Object.entries(draft.env);

  async function submitAgent(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const created = await onAddAgent(draft);
    if (created) onClose();
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="agent-create-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <form
        className="flex max-h-[min(760px,calc(100vh-48px))] w-full max-w-3xl flex-col rounded-2xl border border-[#dfe3ec] bg-white shadow-[0_28px_80px_rgb(16_24_40_/_0.22)]"
        onSubmit={submitAgent}
      >
        <div className="flex items-start justify-between gap-4 border-b border-[#edf0f5] px-5 py-4">
          <div className="min-w-0">
            <div
              id="agent-create-title"
              className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-[#596174]"
            >
              <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
                <Bot size={15} />
              </span>
              {t("New Agent")}
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
              <span className="font-semibold text-[#303849]">{machine.name}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{createStatusText}</span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant={canCreateAgent && selectedProvider ? "outline" : "warning"}>
              {t(createStatusBadge)}
            </Badge>
            <button
              type="button"
              className="composer-icon h-8 min-w-8"
              title={t("Close")}
              onClick={onClose}
            >
              <X size={15} />
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
          <div className="space-y-5">
            <section>
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                {t("Host")}
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                {machines.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                    {t("Register a host before creating agents.")}
                  </div>
                ) : (
                  machines.map((item) => {
                    const selected = item.id === machine.id;
                    const ready = machineCanCreateAgent(item) && item.providers.length > 0;
                    const blockedReason = item.connectionStatus !== "online"
                      ? "Offline"
                      : !machineCanRunCommands(item)
                        ? "Unavailable"
                      : item.providers.length === 0
                        ? "No runtime"
                        : "";
                    return (
                      <button
                        key={item.id}
                        type="button"
                        className={cn(
                          "flex min-h-[62px] items-center gap-3 rounded-xl border bg-white px-3 py-3 text-left transition-colors",
                          selected
                            ? "border-[#8f82ff] bg-[#f7f5ff] ring-2 ring-[#ece8ff]"
                            : "border-[#e2e6ef] hover:border-[#c8c1ff]",
                        )}
                        onClick={() => selectMachine(item.id)}
                      >
                        <span
                          className={cn(
                            "h-2.5 w-2.5 shrink-0 rounded-full",
                            statusDotClass(item.connectionStatus),
                          )}
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-bold text-[#111827]">
                            {item.name}
                          </span>
                          <span className="mt-0.5 block truncate text-xs text-[#667085]">
                            {t("{{providers}} runtimes / {{online}}/{{agents}} online", {
                              providers: item.providers.length,
                              online: item.onlineAgentCount,
                              agents: item.agentCount,
                            })}
                          </span>
                        </span>
                        <Badge variant={ready ? "outline" : "warning"}>
                          {t(ready ? "ready" : blockedReason)}
                        </Badge>
                      </button>
                    );
                  })
                )}
              </div>
            </section>

            <section>
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                {t("Runtime")}
              </div>
              {machine.providers.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                  {t("No runtimes detected for this host.")}
                </div>
              ) : (
                <div className="grid gap-2 sm:grid-cols-2">
                  {machine.providers.map((provider) => {
                    const selected = selectedProvider?.id === provider.id;
                    const iconKey = agentProviderIconKey(provider.id, provider.name);
                    return (
                      <button
                        key={provider.id}
                        type="button"
                        disabled={!canCreateAgent}
                        className={cn(
                          "flex min-h-[68px] items-center gap-3 rounded-xl border bg-white px-3 py-3 text-left transition-colors",
                          selected
                            ? "border-[#8f82ff] bg-[#f7f5ff] ring-2 ring-[#ece8ff]"
                            : "border-[#e2e6ef] hover:border-[#c8c1ff]",
                        )}
                        onClick={() => selectProvider(provider)}
                      >
                        <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-[#edf0f5] bg-white text-[#503ed4]">
                          {iconKey ? (
                            <AgentProviderIcon iconKey={iconKey} className="h-5 w-5" />
                          ) : (
                            <Bot size={18} />
                          )}
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-bold text-[#111827]">
                            {provider.name}
                          </span>
                          <span className="mt-0.5 block truncate text-xs text-[#667085]">
                            {provider.defaultModel || t("{{count}} models", { count: provider.modelChoices.length })}
                          </span>
                        </span>
                        {selected && <Check size={16} className="shrink-0 text-[#503ed4]" />}
                      </button>
                    );
                  })}
                </div>
              )}
            </section>

            <section className="grid gap-3 md:grid-cols-2">
              <Input
                value={draft.name}
                onChange={(event) => updateAgentForm({ name: event.target.value })}
                placeholder={t("Agent name")}
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canCreateAgent}
              />
              <Input
                value={draft.actorId}
                onChange={(event) => updateAgentForm({ actorId: event.target.value })}
                placeholder={t("Actor id (optional)")}
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canCreateAgent}
              />
              <div className="space-y-2">
                {modelChoices.length > 0 ? (
                  <StyledSelect
                    value={modelSelectValue}
                    onChange={(event) => {
                      if (event.target.value === customModelOptionValue) {
                        setCustomModelActive(true);
                        updateAgentForm({
                          model: modelIsKnown ? "" : draft.model,
                        });
                        return;
                      }
                      setCustomModelActive(false);
                      updateAgentForm({ model: event.target.value });
                    }}
                    disabled={!canCreateAgent}
                  >
                    <option value="">{t("Default model")}</option>
                    {modelChoices.map((choice) => (
                      <option key={choice.id} value={choice.id}>
                        {choice.label || choice.id}
                      </option>
                    ))}
                    <option value={customModelOptionValue}>{t("Custom...")}</option>
                  </StyledSelect>
                ) : null}
                {showCustomModel && (
                  <Input
                    value={draft.model}
                    onChange={(event) => updateAgentForm({ model: event.target.value })}
                    placeholder={t("Custom model")}
                    className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                    disabled={!canCreateAgent}
                  />
                )}
              </div>
              <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
                <input
                  type="checkbox"
                  checked={draft.autostart}
                  onChange={(event) =>
                    updateAgentForm({ autostart: event.target.checked })
                  }
                  disabled={!canCreateAgent}
                />
                {t("Autostart")}
              </label>
              <Input
                value={draft.description}
                onChange={(event) => updateAgentForm({ description: event.target.value })}
                placeholder={t("Description")}
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                disabled={!canCreateAgent}
              />
              <Textarea
                value={draft.instructions}
                onChange={(event) => updateAgentForm({ instructions: event.target.value })}
                placeholder={t("Agent instructions")}
                className="min-h-28 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                disabled={!canCreateAgent}
              />
              <div className="space-y-2 md:col-span-2">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    {t("Environment Variables")}
                  </span>
                  <button
                    type="button"
                    onClick={addEnvEntry}
                    disabled={!canCreateAgent}
                    className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-[#dfe3ec] bg-white text-[#596174] hover:bg-[#f0f2f5] disabled:opacity-40"
                    title={t("Add environment variable")}
                  >
                    <Plus size={12} />
                  </button>
                </div>
                {envEntries.map(([key, value], index) => (
                  <div key={index} className="flex items-center gap-2">
                    <Input
                      value={key}
                      onChange={(event) => updateEnvEntry(index, event.target.value, value)}
                      placeholder={t("Key")}
                      className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm font-mono shadow-none"
                      disabled={!canCreateAgent}
                    />
                    <Input
                      value={value}
                      onChange={(event) => updateEnvEntry(index, key, event.target.value)}
                      placeholder={t("Value")}
                      className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      disabled={!canCreateAgent}
                    />
                    <button
                      type="button"
                      onClick={() => removeEnvEntry(index)}
                      disabled={!canCreateAgent}
                      className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500 disabled:opacity-40"
                      title={t("Remove")}
                    >
                      <X size={14} />
                    </button>
                  </div>
                ))}
                {envEntries.length === 0 && (
                  <div className="rounded-lg border border-dashed border-[#dfe3ec] px-3 py-2 text-center text-xs text-[#9aa1ae]">
                    {t("No environment variables. Click + to add one.")}
                  </div>
                )}
              </div>
            </section>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[#edf0f5] px-5 py-4">
          <div className="text-xs font-medium text-[#667085]">
            {t("{{ready}} ready hosts / {{writable}} writable", {
              ready: readyHosts.length,
              writable: writableHosts.length,
            })}
          </div>
          <Button
            type="submit"
            disabled={creating || !agentReady}
            className="rounded-lg"
          >
            {creating ? (
              <Loader2 className="animate-spin" size={15} />
            ) : (
              <Plus size={15} />
            )}
            {t("Create Agent")}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}


export function AgentMemberDetail({
  entry,
  busy,
  onBack,
  onUpdateAgent,
  onAddAgentSkill,
  onRemoveAgent,
}: {
  entry: AgentMemberEntry;
  busy: string | null;
  onBack: () => void;
  onUpdateAgent: (patch: AgentUpdatePatch) => void;
  onAddAgentSkill: (
    machineId: string,
    actorId: string,
    source: string,
  ) => Promise<boolean> | boolean;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const { t } = useI18n();
  const { machine, agent } = entry;
  const actor = agent.spec.actor;
  const agentDetailKey = `${machine.id}:${actor.id}`;
  const [draft, setDraft] = useState<AgentSettingsDraft>(() =>
    agentSettingsDraft(machine, agent),
  );
  const [activeTab, setActiveTab] = useState<AgentDetailTab>("profile");
  const [customModelActive, setCustomModelActive] = useState(false);
  const [skillAddDialogOpen, setSkillAddDialogOpen] = useState(false);
  const handledAgentDetailKeyRef = useRef(agentDetailKey);
  const selectedProvider = providerForAgent(machine, agent, draft.providerId);
  const modelChoices =
    selectedProvider?.modelChoices.length
      ? selectedProvider.modelChoices
      : agent.spec.models?.choices ?? [];
  const modelValue = draft.model;
  const modelIsKnown =
    !modelValue || modelChoices.some((choice) => choice.id === modelValue);
  const showCustomModel =
    modelChoices.length === 0 || customModelActive || !modelIsKnown;
  const modelSelectValue = showCustomModel ? customModelOptionValue : modelValue;
  const saving = busy === `agent:update:${actor.id}`;
  const skillAdding = busy === `agent:skill:add:${actor.id}`;
  const removing = busy === `agent:remove:${actor.id}`;
  const canEdit = machineCanRunCommands(machine);
  const detailTabs: Array<{
    id: AgentDetailTab;
    label: string;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "profile", label: t("Profile"), icon: Bot },
    { id: "prompt", label: t("Prompt Studio"), icon: FileText },
    { id: "skills", label: t("Skills"), icon: Wrench },
    { id: "settings", label: t("Settings"), icon: Settings },
  ];

  useEffect(() => {
    if (handledAgentDetailKeyRef.current === agentDetailKey) return;
    handledAgentDetailKeyRef.current = agentDetailKey;
    setDraft(agentSettingsDraft(machine, agent));
    setCustomModelActive(false);
    setSkillAddDialogOpen(false);
  }, [agentDetailKey, machine, agent]);

  useEffect(() => {
    setActiveTab("profile");
  }, [agentDetailKey]);

  useEffect(() => {
    if (activeTab !== "skills") setSkillAddDialogOpen(false);
  }, [activeTab]);

  function updateDraft(patch: Partial<AgentSettingsDraft>) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function updateWake(patch: Partial<AgentSettingsDraft["wake"]>) {
    setDraft((current) => ({
      ...current,
      wake: { ...current.wake, ...patch },
    }));
  }

  function addEnvEntry() {
    setDraft((current) => ({
      ...current,
      env: { ...current.env, "": "" },
    }));
  }

  function updateEnvEntry(index: number, key: string, value: string) {
    const entries = Object.entries(draft.env);
    entries[index] = [key, value];
    setDraft((current) => ({
      ...current,
      env: Object.fromEntries(entries),
    }));
  }

  function removeEnvEntry(index: number) {
    const entries = Object.entries(draft.env);
    entries.splice(index, 1);
    setDraft((current) => ({
      ...current,
      env: Object.fromEntries(entries),
    }));
  }

  const envEntries = Object.entries(draft.env);

  function agentPatchFromDraft(nextDraft: AgentSettingsDraft): AgentUpdatePatch {
    return {
      machineId: machine.id,
      actorId: actor.id,
      displayName: nextDraft.displayName,
      description: nextDraft.description,
      instructions: nextDraft.instructions,
      providerId: nextDraft.providerId || undefined,
      model: nextDraft.model,
      reasoningEffort: nextDraft.reasoningEffort,
      autostart: nextDraft.autostart,
      avatarUrl: nextDraft.avatarUrl,
      env: nextDraft.env,
      bundleSkills: nextDraft.bundleSkills,
      wake: nextDraft.wake,
    };
  }

  function persistAgent(nextDraft: AgentSettingsDraft) {
    setDraft(nextDraft);
    onUpdateAgent(agentPatchFromDraft(nextDraft));
  }

  function saveAgent() {
    onUpdateAgent(agentPatchFromDraft(draft));
  }

  async function addSkill(source: string) {
    if (!canEdit) return false;
    const ok = await onAddAgentSkill(machine.id, actor.id, source);
    if (ok) setSkillAddDialogOpen(false);
    return ok;
  }

  function removeSkill(skillId: string) {
    if (!canEdit) return;
    persistAgent({
      ...draft,
      bundleSkills: draft.bundleSkills.filter((skill) => skill.id !== skillId),
    });
  }

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          className="mb-4 rounded-lg px-2 text-[#596174] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
        >
          <ArrowLeft size={15} />
          {t("All Agents")}
        </Button>
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="flex min-w-0 items-start gap-4">
            <img
              alt=""
              src={draft.avatarUrl || actorAvatarUrl(actor, actor.id)}
              className="h-16 w-16 shrink-0 rounded-xl border border-white object-cover shadow-sm"
            />
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold text-[#111827]">
                {draft.displayName || agentDisplayName(agent)}
              </h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
                <span className={cn("h-2 w-2 rounded-full", statusDotClass(agent.status))} />
                <span>{t(capitalize(agent.status))}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{actor.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant={agent.status === "online" ? "success" : "outline"}>
                  {t(agent.status)}
                </Badge>
                <Badge variant="secondary">{machine.name}</Badge>
                {machine.connectionStatus !== "online" ? (
                  <Badge variant="warning">{t("host offline")}</Badge>
                ) : machine.readOnly ? (
                  <Badge variant="warning">{t("read only")}</Badge>
                ) : null}
              </div>
            </div>
          </div>
          <Button
            onClick={saveAgent}
            disabled={!canEdit || saving || !draft.displayName.trim()}
            className="rounded-lg"
          >
            {saving ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            {t("Save Changes")}
          </Button>
        </div>
      </section>

      <div className="border-b border-[#dfe3ec] bg-[#fbfbfd] px-6 pt-4 lg:px-8">
        <div className="flex flex-wrap gap-2">
          {detailTabs.map((tab) => {
            const Icon = tab.icon;
            const selected = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                className={cn(
                  "flex h-10 items-center gap-2 rounded-t-lg border border-b-0 px-3 text-sm font-semibold transition-colors",
                  selected
                    ? "border-[#dfe3ec] bg-white text-[#503ed4]"
                    : "border-transparent text-[#596174] hover:border-[#dfe3ec] hover:bg-white",
                )}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={15} />
                {tab.label}
              </button>
            );
          })}
        </div>
      </div>

      {activeTab === "profile" && (
        <>
          <HostDetailSection title={t("Profile")}>
            <div className="grid gap-5 xl:grid-cols-[minmax(260px,0.42fr)_minmax(0,1fr)]">
              <div className="min-w-0">
                <div className="mb-3 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                  {t("Avatar Library")}
                </div>
                <div className="grid max-h-64 grid-cols-[repeat(auto-fill,minmax(38px,1fr))] gap-2 overflow-y-auto rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-3 soft-scrollbar">
                  {avatarLibraryUrls.map((url) => (
                    <button
                      key={url}
                      type="button"
                      title={url.split("/").pop() ?? t("Avatar")}
                      disabled={!canEdit}
                      className={cn(
                        "flex aspect-square items-center justify-center rounded-lg border bg-white p-1 transition-colors",
                        draft.avatarUrl === url
                          ? "border-[#8f82ff] ring-2 ring-[#e4e0ff]"
                          : "border-[#edf0f5] hover:border-[#c8c1ff]",
                      )}
                      onClick={() => updateDraft({ avatarUrl: url })}
                    >
                      <img alt="" src={url} className="h-full w-full rounded-md object-cover" />
                    </button>
                  ))}
                </div>
              </div>

              <div className="grid content-start gap-3 md:grid-cols-2">
                <Input
                  value={draft.displayName}
                  onChange={(event) => updateDraft({ displayName: event.target.value })}
                  placeholder={t("Display name")}
                  className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                  disabled={!canEdit}
                />
                <Input
                  value={actor.id}
                  readOnly
                  className="h-10 rounded-lg border-[#dfe3ec] bg-[#fbfbfd] font-mono text-xs shadow-none"
                />
                <Textarea
                  value={draft.description}
                  onChange={(event) => updateDraft({ description: event.target.value })}
                  placeholder={t("Description")}
                  className="min-h-20 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                  disabled={!canEdit}
                />
                <Textarea
                  value={draft.instructions}
                  onChange={(event) => updateDraft({ instructions: event.target.value })}
                  placeholder={t("Agent instructions")}
                  className="min-h-32 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                  disabled={!canEdit}
                />
              </div>
            </div>
          </HostDetailSection>

          <HostDetailSection title={t("Runtime Configuration")}>
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
              <StyledSelect
                value={selectedProvider?.id ?? draft.providerId}
                onChange={(event) => {
                  const provider = machine.providers.find(
                    (item) => item.id === event.target.value,
                  );
                  setCustomModelActive(false);
                  updateDraft({
                    providerId: event.target.value,
                    model: provider?.defaultModel || provider?.modelChoices[0]?.id || "",
                  });
                }}
                disabled={!canEdit || machine.providers.length === 0}
              >
                {machine.providers.length === 0 ? (
                  <option value="">{t("No runtimes")}</option>
                ) : (
                  machine.providers.map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.name}
                    </option>
                  ))
                )}
              </StyledSelect>
              {modelChoices.length > 0 ? (
                <div className="space-y-2">
                  <StyledSelect
                    value={modelSelectValue}
                    onChange={(event) => {
                      if (event.target.value === customModelOptionValue) {
                        setCustomModelActive(true);
                        updateDraft({
                          model: modelIsKnown ? "" : draft.model,
                        });
                        return;
                      }
                      setCustomModelActive(false);
                      updateDraft({ model: event.target.value });
                    }}
                    disabled={!canEdit}
                  >
                    <option value="">{t("Default model")}</option>
                    {modelChoices.map((choice) => (
                      <option key={choice.id} value={choice.id}>
                        {choice.label || choice.id}
                      </option>
                    ))}
                    <option value={customModelOptionValue}>{t("Custom...")}</option>
                  </StyledSelect>
                  {showCustomModel && (
                    <Input
                      value={draft.model}
                      onChange={(event) => updateDraft({ model: event.target.value })}
                      placeholder={t("Custom model")}
                      className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      disabled={!canEdit}
                    />
                  )}
                </div>
              ) : (
                <Input
                  value={draft.model}
                  onChange={(event) => updateDraft({ model: event.target.value })}
                  placeholder={t("Model")}
                  className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                  disabled={!canEdit}
                />
              )}
              <StyledSelect
                value={draft.reasoningEffort}
                onChange={(event) => updateDraft({ reasoningEffort: event.target.value })}
                disabled={!canEdit}
              >
                {reasoningEffortChoices.map((choice) => (
                  <option key={choice || "default"} value={choice}>
                    {choice
                      ? choice === "xhigh"
                        ? t("Extra high")
                        : t(capitalize(choice))
                      : t("Default reasoning")}
                  </option>
                ))}
              </StyledSelect>
              <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
                <input
                  type="checkbox"
                  checked={draft.autostart}
                  onChange={(event) => updateDraft({ autostart: event.target.checked })}
                  disabled={!canEdit}
                />
                {t("Autostart")}
              </label>
            </div>
          </HostDetailSection>
          <HostDetailSection title={t("Wake Policy")}>
            <div className="space-y-3">
              <div className="grid gap-3 md:grid-cols-2">
                <StyledSelect
                  value={wakePresetIdFor(draft.wake)}
                  onChange={(event) => {
                    const value = event.target.value;
                    if (value === "custom") return;
                    updateWake(
                      applyWakePreset(draft.wake, value as WakePresetId),
                    );
                  }}
                  disabled={!canEdit}
                >
                  {WAKE_PRESETS.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {t(preset.label)}
                    </option>
                  ))}
                  {wakePresetIdFor(draft.wake) === "custom" && (
                    <option value="custom">{t("Custom (via Advanced)")}</option>
                  )}
                </StyledSelect>
                <p className="flex items-center text-xs text-[#667085]">
                  {t(WAKE_PRESETS.find((p) => p.id === wakePresetIdFor(draft.wake))
                    ?.description ??
                    "Custom combination of the advanced wake fields below.")}
                </p>
              </div>
              <details className="rounded-lg border border-[#dfe3ec] bg-white px-3 py-2">
                <summary className="cursor-pointer select-none text-xs font-semibold text-[#667085]">
                  {t("Advanced")}
                </summary>
                <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                  <label className="flex h-10 items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm text-[#303849]">
                    <input
                      type="checkbox"
                      checked={draft.wake.coalesce !== false}
                      onChange={(event) => updateWake({ coalesce: event.target.checked })}
                      disabled={!canEdit}
                    />
                    {t("Coalesce")}
                  </label>
                  <Input
                    type="number"
                    min={0}
                    max={10000}
                    value={draft.wake.debounceMs ?? 0}
                    onChange={(event) =>
                      updateWake({ debounceMs: Math.max(0, Number(event.target.value) || 0) })
                    }
                    placeholder={t("Debounce ms")}
                    className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                    disabled={!canEdit}
                  />
                  <StyledSelect
                    value={draft.wake.replyReminder ?? "first-turn"}
                    onChange={(event) =>
                      updateWake({
                        replyReminder: event.target.value as NonNullable<
                          AgentSettingsDraft["wake"]["replyReminder"]
                        >,
                      })
                    }
                    disabled={!canEdit}
                  >
                    <option value="first-turn">{t("Reminder first turn")}</option>
                    <option value="every-turn">{t("Reminder every turn")}</option>
                    <option value="off">{t("Reminder off")}</option>
                  </StyledSelect>
                  <StyledSelect
                    value={draft.wake.onHumanMessageWhileBusy ?? "queue"}
                    onChange={(event) =>
                      updateWake({
                        onHumanMessageWhileBusy: event.target.value as NonNullable<
                          AgentSettingsDraft["wake"]["onHumanMessageWhileBusy"]
                        >,
                      })
                    }
                    disabled={!canEdit}
                  >
                    <option value="queue">{t("Busy: queue")}</option>
                    <option value="cancel_and_requeue">{t("Busy: cancel + requeue")}</option>
                    <option value="inject">{t("Busy: inject")}</option>
                  </StyledSelect>
                  <Input
                    type="number"
                    min={1}
                    max={8000}
                    value={draft.wake.contextTokenBudget ?? 900}
                    onChange={(event) =>
                      updateWake({
                        contextTokenBudget: Math.max(1, Number(event.target.value) || 900),
                      })
                    }
                    placeholder={t("Context tokens")}
                    className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                    disabled={!canEdit}
                  />
                  <StyledSelect
                    value={draft.wake.turnInputStyle ?? "minimal"}
                    onChange={(event) =>
                      updateWake({
                        turnInputStyle: event.target.value as NonNullable<
                          AgentSettingsDraft["wake"]["turnInputStyle"]
                        >,
                      })
                    }
                    disabled={!canEdit}
                  >
                    <option value="minimal">{t("Turn input: minimal text")}</option>
                    <option value="structured">{t("Turn input: structured JSON")}</option>
                  </StyledSelect>
                </div>
              </details>
            </div>
          </HostDetailSection>
          <HostDetailSection title={t("Environment Variables")}>
            <div className="space-y-2">
              {envEntries.map(([key, value], index) => (
                <div key={index} className="flex items-center gap-2">
                  <Input
                    value={key}
                    onChange={(event) => updateEnvEntry(index, event.target.value, value)}
                    placeholder={t("Key")}
                    className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm font-mono shadow-none"
                    disabled={!canEdit}
                  />
                  <Input
                    value={value}
                    onChange={(event) => updateEnvEntry(index, key, event.target.value)}
                    placeholder={t("Value")}
                    className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                    disabled={!canEdit}
                  />
                  <button
                    type="button"
                    onClick={() => removeEnvEntry(index)}
                    disabled={!canEdit}
                    className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500 disabled:opacity-40"
                    title={t("Remove")}
                  >
                    <X size={14} />
                  </button>
                </div>
              ))}
              <button
                type="button"
                onClick={addEnvEntry}
                disabled={!canEdit}
                className="inline-flex items-center gap-1.5 rounded-lg border border-dashed border-[#dfe3ec] px-3 py-2 text-xs text-[#596174] hover:border-[#c0c7d2] hover:bg-[#f0f2f5] disabled:opacity-40"
              >
                <Plus size={12} />
                {t("Add environment variable")}
              </button>
            </div>
          </HostDetailSection>
        </>
      )}

      {activeTab === "prompt" && (
        <AgentPromptStudio machine={machine} agent={agent} canEdit={canEdit} />
      )}

      {activeTab === "skills" && (
        <>
          <HostDetailSection
            title={t("Skills")}
            count={draft.bundleSkills.length}
            action={
              <Button
                type="button"
                size="sm"
                onClick={() => setSkillAddDialogOpen(true)}
                disabled={!canEdit || saving || skillAdding}
                className="rounded-lg"
              >
                <Plus size={14} />
                {t("Add Skill")}
              </Button>
            }
          >
            <div className="space-y-3">
              {draft.bundleSkills.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-4 py-6 text-sm text-[#667085]">
                  {t("No custom skills configured.")}
                </div>
              ) : (
                draft.bundleSkills.map((skill) => (
                  <div
                    key={skill.id}
                    className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-[#dfe3ec] bg-white px-4 py-3"
                  >
                    <div className="min-w-0">
                      <div className="text-sm font-bold text-[#111827]">{skill.id}</div>
                      <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                        {skill.source}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => removeSkill(skill.id)}
                      disabled={!canEdit || saving}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500 disabled:opacity-40"
                      title={t("Remove skill")}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                ))
              )}
            </div>
          </HostDetailSection>

          {skillAddDialogOpen && (
            <AgentSkillAddDialog
              machine={machine}
              canEdit={canEdit}
              saving={skillAdding}
              onCancel={() => setSkillAddDialogOpen(false)}
              onAddSkill={addSkill}
            />
          )}
        </>
      )}

      {activeTab === "settings" && (
        <>
          <HostDetailSection title={t("Info")}>
            <div className="divide-y divide-[#edf0f5]">
              <HostInfoRow label={t("Host")}>{machine.name}</HostInfoRow>
              <HostInfoRow label={t("Actor ID")} mono>{actor.id}</HostInfoRow>
              <HostInfoRow label={t("Profile Path")} mono>{agent.profilePath || t("Not set")}</HostInfoRow>
            </div>
          </HostDetailSection>

          <HostDetailSection title={t("Actions")}>
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#111827]">{t("Remove Agent")}</div>
                <div className="mt-1 text-sm text-[#667085]">
                  {t("Remove this member from {{host}}.", { host: machine.name })}
                </div>
              </div>
              {canEdit ? (
                <Button
                  variant="destructive"
                  size="sm"
                  title={t("Remove agent")}
                  onClick={() => onRemoveAgent(machine.id, actor.id)}
                  disabled={removing}
                  className="rounded-lg"
                >
                  {removing ? <Loader2 className="animate-spin" size={15} /> : <Trash2 size={15} />}
                  {t("Remove Agent")}
                </Button>
              ) : (
                <Badge variant="warning">
                  {t(machine.connectionStatus === "online" ? "unavailable" : "host offline")}
                </Badge>
              )}
            </div>
          </HostDetailSection>
        </>
      )}
    </div>
  );
}

function AgentSkillAddDialog({
  machine,
  canEdit,
  saving,
  onCancel,
  onAddSkill,
}: {
  machine: MachineInfo;
  canEdit: boolean;
  saving: boolean;
  onCancel: () => void;
  onAddSkill: (source: string) => Promise<boolean> | boolean;
}) {
  const { t } = useI18n();
  const canBrowseRemote = Boolean(
    machineCanRunCommands(machine) && machine.capabilities.includes("fs.dir.list"),
  );
  const skillDirectoryInputId = useId();
  const [browserPath, setBrowserPath] = useState<string | undefined>();
  const [browserReloadKey, setBrowserReloadKey] = useState(0);
  const [browser, setBrowser] = useState<MachineDirListResult | null>(null);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const [selectedSkillPath, setSelectedSkillPath] = useState("");
  const entries = browser?.entries ?? [];
  const skillSource = selectedSkillPath.trim();
  const canAdd = canEdit && skillSource.length > 0 && !saving;

  useEffect(() => {
    if (!canBrowseRemote) {
      setBrowser(null);
      setBrowserLoading(false);
      setBrowserError(null);
      return;
    }
    let cancelled = false;
    setBrowserLoading(true);
    setBrowserError(null);
    machineDirList({ machineId: machine.id, path: browserPath })
      .then((result) => {
        if (cancelled) return;
        setBrowser(result);
      })
      .catch((err) => {
        if (cancelled) return;
        setBrowser(null);
        setBrowserError(errorText(err));
      })
      .finally(() => {
        if (!cancelled) setBrowserLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [browserPath, browserReloadKey, canBrowseRemote, machine.id]);

  function openBrowserPath(path?: string | null) {
    if (!canBrowseRemote) return;
    const nextPath = path?.trim() || undefined;
    if (browserPath === nextPath) setBrowserReloadKey((currentKey) => currentKey + 1);
    setBrowserPath(nextPath);
  }

  function submitSkill(event: FormEvent) {
    event.preventDefault();
    if (!canAdd) return;
    void onAddSkill(skillSource);
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label={t("Add skill")}
      onMouseDown={onCancel}
    >
      <form
        className="flex max-h-[88vh] w-full max-w-4xl flex-col overflow-hidden rounded-lg border border-[#dfe3ec] bg-white shadow-soft"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={submitSkill}
      >
        <div className="flex min-w-0 items-start justify-between gap-4 border-b border-[#edf0f5] px-5 py-4">
          <div className="min-w-0">
            <div className="truncate text-sm font-bold text-[#111827]">{t("Add Skill")}</div>
            <div className="mt-0.5 truncate text-xs text-[#667085]">{machine.name}</div>
          </div>
          <button className="composer-icon h-8 min-w-8" type="button" title={t("Close")} onClick={onCancel}>
            <X size={14} />
          </button>
        </div>

        <div className="grid min-h-0 gap-4 overflow-y-auto px-5 py-4 soft-scrollbar lg:grid-cols-[minmax(260px,0.42fr)_minmax(0,1fr)]">
          <div className="space-y-3">
            <div>
              <label
                htmlFor={skillDirectoryInputId}
                className="mb-1 block text-xs font-bold text-[#596174]"
              >
                {t("Skill directory")}
              </label>
              <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                <Input
                  id={skillDirectoryInputId}
                  value={selectedSkillPath}
                  onChange={(event) => setSelectedSkillPath(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter") return;
                    event.preventDefault();
                    openBrowserPath(selectedSkillPath);
                  }}
                  placeholder="/path/to/skill"
                  className="h-10 rounded-lg border-[#dfe3ec] bg-white font-mono text-xs shadow-none"
                  disabled={!canEdit}
                  autoFocus
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => openBrowserPath(selectedSkillPath)}
                  disabled={!canBrowseRemote || browserLoading}
                  className="h-10 rounded-lg"
                >
                  <Folder size={13} />
                  {t("Browse")}
                </Button>
              </div>
            </div>
            {!canBrowseRemote && (
              <div className="rounded-lg border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2 text-xs text-[#667085]">
                {machine.connectionStatus === "online"
                  ? t("Directory browsing is unavailable on this host.")
                  : t("Start the host daemon to browse or add skills.")}
              </div>
            )}
          </div>

          <div className="min-h-80 rounded-lg border border-[#dfe3ec] bg-[#fbfbfd]">
            <div className="flex flex-wrap items-center gap-2 border-b border-[#edf0f5] px-3 py-2">
              {browser?.roots.map((root) => (
                <button
                  key={root.path}
                  type="button"
                  title={root.path}
                  onClick={() => openBrowserPath(root.path)}
                  disabled={!canBrowseRemote || browserLoading}
                  className="inline-flex h-8 max-w-[180px] items-center gap-1.5 rounded-md border border-[#dfe3ec] bg-white px-2 text-xs font-semibold text-[#596174] hover:border-[#c8c1ff] hover:text-[#503ed4] disabled:opacity-40"
                >
                  <HardDrive size={12} />
                  <span className="truncate">{root.label}</span>
                </button>
              ))}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => openBrowserPath(browser?.parent)}
                disabled={!canBrowseRemote || browserLoading || !browser?.parent}
                title={t("Parent directory")}
                className="h-8 rounded-md"
              >
                <ChevronLeft size={13} />
                {t("Up")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => browser?.path && setSelectedSkillPath(browser.path)}
                disabled={!canEdit || !browser?.path}
                title={t("Use current directory")}
                className="h-8 rounded-md"
              >
                <Check size={13} />
                {t("Use Current")}
              </Button>
            </div>

            <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 border-b border-[#edf0f5] px-3 py-2">
              <div
                className="flex h-9 min-w-0 items-center truncate rounded-lg border border-[#dfe3ec] bg-white px-3 font-mono text-xs text-[#667085]"
                title={browser?.path}
              >
                {browser?.path ?? t("No directory open")}
              </div>
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => openBrowserPath(browser?.path ?? browserPath)}
                disabled={!canBrowseRemote || browserLoading}
                title={t("Refresh")}
                className="h-9 w-9 rounded-lg"
              >
                {browserLoading ? <Loader2 className="animate-spin" size={14} /> : <RefreshCw size={14} />}
              </Button>
            </div>

            <div className="max-h-72 overflow-y-auto p-2 soft-scrollbar">
              {browserLoading ? (
                <div className="flex items-center gap-2 px-2 py-3 text-sm text-[#667085]">
                  <Loader2 className="animate-spin" size={15} />
                  {t("Loading")}
                </div>
              ) : !browser ? (
                <div className="px-2 py-8 text-center text-xs text-[#667085]">
                  {canBrowseRemote ? t("Open a directory.") : t("Browsing unavailable.")}
                </div>
              ) : entries.length === 0 ? (
                <div className="px-2 py-8 text-center text-xs text-[#667085]">{t("No child folders.")}</div>
              ) : (
                entries.map((entry) => (
                  <button
                    key={entry.path}
                    type="button"
                    onClick={() => {
                      setSelectedSkillPath(entry.path);
                      openBrowserPath(entry.path);
                    }}
                    disabled={!canEdit || browserLoading}
                    title={entry.path}
                    className={cn(
                      "grid h-9 w-full grid-cols-[auto_minmax(0,1fr)] items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-white disabled:opacity-40",
                      selectedSkillPath === entry.path
                        ? "bg-white text-[#503ed4]"
                        : "text-[#303849]",
                    )}
                  >
                    <Folder size={14} className="text-[#667085]" />
                    <span className="truncate font-mono text-xs">{entry.name}</span>
                  </button>
                ))
              )}
            </div>
            {browser?.truncated && (
              <div className="border-t border-[#edf0f5] px-3 py-2 text-xs text-[#8a93a5]">
                {t("Showing the first 500 folders.")}
              </div>
            )}
            {browserError && (
              <div className="mx-3 mb-3 rounded-md bg-[#fff4f4] px-2 py-1.5 text-xs text-[#b42318]">
                {browserError}
              </div>
            )}
          </div>
        </div>

        <div className="flex justify-end gap-2 border-t border-[#edf0f5] px-5 py-4">
          <Button variant="outline" size="sm" onClick={onCancel}>
            {t("Cancel")}
          </Button>
          <Button type="submit" size="sm" disabled={!canAdd}>
            {saving ? <Loader2 className="animate-spin" size={13} /> : <Plus size={13} />}
            {t("Add Skill")}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
