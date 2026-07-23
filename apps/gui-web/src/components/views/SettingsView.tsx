import {
  useEffect,
  useRef,
  useState,
} from "react";
import type { ComponentType } from "react";
import { PageHeader } from "@/components/shared/PageComponents";
import { ProviderAddDialog } from "@/components/settings/ProviderComponents";
import { HostListItem, RegisteredHostsEmpty, HostRegisterDialog, MachineCard } from "@/components/settings/MachineComponents";
import { MemberListItem, AgentRosterOverview, AgentCreateDialog, AgentMemberDetail } from "@/components/settings/AgentComponents";
import { ServiceListItem, ServiceMemberDetail, ServiceRosterOverview } from "@/components/settings/ServiceComponents";
import { HumanListItem, HumanRosterOverview } from "@/components/settings/HumanComponents";
import { ClearCacheSection } from "@/components/settings/ClearCacheSection";
import { Button } from "@/components/ui/button";
import { agentMemberEntries, serviceMemberEntries } from "@/lib/agent-utils";
import { agentFormForMachine, displayName, findAgentMemberEntry, machineCanCreateAgent } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Bot, ChevronDown, ListChecks, Loader2, Plus, RefreshCw, Server, Split, Users } from "lucide-react";
import type { Actor, MachineInfo, Run } from "@/ipc/types";
import type { ActorWorkspaceSection, AgentFormState, AgentMemberEntry, AgentUpdatePatch, ServiceMemberEntry } from "@/lib/types";


export function SettingsView({
  actors,
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
  onAddAgentSkill,
  onRemoveAgent,
  onOpenLocalPath,
}: {
  actors: Record<string, Actor>;
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
  onAddAgentSkill: (
    machineId: string,
    actorId: string,
    source: string,
  ) => Promise<boolean> | boolean;
  onRemoveAgent: (machineId: string, actorId: string) => void;
  onOpenLocalPath: (path: string) => void;
}) {
  const [activeSection, setActiveSection] = useState<ActorWorkspaceSection>("hosts");
  const [selectedMachineId, setSelectedMachineId] = useState<string | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(targetAgentId);
  const [selectedServiceId, setSelectedServiceId] = useState<string | null>(null);
  const [createAgentMachineId, setCreateAgentMachineId] = useState<string | null>(null);
  const [hostRegisterOpen, setHostRegisterOpen] = useState(false);
  const [providerAddOpen, setProviderAddOpen] = useState(false);
  const [memberCreateMenuOpen, setMemberCreateMenuOpen] = useState(false);
  const memberCreateMenuRef = useRef<HTMLDivElement | null>(null);
  const handledTargetAgentIdRef = useRef<string | null>(null);
  const humanActors = Object.values(actors)
    .filter((actor) => actor.kind === "human")
    .sort((left, right) => displayName(left).localeCompare(displayName(right)));
  const memberEntries = agentMemberEntries(machines);
  const serviceEntries = serviceMemberEntries(machines);
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
    { id: "humans", label: "Humans", count: humanActors.length, icon: Users },
    { id: "agents", label: "Agents", count: memberEntries.length, icon: Bot },
    { id: "services", label: "Services", count: serviceEntries.length, icon: Split },
  ];
  const selectedMemberEntry =
    selectedAgentId === null
      ? null
      : memberEntries.find((entry) => entry.agent.spec.actor.id === selectedAgentId) ?? null;
  const selectedServiceEntry =
    selectedServiceId === null
      ? null
      : serviceEntries.find((entry) => entry.service.id === selectedServiceId) ?? null;
  const selectedMachine =
    selectedMemberEntry?.machine ??
    selectedServiceEntry?.machine ??
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
    if (
      selectedServiceId &&
      !serviceEntries.some((entry) => entry.service.id === selectedServiceId)
    ) {
      setSelectedServiceId(null);
    }
  }, [selectedServiceId, serviceEntries]);

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
    setSelectedServiceId(null);
    setAgentForm(agentFormForMachine(agentForm, machine));
  }

  function selectAgent(entry: AgentMemberEntry) {
    setActiveSection("agents");
    setSelectedMachineId(entry.machine.id);
    setSelectedAgentId(entry.agent.spec.actor.id);
    setSelectedServiceId(null);
    setAgentForm(agentFormForMachine(agentForm, entry.machine));
  }

  function selectService(entry: ServiceMemberEntry) {
    setActiveSection("services");
    setSelectedMachineId(entry.machine.id);
    setSelectedAgentId(null);
    setSelectedServiceId(entry.service.id);
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
    setSelectedServiceId(null);
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
    setSelectedServiceId(null);
  }

  function showServiceRoster() {
    setActiveSection("services");
    setSelectedServiceId(null);
  }

  function openRegisterHostDialog() {
    setActiveSection("hosts");
    setSelectedAgentId(null);
    setSelectedServiceId(null);
    setHostRegisterOpen(true);
  }

  function hostRegistered(machine: MachineInfo) {
    setActiveSection("hosts");
    setSelectedMachineId(machine.id);
    setSelectedAgentId(null);
    setSelectedServiceId(null);
    setAgentForm(agentFormForMachine(agentForm, machine));
  }

  function selectSection(section: ActorWorkspaceSection) {
    setActiveSection(section);
    if (section === "agents") {
      setSelectedAgentId(null);
      setSelectedServiceId(null);
      return;
    }
    if (section === "hosts") {
      setSelectedAgentId(null);
      setSelectedServiceId(null);
      return;
    }
    setSelectedAgentId(null);
    setSelectedServiceId(null);
  }

  const detailContent =
    activeSection === "humans" ? (
      <HumanRosterOverview humans={humanActors} />
    ) : activeSection === "agents" ? (
      selectedMemberEntry ? (
        <AgentMemberDetail
          entry={selectedMemberEntry}
          busy={busy}
          onBack={showAgentRoster}
          onUpdateAgent={onUpdateAgent}
          onAddAgentSkill={onAddAgentSkill}
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
      selectedServiceEntry ? (
        <ServiceMemberDetail
          entry={selectedServiceEntry}
          onBack={showServiceRoster}
        />
      ) : (
        <ServiceRosterOverview
          entries={serviceEntries}
          machines={machines}
          onSelectService={selectService}
        />
      )
    );

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title="Actors"
        detail={`${humanActors.length} humans / ${onlineAgents}/${memberEntries.length} agents online / ${machines.length} hosts / ${providerCount} providers / ${serviceEntries.length} services`}
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
              {activeSection === "humans" && (
                <div>
                  <div className="mb-2 flex items-center justify-between px-1">
                    <div className="text-xs font-semibold uppercase tracking-wide text-[#596174]">
                      Humans
                    </div>
                    <span className="count-badge">{humanActors.length}</span>
                  </div>
                  {humanActors.length === 0 ? (
                    <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-3 text-xs text-[#667085]">
                      No humans registered.
                    </div>
                  ) : (
                    <div className="space-y-1.5">
                      {humanActors.map((human) => (
                        <HumanListItem key={human.id} human={human} />
                      ))}
                    </div>
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
                  <span className="count-badge">{serviceEntries.length}</span>
                </div>
                {serviceEntries.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-white p-3 text-xs text-[#667085]">
                    No services registered.
                  </div>
                ) : (
                  <div className="space-y-1.5">
                    {serviceEntries.map((entry) => (
                      <ServiceListItem
                        key={`${entry.machine.id}:${entry.service.id}`}
                        entry={entry}
                        selected={selectedServiceEntry?.service.id === entry.service.id}
                        onSelect={() => selectService(entry)}
                      />
                    ))}
                  </div>
                )}
              </div>
              )}
            </div>
          </aside>

          <div className="min-h-0 overflow-y-auto bg-white soft-scrollbar">
            {detailContent}
            <div className="border-t border-[#eef0f5] p-4">
              <ClearCacheSection />
            </div>
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
