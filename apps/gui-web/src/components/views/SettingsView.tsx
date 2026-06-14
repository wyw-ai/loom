import {
  useEffect,
  useRef,
  useState,
} from "react";
import type { ComponentType, FormEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { AgentProviderIcon, agentProviderIconKey } from "@/components/agent/AgentProviderIcon";
import { PageHeader } from "@/components/shared/PageComponents";
import { HostDetailSection, HostInfoRow, ProviderBadge, StyledSelect } from "@/components/shared/UIComponents";
import { AgentPromptStudio } from "@/components/views/AgentPromptStudio";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { actorAvatarUrl, agentDisplayName, agentMemberEntries, agentModelValue, agentSettingsDraft, getActorRunContext, providerAvailabilityGroups, providerForAgent, runStatusAnimationName, runStatusDotClass, runStatusFullLabel } from "@/lib/agent-utils";
import { avatarLibraryUrls, defaultProviderManifestText, reasoningEffortChoices } from "@/lib/constants";
import { agentFormForMachine, capitalize, errorText, findAgentMemberEntry, machineCanCreateAgent, resolveAgentProvider, statusDotClass } from "@/lib/format-utils";
import { cn, formatTime } from "@/lib/utils";
import { ArrowLeft, Bot, Check, ChevronDown, FileText, HardDrive, ListChecks, Loader2, Plus, RefreshCw, Server, Settings, Split, Trash2, X } from "lucide-react";
import type { MachineAgentProviderInfo, MachineInfo, Run } from "@/ipc/types";
import type { ActorWorkspaceSection, AgentDetailTab, AgentFormState, AgentMemberEntry, AgentSettingsDraft, AgentUpdatePatch, ProviderAvailabilityGroup } from "@/lib/types";
import * as ipc from "@/ipc/bridge";

export function SettingsView({
  busy,
  agentForm,
  setAgentForm,
  machines,
  runs,
  targetAgentId,
  onConsumeTargetAgent,
  onCheckMachines,
  onCreateMachine,
  onRemoveMachine,
  onAddAgent,
  onUpdateAgent,
  onRemoveAgent,
  onOpenLocalPath,
}: {
  busy: string | null;
  agentForm: AgentFormState;
  setAgentForm: (form: AgentFormState) => void;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  targetAgentId: string | null;
  onConsumeTargetAgent: () => void;
  onCheckMachines: () => void;
  onCreateMachine: (args: {
    name: string;
    dataRoot?: string;
  }) => Promise<MachineInfo | null> | MachineInfo | null;
  onRemoveMachine: (machineId: string) => void;
  onAddAgent: (form: AgentFormState) => Promise<boolean> | boolean;
  onUpdateAgent: (patch: AgentUpdatePatch) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
  onOpenLocalPath: (path: string) => void;
}) {
  const [activeSection, setActiveSection] = useState<ActorWorkspaceSection>("hosts");
  const [selectedMachineId, setSelectedMachineId] = useState<string | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(targetAgentId);
  const [createAgentMachineId, setCreateAgentMachineId] = useState<string | null>(null);
  const [hostRegisterOpen, setHostRegisterOpen] = useState(false);
  const [providerAddOpen, setProviderAddOpen] = useState(false);
  const [memberCreateMenuOpen, setMemberCreateMenuOpen] = useState(false);
  const memberCreateMenuRef = useRef<HTMLDivElement | null>(null);
  const handledTargetAgentIdRef = useRef<string | null>(null);
  const memberEntries = agentMemberEntries(machines);
  const onlineAgents = memberEntries.filter((entry) => entry.agent.status === "online").length;
  const canCreateAgentFromAnyHost = machines.some(
    (machine) => machineCanCreateAgent(machine) && machine.providers.length > 0,
  );
  const providerCount = machines.reduce(
    (count, machine) => count + machine.providers.length,
    0,
  );
  const workspaceSections: Array<{
    id: ActorWorkspaceSection;
    label: string;
    count: number;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "hosts", label: "Registered Hosts", count: machines.length, icon: Server },
    { id: "agents", label: "Agents", count: memberEntries.length, icon: Bot },
    { id: "services", label: "Services", count: 0, icon: Split },
  ];
  const selectedMemberEntry =
    selectedAgentId === null
      ? null
      : memberEntries.find((entry) => entry.agent.spec.actor.id === selectedAgentId) ?? null;
  const selectedMachine =
    selectedMemberEntry?.machine ??
    machines.find((machine) => machine.id === selectedMachineId) ??
    machines.find((machine) => machine.id === agentForm.machineId) ??
    machines[0];
  const createAgentMachine = createAgentMachineId
    ? machines.find((machine) => machine.id === createAgentMachineId) ?? null
    : null;

  useEffect(() => {
    if (machines.length === 0) {
      if (selectedMachineId) setSelectedMachineId(null);
      return;
    }
    if (selectedMachineId && machines.some((machine) => machine.id === selectedMachineId)) {
      return;
    }
    const nextMachine =
      machines.find((machine) => machine.id === agentForm.machineId) ?? machines[0];
    setSelectedMachineId(nextMachine.id);
    setAgentForm(agentFormForMachine(agentForm, nextMachine));
  }, [agentForm, machines, selectedMachineId, setAgentForm]);

  useEffect(() => {
    if (
      selectedAgentId &&
      !memberEntries.some((entry) => entry.agent.spec.actor.id === selectedAgentId)
    ) {
      setSelectedAgentId(null);
    }
  }, [memberEntries, selectedAgentId]);

  useEffect(() => {
    if (!targetAgentId) {
      handledTargetAgentIdRef.current = null;
      return;
    }
    const entry = findAgentMemberEntry(machines, targetAgentId);
    if (handledTargetAgentIdRef.current !== targetAgentId) {
      handledTargetAgentIdRef.current = targetAgentId;
      setActiveSection("agents");
      setSelectedAgentId(targetAgentId);
      onConsumeTargetAgent();
    }
    if (entry && selectedAgentId === targetAgentId) {
      setSelectedMachineId(entry.machine.id);
    }
  }, [machines, onConsumeTargetAgent, selectedAgentId, targetAgentId]);

  useEffect(() => {
    if (!createAgentMachineId) return;
    if (!machines.some((machine) => machine.id === createAgentMachineId)) {
      setCreateAgentMachineId(null);
    }
  }, [createAgentMachineId, machines]);

  useEffect(() => {
    if (!memberCreateMenuOpen) return;
    const close = (event: globalThis.MouseEvent) => {
      const target = event.target;
      if (
        target instanceof Node &&
        memberCreateMenuRef.current?.contains(target)
      ) {
        return;
      }
      setMemberCreateMenuOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [memberCreateMenuOpen]);

  function selectMachine(machine: MachineInfo) {
    setActiveSection("hosts");
    setSelectedMachineId(machine.id);
    setSelectedAgentId(null);
    setAgentForm(agentFormForMachine(agentForm, machine));
  }

  function selectAgent(entry: AgentMemberEntry) {
    setActiveSection("agents");
    setSelectedMachineId(entry.machine.id);
    setSelectedAgentId(entry.agent.spec.actor.id);
    setAgentForm(agentFormForMachine(agentForm, entry.machine));
  }

  function openCreateAgentDialog(machine?: MachineInfo | null) {
    const nextMachine =
      machine ??
      (selectedMachine && machineCanCreateAgent(selectedMachine)
        ? selectedMachine
        : null) ??
      machines.find(
        (item) => machineCanCreateAgent(item) && item.providers.length > 0,
      ) ??
      machines.find(machineCanCreateAgent) ??
      selectedMachine ??
      machines[0];
    if (!nextMachine) return;
    setActiveSection("agents");
    setSelectedMachineId(nextMachine.id);
    setSelectedAgentId(null);
    setAgentForm(agentFormForMachine(agentForm, nextMachine));
    setCreateAgentMachineId(nextMachine.id);
    setMemberCreateMenuOpen(false);
  }

  function selectCreateAgentMachine(machineId: string) {
    const nextMachine = machines.find((machine) => machine.id === machineId) ?? null;
    if (!nextMachine) return;
    setSelectedMachineId(nextMachine.id);
    setCreateAgentMachineId(nextMachine.id);
    setAgentForm(agentFormForMachine(agentForm, nextMachine));
  }

  function showAgentRoster() {
    setActiveSection("agents");
    setSelectedAgentId(null);
  }

  function openRegisterHostDialog() {
    setActiveSection("hosts");
    setSelectedAgentId(null);
    setHostRegisterOpen(true);
  }

  function hostRegistered(machine: MachineInfo) {
    setActiveSection("hosts");
    setSelectedMachineId(machine.id);
    setSelectedAgentId(null);
    setAgentForm(agentFormForMachine(agentForm, machine));
  }

  function selectSection(section: ActorWorkspaceSection) {
    setActiveSection(section);
    if (section === "agents") {
      setSelectedAgentId(null);
      return;
    }
    if (section === "hosts") {
      setSelectedAgentId(null);
      return;
    }
    setSelectedAgentId(null);
  }

  const detailContent =
    activeSection === "agents" ? (
      selectedMemberEntry ? (
        <AgentMemberDetail
          entry={selectedMemberEntry}
          busy={busy}
          onBack={showAgentRoster}
          onUpdateAgent={onUpdateAgent}
          onRemoveAgent={onRemoveAgent}
        />
      ) : (
        <AgentRosterOverview
          machines={machines}
          entries={memberEntries}
          busy={busy}
          onOpenCreateAgent={openCreateAgentDialog}
          onOpenProviderAdd={() => setProviderAddOpen(true)}
          onSelectAgent={selectAgent}
          onRemoveAgent={onRemoveAgent}
        />
      )
    ) : activeSection === "hosts" ? (
      selectedMachine ? (
        <MachineCard
          machine={selectedMachine}
          busy={busy}
          onRemove={onRemoveMachine}
          onOpenLocalPath={onOpenLocalPath}
        />
      ) : (
        <RegisteredHostsEmpty
          busy={busy}
          onOpenRegisterHost={openRegisterHostDialog}
        />
      )
    ) : (
      <ServiceRosterOverview />
    );

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title="Actors"
        detail={`${onlineAgents}/${memberEntries.length} agents online / ${machines.length} hosts / ${providerCount} providers`}
      />
      <div className="min-h-0 flex-1 overflow-hidden bg-white">
        <div className="grid h-full min-h-0 grid-cols-1 lg:grid-cols-[minmax(208px,224px)_minmax(0,1fr)]">
          <aside className="flex min-h-0 flex-col border-r border-[#e2e6ef] bg-[#fbfbfd]">
            <div className="border-b border-[#edf0f5] p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0 text-sm font-bold text-[#111827]">
                  Actor Manage
                </div>
                <Button
                  variant="outline"
                  size="icon"
                  title="Refresh hosts"
                  onClick={onCheckMachines}
                  disabled={busy === "machine:check"}
                  className="h-8 w-8 rounded-lg border-[#dfe3ec] bg-white"
                >
                  {busy === "machine:check" ? (
                    <Loader2 className="animate-spin" size={15} />
                  ) : (
                    <RefreshCw size={15} />
                  )}
                </Button>
              </div>
              <div className="mt-3 space-y-1">
                {workspaceSections.map((section) => {
                  const Icon = section.icon;
                  const selected = activeSection === section.id;
                  return (
                    <button
                      key={section.id}
                      type="button"
                      className={cn(
                        "flex h-9 w-full items-center gap-2 rounded-lg border px-2.5 text-left text-sm font-semibold transition-colors",
                        selected
                          ? "border-[#bdb7ff] bg-white text-[#503ed4] shadow-sm"
                          : "border-transparent text-[#596174] hover:border-[#dfe3ec] hover:bg-white",
                      )}
                      onClick={() => selectSection(section.id)}
                    >
                      <Icon size={15} className="shrink-0" />
                      <span className="min-w-0 flex-1 truncate">{section.label}</span>
                      <span className="count-badge">{section.count}</span>
                    </button>
                  );
                })}
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-3 soft-scrollbar">
              {activeSection === "hosts" && (
                <div className="space-y-2">
                  <div className="mb-2 flex items-center justify-between px-1">
                    <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                      Registered Hosts
                    </div>
                    <div className="flex items-center gap-1.5">
                      <span className="count-badge">{machines.length}</span>
                      <button
                        type="button"
                        className="composer-icon h-7 min-w-7 rounded-lg border border-[#dfe3ec] bg-white text-[#503ed4]"
                        title="Register host"
                        onClick={openRegisterHostDialog}
                        disabled={busy === "machine:create"}
                      >
                        {busy === "machine:create" ? (
                          <Loader2 className="animate-spin" size={13} />
                        ) : (
                          <Plus size={13} />
                        )}
                      </button>
                    </div>
                  </div>
                  {machines.length === 0 ? (
                    <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-4 text-sm text-[#667085]">
                      <div>No registered hosts.</div>
                      <Button
                        size="sm"
                        className="mt-3 rounded-lg"
                        onClick={openRegisterHostDialog}
                        disabled={busy === "machine:create"}
                      >
                        {busy === "machine:create" ? (
                          <Loader2 className="animate-spin" size={14} />
                        ) : (
                          <Plus size={14} />
                        )}
                        Register Host
                      </Button>
                    </div>
                  ) : (
                    machines.map((machine) => (
                      <HostListItem
                        key={machine.id}
                        machine={machine}
                        selected={selectedMachine?.id === machine.id}
                        onSelect={() => selectMachine(machine)}
                      />
                    ))
                  )}
                </div>
              )}
              {activeSection === "agents" && (
                <div>
                <div className="mb-2 flex items-center justify-between px-1">
                  <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    Agents
                  </div>
                  <div ref={memberCreateMenuRef} className="relative flex items-center gap-1.5">
                    <span className="count-badge">{memberEntries.length}</span>
                    <button
                      type="button"
                      className="composer-icon h-7 min-w-9 gap-0.5 rounded-lg border border-[#dfe3ec] bg-white text-[#503ed4]"
                      title="Add actor"
                      aria-expanded={memberCreateMenuOpen}
                      onClick={() => setMemberCreateMenuOpen((open) => !open)}
                    >
                      <Plus size={13} />
                      <ChevronDown size={12} />
                    </button>
                    {memberCreateMenuOpen && (
                      <div className="absolute right-0 top-full z-30 mt-2 w-48 rounded-xl border border-[#dfe3ec] bg-white p-1.5 shadow-[0_18px_44px_rgb(16_24_40_/_0.16)]">
                        <button
                          type="button"
                          className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm font-semibold text-[#303849] hover:bg-[#f5f3ff]"
                        onClick={() => openCreateAgentDialog()}
                      >
                        <Bot size={15} className="text-[#503ed4]" />
                        Agent
                        </button>
                        <button
                          type="button"
                          disabled
                          className="mt-1 flex w-full cursor-not-allowed items-start gap-2 rounded-lg px-2.5 py-2 text-left opacity-55"
                        >
                          <Server size={15} className="mt-0.5 text-[#667085]" />
                          <span className="min-w-0">
                            <span className="block text-sm font-semibold text-[#303849]">
                              Service
                            </span>
                            <span className="block text-xs font-medium text-[#667085]">
                              Coming Soon
                            </span>
                          </span>
                        </button>
                      </div>
                    )}
                  </div>
                </div>
                <div className="space-y-1.5">
                  <button
                    type="button"
                    className={cn(
                      "flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-colors",
                      selectedAgentId === null
                        ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
                        : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
                    )}
                    onClick={showAgentRoster}
                  >
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[#edf0f5] bg-white text-[#503ed4]">
                      <ListChecks size={15} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-bold text-[#111827]">
                        All Agents
                      </span>
                      <span className="mt-0.5 block truncate text-xs text-[#667085]">
                        {memberEntries.length} registered
                      </span>
                    </span>
                  </button>
                  {memberEntries.length === 0 ? (
                    <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-3 text-xs text-[#667085]">
                      <div>No agents registered.</div>
                      <Button
                        size="sm"
                        className="mt-3 rounded-lg"
                        onClick={() => openCreateAgentDialog()}
                        disabled={!canCreateAgentFromAnyHost}
                      >
                        <Plus size={14} />
                        Create Agent
                      </Button>
                    </div>
                  ) : (
                    memberEntries.map((entry) => (
                      <MemberListItem
                        key={`${entry.machine.id}:${entry.agent.spec.actor.id}`}
                        entry={entry}
                        runs={runs}
                        selected={selectedMemberEntry?.agent.spec.actor.id === entry.agent.spec.actor.id}
                        onSelect={() => selectAgent(entry)}
                      />
                    ))
                  )}
                </div>
              </div>
              )}
              {activeSection === "services" && (
                <div>
                <div className="mb-2 flex items-center justify-between px-1">
                  <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    Services
                  </div>
                  <span className="count-badge">0</span>
                </div>
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-3 text-xs text-[#667085]">
                  No services registered.
                </div>
              </div>
              )}
            </div>
          </aside>

          <div className="min-h-0 overflow-y-auto bg-white soft-scrollbar">
            {detailContent}
          </div>
        </div>
      </div>
      {createAgentMachine && (
        <AgentCreateDialog
          agentForm={agentForm}
          busy={busy}
          machine={createAgentMachine}
          machines={machines}
          setAgentForm={setAgentForm}
          onSelectMachine={selectCreateAgentMachine}
          onAddAgent={onAddAgent}
          onClose={() => setCreateAgentMachineId(null)}
        />
      )}
      {providerAddOpen && (
        <ProviderAddDialog
          machines={machines}
          preferredMachineId={selectedMachine?.id ?? null}
          onCheckMachines={onCheckMachines}
          onClose={() => setProviderAddOpen(false)}
        />
      )}
      {hostRegisterOpen && (
        <HostRegisterDialog
          busy={busy}
          onCreateMachine={onCreateMachine}
          onCreated={hostRegistered}
          onClose={() => setHostRegisterOpen(false)}
        />
      )}
    </section>
  );
}


export function SettingsSection({
  title,
  detail,
  action,
  children,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-[#dfe3ec] bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-bold text-[#111827]">{title}</div>
          <div className="mt-1 text-sm text-[#667085]">{detail}</div>
        </div>
        {action}
      </div>
      {children}
    </section>
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
  const actor = entry.agent.spec.actor;
  const ctx = getActorRunContext(runs, actor.id);
  const working = ctx != null && !ctx.isTerminal;
  const label = working ? (runStatusFullLabel(ctx) ?? "Processing…") : entry.machine.name;
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
          <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
            <div className="min-w-0">
              <div className="text-sm font-bold text-[#111827]">Start Host</div>
              <div className="mt-1 text-sm text-[#667085]">
                Run the serve command above, then refresh hosts after the daemon connects.
              </div>
            </div>
            <Badge variant="warning">pending daemon</Badge>
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
            <h2 className="text-xl font-bold text-[#111827]">Agents</h2>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
              <span>{entries.length} registered</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{onlineAgents} online</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{providerGroups.length} provider types</span>
            </div>
          </div>
          <Button
            onClick={() => onOpenCreateAgent()}
            disabled={!canCreateAgent}
            className="rounded-lg"
          >
            <Plus size={15} />
            Create Agent
          </Button>
        </div>
      </section>

      <HostDetailSection title="Agent Roster" count={entries.length}>
        <div className="space-y-2">
          {entries.length === 0 ? (
            <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
              <div>No agents registered.</div>
              <Button
                size="sm"
                className="mt-3 rounded-lg"
                onClick={() => onOpenCreateAgent()}
                disabled={!canCreateAgent}
              >
                <Plus size={14} />
                Create Agent
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
        title="Create Readiness"
        action={
          <Button
            variant="outline"
            size="sm"
            onClick={onOpenProviderAdd}
            disabled={machines.length === 0}
            className="h-8 rounded-lg border-[#dfe3ec] bg-white"
          >
            <Plus size={14} />
            Add Provider
          </Button>
        }
      >
        <div className="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
          <div className="min-w-0 rounded-xl border border-[#edf0f5] bg-white p-3">
            <div className="mb-3 flex items-center gap-2">
              <div className="flex items-center gap-2">
                <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                  Hosts
                </div>
                <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
                  {hostRows.length}
                </span>
              </div>
            </div>
            <div className="grid gap-2">
              {hostRows.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                  No registered hosts.
                </div>
              ) : (
                hostRows.map(({ machine, canCreate }) => {
                  const reason = !machineCanCreateAgent(machine)
                    ? "read only"
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
                        <Badge variant={canCreate ? "outline" : "warning"}>{reason}</Badge>
                      </div>
                      <div className="mt-2 truncate text-xs text-[#667085]">
                        {machine.providers.length} providers / {machine.onlineAgentCount}/{machine.agentCount} online
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
                Provider Availability
              </div>
              <span className="font-mono text-xs font-semibold text-[#9aa1ae]">
                {providerGroups.length}
              </span>
            </div>
            <div className="max-h-60 space-y-2 overflow-y-auto soft-scrollbar">
              {providerGroups.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[#dfe3ec] bg-white p-3 text-sm text-[#667085]">
                  No providers detected.
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
            <span className="font-mono">{agentModelValue(entry.agent) || "default"}</span>
          </span>
        </span>
      </button>
      <div className="flex items-center gap-2">
        <Badge variant={entry.agent.status === "online" ? "success" : "outline"}>
          {entry.agent.status}
        </Badge>
        {!entry.machine.readOnly && (
          <Button
            variant="ghost"
            size="icon"
            title="Remove agent"
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


export function ServiceRosterOverview() {
  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <h2 className="text-xl font-bold text-[#111827]">Services</h2>
        <div className="mt-2 text-sm text-[#667085]">0 registered</div>
      </section>
      <HostDetailSection title="Service Roster" count={0}>
        <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
          No services registered.
        </div>
      </HostDetailSection>
    </div>
  );
}

const customModelOptionValue = "__loom_custom_model__";


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
  const createStatusText = !canCreateAgent
    ? "This host is read-only for the current account."
    : !selectedProvider
      ? "No runtime detected for this host."
      : `${selectedProvider.name} on ${machine.name}`;
  const createStatusBadge = !canCreateAgent
    ? "read only"
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
              New Agent
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
              <span className="font-semibold text-[#303849]">{machine.name}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{createStatusText}</span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant={canCreateAgent && selectedProvider ? "outline" : "warning"}>
              {createStatusBadge}
            </Badge>
            <button
              type="button"
              className="composer-icon h-8 min-w-8"
              title="Close"
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
                Host
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                {machines.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                    Register a host before creating agents.
                  </div>
                ) : (
                  machines.map((item) => {
                    const selected = item.id === machine.id;
                    const ready = machineCanCreateAgent(item) && item.providers.length > 0;
                    const blockedReason = !machineCanCreateAgent(item)
                      ? "Read only"
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
                            {item.providers.length} runtimes / {item.onlineAgentCount}/{item.agentCount} online
                          </span>
                        </span>
                        <Badge variant={ready ? "outline" : "warning"}>
                          {ready ? "ready" : blockedReason}
                        </Badge>
                      </button>
                    );
                  })
                )}
              </div>
            </section>

            <section>
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                Runtime
              </div>
              {machine.providers.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                  No runtimes detected for this host.
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
                            {provider.defaultModel || `${provider.modelChoices.length} models`}
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
                placeholder="Agent name"
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                disabled={!canCreateAgent}
              />
              <Input
                value={draft.actorId}
                onChange={(event) => updateAgentForm({ actorId: event.target.value })}
                placeholder="Actor id (optional)"
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
                    <option value="">Default model</option>
                    {modelChoices.map((choice) => (
                      <option key={choice.id} value={choice.id}>
                        {choice.label || choice.id}
                      </option>
                    ))}
                    <option value={customModelOptionValue}>Custom...</option>
                  </StyledSelect>
                ) : null}
                {showCustomModel && (
                  <Input
                    value={draft.model}
                    onChange={(event) => updateAgentForm({ model: event.target.value })}
                    placeholder="Custom model"
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
                Autostart
              </label>
              <Input
                value={draft.description}
                onChange={(event) => updateAgentForm({ description: event.target.value })}
                placeholder="Description"
                className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                disabled={!canCreateAgent}
              />
              <Textarea
                value={draft.instructions}
                onChange={(event) => updateAgentForm({ instructions: event.target.value })}
                placeholder="Agent instructions"
                className="min-h-28 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                disabled={!canCreateAgent}
              />
              <div className="space-y-2 md:col-span-2">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                    Environment Variables
                  </span>
                  <button
                    type="button"
                    onClick={addEnvEntry}
                    disabled={!canCreateAgent}
                    className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-[#dfe3ec] bg-white text-[#596174] hover:bg-[#f0f2f5] disabled:opacity-40"
                    title="Add environment variable"
                  >
                    <Plus size={12} />
                  </button>
                </div>
                {envEntries.map(([key, value], index) => (
                  <div key={index} className="flex items-center gap-2">
                    <Input
                      value={key}
                      onChange={(event) => updateEnvEntry(index, event.target.value, value)}
                      placeholder="Key"
                      className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm font-mono shadow-none"
                      disabled={!canCreateAgent}
                    />
                    <Input
                      value={value}
                      onChange={(event) => updateEnvEntry(index, key, event.target.value)}
                      placeholder="Value"
                      className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      disabled={!canCreateAgent}
                    />
                    <button
                      type="button"
                      onClick={() => removeEnvEntry(index)}
                      disabled={!canCreateAgent}
                      className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500 disabled:opacity-40"
                      title="Remove"
                    >
                      <X size={14} />
                    </button>
                  </div>
                ))}
                {envEntries.length === 0 && (
                  <div className="rounded-lg border border-dashed border-[#dfe3ec] px-3 py-2 text-center text-xs text-[#9aa1ae]">
                    No environment variables. Click <Plus size={10} className="inline align-middle" /> to add one.
                  </div>
                )}
              </div>
            </section>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[#edf0f5] px-5 py-4">
          <div className="text-xs font-medium text-[#667085]">
            {readyHosts.length} ready hosts / {writableHosts.length} writable
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
            Create Agent
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
  onRemoveAgent,
}: {
  entry: AgentMemberEntry;
  busy: string | null;
  onBack: () => void;
  onUpdateAgent: (patch: AgentUpdatePatch) => void;
  onRemoveAgent: (machineId: string, actorId: string) => void;
}) {
  const { machine, agent } = entry;
  const actor = agent.spec.actor;
  const agentDetailKey = `${machine.id}:${actor.id}`;
  const [draft, setDraft] = useState<AgentSettingsDraft>(() =>
    agentSettingsDraft(machine, agent),
  );
  const [activeTab, setActiveTab] = useState<AgentDetailTab>("profile");
  const [customModelActive, setCustomModelActive] = useState(false);
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
  const removing = busy === `agent:remove:${actor.id}`;
  const canEdit = !machine.readOnly;
  const detailTabs: Array<{
    id: AgentDetailTab;
    label: string;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "profile", label: "Profile", icon: Bot },
    { id: "prompt", label: "Prompt Studio", icon: FileText },
    { id: "settings", label: "Settings", icon: Settings },
  ];

  useEffect(() => {
    if (handledAgentDetailKeyRef.current === agentDetailKey) return;
    handledAgentDetailKeyRef.current = agentDetailKey;
    setDraft(agentSettingsDraft(machine, agent));
    setCustomModelActive(false);
  }, [agentDetailKey, machine, agent]);

  useEffect(() => {
    setActiveTab("profile");
  }, [agentDetailKey]);

  function updateDraft(patch: Partial<AgentSettingsDraft>) {
    setDraft((current) => ({ ...current, ...patch }));
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

  function saveAgent() {
    onUpdateAgent({
      machineId: machine.id,
      actorId: actor.id,
      displayName: draft.displayName,
      description: draft.description,
      instructions: draft.instructions,
      providerId: draft.providerId || undefined,
      model: draft.model,
      reasoningEffort: draft.reasoningEffort,
      autostart: draft.autostart,
      avatarUrl: draft.avatarUrl,
      env: draft.env,
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
          All Agents
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
                <span>{capitalize(agent.status)}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{actor.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant={agent.status === "online" ? "success" : "outline"}>
                  {agent.status}
                </Badge>
                <Badge variant="secondary">{machine.name}</Badge>
                {machine.readOnly && <Badge variant="warning">read only</Badge>}
              </div>
            </div>
          </div>
          <Button
            onClick={saveAgent}
            disabled={!canEdit || saving || !draft.displayName.trim()}
            className="rounded-lg"
          >
            {saving ? <Loader2 className="animate-spin" size={15} /> : <Check size={15} />}
            Save Changes
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
          <HostDetailSection title="Profile">
            <div className="grid gap-5 xl:grid-cols-[minmax(260px,0.42fr)_minmax(0,1fr)]">
              <div className="min-w-0">
                <div className="mb-3 text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
                  Avatar Library
                </div>
                <div className="grid max-h-64 grid-cols-[repeat(auto-fill,minmax(38px,1fr))] gap-2 overflow-y-auto rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-3 soft-scrollbar">
                  {avatarLibraryUrls.map((url) => (
                    <button
                      key={url}
                      type="button"
                      title={url.split("/").pop() ?? "Avatar"}
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
                  placeholder="Display name"
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
                  placeholder="Description"
                  className="min-h-20 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                  disabled={!canEdit}
                />
                <Textarea
                  value={draft.instructions}
                  onChange={(event) => updateDraft({ instructions: event.target.value })}
                  placeholder="Agent instructions"
                  className="min-h-32 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none md:col-span-2"
                  disabled={!canEdit}
                />
              </div>
            </div>
          </HostDetailSection>

          <HostDetailSection title="Runtime Configuration">
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
                  <option value="">No runtimes</option>
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
                    <option value="">Default model</option>
                    {modelChoices.map((choice) => (
                      <option key={choice.id} value={choice.id}>
                        {choice.label || choice.id}
                      </option>
                    ))}
                    <option value={customModelOptionValue}>Custom...</option>
                  </StyledSelect>
                  {showCustomModel && (
                    <Input
                      value={draft.model}
                      onChange={(event) => updateDraft({ model: event.target.value })}
                      placeholder="Custom model"
                      className="h-10 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                      disabled={!canEdit}
                    />
                  )}
                </div>
              ) : (
                <Input
                  value={draft.model}
                  onChange={(event) => updateDraft({ model: event.target.value })}
                  placeholder="Model"
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
                    {choice ? capitalize(choice) : "Default reasoning"}
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
                Autostart
              </label>
            </div>
          </HostDetailSection>
          <HostDetailSection title="Environment Variables">
            <div className="space-y-2">
              {envEntries.map(([key, value], index) => (
                <div key={index} className="flex items-center gap-2">
                  <Input
                    value={key}
                    onChange={(event) => updateEnvEntry(index, event.target.value, value)}
                    placeholder="Key"
                    className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm font-mono shadow-none"
                    disabled={!canEdit}
                  />
                  <Input
                    value={value}
                    onChange={(event) => updateEnvEntry(index, key, event.target.value)}
                    placeholder="Value"
                    className="h-9 flex-1 rounded-lg border-[#dfe3ec] bg-white text-sm shadow-none"
                    disabled={!canEdit}
                  />
                  <button
                    type="button"
                    onClick={() => removeEnvEntry(index)}
                    disabled={!canEdit}
                    className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500 disabled:opacity-40"
                    title="Remove"
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
                Add environment variable
              </button>
            </div>
          </HostDetailSection>
        </>
      )}

      {activeTab === "prompt" && (
        <AgentPromptStudio machine={machine} agent={agent} canEdit={canEdit} />
      )}

      {activeTab === "settings" && (
        <>
          <HostDetailSection title="Info">
            <div className="divide-y divide-[#edf0f5]">
              <HostInfoRow label="Host">{machine.name}</HostInfoRow>
              <HostInfoRow label="Actor ID" mono>{actor.id}</HostInfoRow>
              <HostInfoRow label="Profile Path" mono>{agent.profilePath || "Not set"}</HostInfoRow>
            </div>
          </HostDetailSection>

          <HostDetailSection title="Actions">
            <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#dfe3ec] bg-[#fbfbfd] px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-bold text-[#111827]">Remove Agent</div>
                <div className="mt-1 text-sm text-[#667085]">
                  Remove this member from {machine.name}.
                </div>
              </div>
              {!machine.readOnly ? (
                <Button
                  variant="destructive"
                  size="sm"
                  title="Remove agent"
                  onClick={() => onRemoveAgent(machine.id, actor.id)}
                  disabled={removing}
                  className="rounded-lg"
                >
                  {removing ? <Loader2 className="animate-spin" size={15} /> : <Trash2 size={15} />}
                  Remove Agent
                </Button>
              ) : (
                <Badge variant="warning">read only</Badge>
              )}
            </div>
          </HostDetailSection>
        </>
      )}
    </div>
  );
}


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


