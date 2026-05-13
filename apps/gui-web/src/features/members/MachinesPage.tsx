import { useEffect, useState, type ReactNode } from "react";
import clsx from "clsx";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  Cpu,
  FileText,
  FolderOpen,
  HardDrive,
  Monitor,
  Plus,
  RefreshCw,
  Server,
  Terminal,
  Trash2,
  UserPlus,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type {
  AgentInfo,
  AgentProviderSummary,
  MachineAgentInfo,
  MachineInfo,
} from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { PixelAvatar } from "@/features/common/PixelAvatar";

const KNOWN_RUNTIMES = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex CLI" },
  { id: "qoder", name: "Qoder CLI" },
  { id: "copilot", name: "Copilot CLI" },
  { id: "opencode", name: "OpenCode" },
];

const CODEX_MODELS = [
  { id: "gpt-5.5", label: "GPT-5.5" },
  { id: "gpt-5.4", label: "GPT-5.4" },
  { id: "gpt-5.4-mini", label: "GPT-5.4 Mini" },
  { id: "gpt-5.3-codex", label: "GPT-5.3 Codex" },
];

const REASONING_CHOICES = ["low", "medium", "high", "xhigh"];

export function MachinesPage() {
  const upsertMany = useActors((s) => s.upsertMany);
  const removeMany = useActors((s) => s.removeMany);
  const pushToast = useUI((s) => s.pushToast);
  const openModal = useUI((s) => s.openModal);
  const activeWorkspace = useWorkspaces(
    (s) => s.workspaces.find((workspace) => workspace.id === s.activeId) ?? null,
  );
  const [machines, setMachines] = useState<MachineInfo[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [agentOpen, setAgentOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const applyMachines = (rows: MachineInfo[]) => {
    setMachines(rows);
    upsertMany(
      rows.flatMap((machine) =>
        machine.agents.map((agent) => agent.spec.actor),
      ),
    );
    setSelectedId((current) =>
      current && rows.some((machine) => machine.id === current)
        ? current
        : rows[0]?.id ?? null,
    );
  };

  const load = async (check = false, showLoading = true) => {
    const requestWorkspaceId = useWorkspaces.getState().activeId;
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const result = check ? await ipc.machineCheck() : await ipc.machineList();
      if (useWorkspaces.getState().activeId !== requestWorkspaceId) {
        return [];
      }
      applyMachines(result.machines);
      return result.machines;
    } catch (e) {
      if (useWorkspaces.getState().activeId !== requestWorkspaceId) {
        return [];
      }
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      if (showLoading) pushToast("error", `computer/list failed: ${message}`);
      return machines;
    } finally {
      if (showLoading) setLoading(false);
    }
  };

  useEffect(() => {
    setMachines([]);
    setSelectedId(null);
    setError(null);
    void load(true);
    const interval = window.setInterval(() => void load(true, false), 4000);
    return () => window.clearInterval(interval);
  }, [activeWorkspace?.id]);

  const selected = machines.find((machine) => machine.id === selectedId) ?? null;

  const addMachine = async (input: MachineCreateInput) => {
    const before = new Set(machines.map((machine) => machine.id));
    const result = await ipc.machineCreate(input);
    applyMachines(result.machines);
    const created =
      result.machines.find((machine) => !before.has(machine.id)) ??
      result.machines[result.machines.length - 1];
    setSelectedId(created?.id ?? null);
    pushToast("info", `${created.name} configured`);
    return created;
  };

  const createAgent = async (machine: MachineInfo, input: AgentCreateInput) => {
    if (machine.readOnly) {
      pushToast("warn", "remote computers are read-only until server-mediated commands land");
      return;
    }
    const result = await ipc.machineAgentCreate({
      machineId: machine.id,
      providerId: input.providerId,
      actorId: input.actorId,
      name: input.name,
      description: input.description,
      model: input.model,
      reasoningEffort: input.reasoningEffort,
      autostart: input.autostart,
    });
    applyMachines(result.machines);
    setSelectedId(machine.id);
    pushToast("info", `${input.name} created on ${machine.name}`);
  };

  const removeMachine = (machine: MachineInfo) => {
    if (machine.readOnly) {
      pushToast("warn", "remote computers are read-only until server-mediated commands land");
      return;
    }
    if (machines.length <= 1) {
      pushToast("warn", "At least one computer is required");
      return;
    }
    openModal({
      type: "confirm",
      title: `Delete ${machine.name}?`,
      body: "This removes the local computer profile. Agent data directories stay on disk.",
      confirmLabel: "Delete",
      danger: true,
      onConfirm: async () => {
        const actorIds = [
          machine.connectionActorId,
          ...machine.agents.map((agent) => agent.spec.actor.id),
        ];
        const result = await ipc.machineRemove(machine.id);
        removeMany(actorIds);
        applyMachines(result.machines);
        pushToast("info", `${machine.name} deleted`);
      },
    });
  };

  const removeAgent = (machine: MachineInfo, agent: MachineAgentInfo) => {
    if (machine.readOnly) {
      pushToast("warn", "remote computers are read-only until server-mediated commands land");
      return;
    }
    openModal({
      type: "confirm",
      title: `Delete ${agent.spec.actor.displayName || agent.spec.actor.id}?`,
      body: `This removes the agent from ${machine.name}.`,
      confirmLabel: "Delete",
      danger: true,
      onConfirm: async () => {
        const result = await ipc.machineAgentRemove(
          machine.id,
          agent.spec.actor.id,
        );
        removeMany([agent.spec.actor.id]);
        applyMachines(result.machines);
        setSelectedId(machine.id);
        pushToast(
          "info",
          `${agent.spec.actor.displayName || agent.spec.actor.id} deleted`,
        );
      },
    });
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 bg-white text-black">
      <aside className="hidden h-full w-72 shrink-0 select-none flex-col border-r-2 border-black bg-brutal-cream md:flex">
        <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black px-5">
          <div className="flex size-icon-header items-center justify-center border-2 border-black bg-brutal-yellow">
            <Monitor size={18} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-base font-black">Computers</div>
            <div className="truncate font-mono text-xs text-black/45">
              {activeWorkspace?.name ?? "Local workspace"}
            </div>
          </div>
          <button
            className="btn-brutal-sm bg-white p-1.5"
            title="Add computer"
            onClick={() => setAddOpen(true)}
          >
            <Plus size={14} />
          </button>
        </header>
        <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto p-2">
          <div className="mb-2 flex items-center justify-between px-2 text-xs font-black uppercase tracking-widest">
            <span>Computers</span>
            <span className="font-mono text-black/40">{machines.length}</span>
          </div>
          {machines.map((machine) => (
            <ComputerListRow
              key={machine.id}
              machine={machine}
              active={machine.id === selectedId}
              onClick={() => setSelectedId(machine.id)}
            />
          ))}
        </div>
      </aside>

      <main className="flex min-h-0 min-w-0 flex-1 flex-col bg-white">
        <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black px-5 md:hidden">
          <Monitor size={18} />
          <div className="min-w-0 flex-1 text-base font-black">Computers</div>
          <button
            className="btn-brutal-sm bg-brutal-pink p-1.5"
            onClick={() => setAddOpen(true)}
          >
            <Plus size={14} />
          </button>
        </header>

        {error && machines.length === 0 ? (
          <div className="m-5 border-2 border-black bg-danger px-4 py-3 text-sm font-bold">
            {error}
          </div>
        ) : machines.length === 0 ? (
          <div className="flex h-full items-center justify-center font-mono text-sm text-black/40">
            {loading ? "Loading computers..." : "No computers configured."}
          </div>
        ) : selected ? (
          <ComputerDetail
            machine={selected}
            canRemove={machines.length > 1}
            loading={loading}
            onRefresh={() => void load(true)}
            onCreateAgent={() => setAgentOpen(true)}
            onRemove={() => removeMachine(selected)}
            onRemoveAgent={(agent) => removeAgent(selected, agent)}
          />
        ) : (
          <div className="flex h-full items-center justify-center font-mono text-sm text-black/40">
            Select a computer
          </div>
        )}
      </main>

      {addOpen && (
        <AddComputerDialog
          onClose={() => setAddOpen(false)}
          onCreate={addMachine}
          onRefresh={() => load(true)}
        />
      )}
      {agentOpen && selected && (
        <AddAgentDialog
          machine={selected}
          onClose={() => setAgentOpen(false)}
          onRefresh={() => void load(true)}
          onCreate={async (input) => {
            await createAgent(selected, input);
            setAgentOpen(false);
          }}
        />
      )}
    </div>
  );
}

function ComputerListRow({
  machine,
  active,
  onClick,
}: {
  machine: MachineInfo;
  active: boolean;
  onClick: () => void;
}) {
  const online = machine.connectionStatus === "online";
  const readOnly = machine.readOnly;
  return (
    <button
      className={clsx(
        "mb-2 flex w-full items-center gap-3 border-2 px-3 py-2 text-left transition-colors",
        active
          ? "border-black bg-brutal-pink shadow-brutal-sm"
          : "border-transparent hover:border-black hover:bg-white hover:shadow-brutal-sm",
      )}
      onClick={onClick}
    >
      <div
        className={clsx(
          "flex h-9 w-9 shrink-0 items-center justify-center border-2 border-black",
          online ? "bg-brutal-lime" : "bg-white",
        )}
      >
        {online ? <Wifi size={16} /> : <WifiOff size={16} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1">
          <span className="truncate text-sm font-black">{machine.name}</span>
          {readOnly && (
            <span className="chip-brutal bg-white text-[9px]">read-only</span>
          )}
        </div>
        <div className="truncate font-mono text-[11px] text-black/45">
          daemon {online ? "online" : machine.connectionStatus}
        </div>
      </div>
    </button>
  );
}

function ComputerDetail({
  machine,
  canRemove,
  loading,
  onRefresh,
  onCreateAgent,
  onRemove,
  onRemoveAgent,
}: {
  machine: MachineInfo;
  canRemove: boolean;
  loading: boolean;
  onRefresh: () => void;
  onCreateAgent: () => void;
  onRemove: () => void;
  onRemoveAgent: (agent: MachineAgentInfo) => void;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [commandOpen, setCommandOpen] = useState(machine.connectionStatus !== "online");
  const online = machine.connectionStatus === "online";
  const readOnly = machine.readOnly;
  const runtimes = runtimeRows(machine);

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("info", `${label} copied`);
    } catch {
      pushToast("info", value);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black bg-white px-5">
        <div
          className={clsx(
            "flex h-10 w-10 items-center justify-center border-2 border-black",
            online ? "bg-brutal-lime" : "bg-brutal-cyan",
          )}
        >
          {online ? <Wifi size={18} /> : <WifiOff size={18} />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-base font-black">{machine.name}</span>
            {readOnly && (
              <span className="chip-brutal bg-white text-[10px]">read-only</span>
            )}
          </div>
          <div className="truncate font-mono text-xs text-black/45">
            {machine.id} · daemon {online ? "connected" : machine.connectionStatus}
          </div>
        </div>
        <button
          className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs disabled:opacity-40"
          disabled={loading}
          onClick={onRefresh}
        >
          <RefreshCw size={13} /> Scan
        </button>
        <button
          className="btn-brutal-sm gap-1 bg-brutal-pink px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:bg-black/10"
          disabled={readOnly || machine.providers.length === 0}
          title={
            readOnly
              ? "Remote computers are read-only until server-mediated commands land"
              : machine.providers.length === 0
                ? "Install a supported runtime before creating agents"
                : "Create agent"
          }
          onClick={onCreateAgent}
        >
          <UserPlus size={13} /> Create
        </button>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto">
        <section className="border-b border-black/10 p-5">
          <div className="grid gap-3 md:grid-cols-3">
            <Metric label="Status" value={online ? "Connected" : machine.connectionStatus} />
            <Metric label="Agents" value={`${machine.onlineAgentCount}/${machine.agentCount}`} />
            <Metric label="Runtimes" value={String(machine.providers.length)} />
          </div>
        </section>

        <InfoSection title="Info">
          <div className="grid max-w-4xl gap-4 text-sm md:grid-cols-2">
            <InfoCell icon={Server} label="Daemon Actor" value={machine.connectionActorId} />
            <InfoCell icon={Server} label="Source" value={machine.source} />
            <InfoCell icon={HardDrive} label="Data Root" value={machine.dataRoot} />
            <InfoCell icon={Monitor} label="Kind" value={machine.kind} />
            <InfoCell icon={Cpu} label="Config" value={machine.configDir} />
          </div>
        </InfoSection>

        <InfoSection title="Detected Runtimes">
          <div className="grid max-w-4xl gap-2 md:grid-cols-2">
            {runtimes.map((runtime) => (
              <RuntimeRow key={runtime.id} runtime={runtime} />
            ))}
          </div>
        </InfoSection>

        <InfoSection title={`Agents on this computer (${machine.agentCount})`}>
          <div className="mb-3 flex flex-wrap gap-2">
            <button
              className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs disabled:opacity-40"
              disabled
            >
              <CheckCircle2 size={13} /> Start All
            </button>
            <button
              className="btn-brutal-sm gap-1 bg-brutal-pink px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:bg-black/10"
              disabled={readOnly || machine.providers.length === 0}
              onClick={onCreateAgent}
            >
              <UserPlus size={13} /> Create
            </button>
          </div>
          {machine.agents.length === 0 ? (
            <div className="border-2 border-dashed border-black/25 px-5 py-8 text-center font-mono text-sm text-black/40">
              No agents on this computer.
            </div>
          ) : (
            <div className="max-w-4xl border-2 border-black bg-white">
              {machine.agents.map((agent) => (
                <AgentRow
                  key={agent.spec.actor.id}
                  agent={agent}
                  readOnly={readOnly}
                  onRemove={() => onRemoveAgent(agent)}
                />
              ))}
            </div>
          )}
        </InfoSection>

        <InfoSection title="Agent Workspaces">
          <div className="max-w-4xl border-2 border-dashed border-black/25 px-5 py-6 text-sm">
            <button
              className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs"
              onClick={onRefresh}
            >
              <RefreshCw size={13} /> Scan
            </button>
          </div>
        </InfoSection>

        {!readOnly && (
          <InfoSection title="Connect Computer">
            <div className="max-w-4xl border-2 border-black bg-brutal-cream">
              <div className="flex items-center gap-2 border-b-2 border-black bg-white px-3 py-2">
                <Terminal size={14} />
                <span className="text-xs font-black uppercase tracking-widest text-black/50">
                  Daemon Command
                </span>
                <button
                  className="btn-brutal-sm ml-auto gap-1 bg-brutal-pink px-2 py-1 text-xs"
                  onClick={() => void copy(machine.setupScript, "daemon script")}
                >
                  <Copy size={12} /> Copy
                </button>
                <button
                  className="btn-brutal-sm bg-white px-2 py-1 text-xs"
                  onClick={() => setCommandOpen((open) => !open)}
                >
                  {commandOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                </button>
              </div>
              {commandOpen && (
                <textarea
                  className="block h-36 w-full resize-none border-0 bg-brutal-cream p-3 font-mono text-xs outline-none"
                  readOnly
                  value={machine.setupScript}
                />
              )}
            </div>
          </InfoSection>
        )}

        <InfoSection title="Actions">
          <button
            className="btn-brutal gap-2 bg-danger px-4 py-2 text-sm disabled:cursor-not-allowed disabled:opacity-40"
            disabled={readOnly || !canRemove}
            onClick={onRemove}
          >
            <Trash2 size={14} /> Delete Computer
          </button>
        </InfoSection>
      </div>
    </div>
  );
}

function RuntimeRow({
  runtime,
}: {
  runtime: {
    id: string;
    name: string;
    installed: boolean;
    provider?: AgentProviderSummary;
  };
}) {
  return (
    <div
      className={clsx(
        "flex min-w-0 items-center gap-3 border-2 border-black px-3 py-2",
        runtime.installed ? "bg-white" : "bg-black/5 text-black/45",
      )}
    >
      <span
        className={clsx(
          "h-2.5 w-2.5 shrink-0 rounded-full border border-black",
          runtime.installed ? "bg-brutal-lime" : "bg-black/20",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-black">{runtime.name}</div>
        <div className="truncate font-mono text-[11px] text-black/45">
          {runtime.installed
            ? runtime.provider?.command
            : "not installed"}
        </div>
      </div>
    </div>
  );
}

function AgentRow({
  agent,
  readOnly,
  onRemove,
}: {
  agent: MachineAgentInfo;
  readOnly: boolean;
  onRemove: () => void;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const online = agent.status === "online";
  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("info", `${label} copied`);
    } catch {
      pushToast("info", value);
    }
  };
  const openProfile = async () => {
    if (readOnly) {
      pushToast("warn", "remote profile files are read-only from this GUI");
      return;
    }
    try {
      await ipc.openLocalPath(agent.profilePath);
      pushToast("info", "profile opened");
    } catch (e) {
      pushToast("error", e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <div className="border-b border-black/10 px-3 py-3 last:border-b-0">
      <div className="flex items-center gap-3">
        <PixelAvatar
          id={agent.spec.actor.id}
          label={agent.spec.actor.displayName}
          size={24}
        />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-black">
            {agent.spec.actor.displayName || agent.spec.actor.id}
          </div>
          <div className="truncate font-mono text-[11px] text-black/45">
            {agentProviderName(agent)} · {agentModelLabel(agent)}
          </div>
        </div>
        <span
          className={clsx(
            "h-2.5 w-2.5 shrink-0 rounded-full border border-black",
            online ? "bg-brutal-lime" : "bg-black/20",
          )}
          title={agent.status}
        />
        <button
          className="btn-brutal-sm bg-white p-1 disabled:cursor-not-allowed disabled:opacity-40"
          disabled={readOnly}
          title={
            readOnly
              ? "Remote agents are read-only until server-mediated commands land"
              : "Remove agent"
          }
          onClick={onRemove}
        >
          <Trash2 size={12} />
        </button>
      </div>
      <div className="mt-2 grid min-w-0 gap-2 pl-9 md:grid-cols-[minmax(0,1fr)_auto]">
        <div className="flex min-w-0 items-center gap-2 border border-black/20 bg-brutal-cream px-2 py-1.5">
          <HardDrive size={12} className="shrink-0 text-black/45" />
          <span className="truncate font-mono text-[11px] text-black/65">
            {agent.profilePath}
          </span>
        </div>
        <div className="flex flex-wrap gap-1">
          <button
            className="btn-brutal-sm bg-white p-1.5"
            disabled={readOnly}
            title={
              readOnly
                ? "Remote profile paths cannot be opened locally"
                : "Open profile folder"
            }
            onClick={() => void openProfile()}
          >
            <FolderOpen size={12} />
          </button>
          <button
            className="btn-brutal-sm bg-white p-1.5"
            title="Copy profile path"
            onClick={() => void copy(agent.profilePath, "profile path")}
          >
            <Copy size={12} />
          </button>
          <button
            className="btn-brutal-sm gap-1 bg-white px-2 py-1 text-[11px]"
            title={agent.identityPath}
            onClick={() => void copy(agent.identityPath, "identity.md path")}
          >
            <FileText size={12} /> Identity
          </button>
          <button
            className="btn-brutal-sm gap-1 bg-white px-2 py-1 text-[11px]"
            title={agent.soulPath}
            onClick={() => void copy(agent.soulPath, "soul.md path")}
          >
            <FileText size={12} /> Soul
          </button>
        </div>
      </div>
    </div>
  );
}

function AddComputerDialog({
  onClose,
  onCreate,
  onRefresh,
}: {
  onClose: () => void;
  onCreate: (input: MachineCreateInput) => Promise<MachineInfo>;
  onRefresh: () => Promise<MachineInfo[]>;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [name, setName] = useState("my-computer");
  const [dataRoot, setDataRoot] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [created, setCreated] = useState<MachineInfo | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    if (busy) return;
    setBusy(true);
    try {
      setCreated(
        await onCreate({
          name: name.trim() || "my-computer",
          dataRoot: dataRoot.trim(),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  const check = async (notify = true) => {
    if (!created) return;
    if (notify) setBusy(true);
    try {
      const rows = await onRefresh();
      const next = rows.find((machine) => machine.id === created.id);
      if (next) setCreated(next);
      if (notify) {
        const status = next?.connectionStatus ?? "unknown";
        pushToast(
          status === "online" ? "info" : "warn",
          `${created.name}: ${status}`,
        );
      }
    } finally {
      if (notify) setBusy(false);
    }
  };

  useEffect(() => {
    if (!created) return;
    const interval = window.setInterval(() => void check(false), 3000);
    return () => window.clearInterval(interval);
  }, [created?.id]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/60 p-4">
      <section className="card-brutal w-full max-w-xl p-6">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-black uppercase">
            {created ? "Connect Computer" : "Add Computer"}
          </h2>
          <button className="btn-brutal-sm bg-white p-1" onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        {!created ? (
          <div className="space-y-4">
            <button className="flex w-full items-center gap-3 border-2 border-black bg-brutal-yellow px-4 py-3 text-left shadow-brutal-sm">
              <Monitor size={20} />
              <span className="min-w-0 flex-1">
                <span className="block font-black">Your Computer</span>
                <span className="block text-sm text-black/55">
                  Run agents on this computer
                </span>
              </span>
              <CheckCircle2 size={18} />
            </button>
            <div className="flex w-full items-center gap-3 border-2 border-black bg-black/5 px-4 py-3 text-left text-black/45">
              <Server size={20} />
              <span className="min-w-0 flex-1">
                <span className="block font-black">Cloud Computer</span>
                <span className="block text-sm">Coming soon</span>
              </span>
            </div>
            <Field label="Name">
              <input
                className="input-brutal w-full"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </Field>
            <button
              className="flex items-center gap-1 text-sm font-black uppercase tracking-wide text-black/60 hover:text-black"
              onClick={() => setAdvanced((open) => !open)}
            >
              {advanced ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
              Advanced
            </button>
            {advanced && (
              <Field label="Data Root" optional>
                <input
                  className="input-brutal w-full"
                  placeholder="Leave blank for the default data root"
                  value={dataRoot}
                  onChange={(e) => setDataRoot(e.target.value)}
                />
              </Field>
            )}
            <div className="flex justify-end gap-3 pt-2">
              <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onClose}>
                Cancel
              </button>
              <button
                className="btn-brutal bg-brutal-pink px-4 py-2 text-sm disabled:bg-black/10"
                disabled={busy}
                onClick={() => void create()}
              >
                Next
              </button>
            </div>
          </div>
        ) : (
          <div className="space-y-4">
            <textarea
              className="h-44 w-full resize-none border-2 border-black bg-brutal-cream p-3 font-mono text-xs outline-none"
              readOnly
              value={created.setupScript}
            />
            <div className="grid gap-2 sm:grid-cols-3">
              <StepPill index={1} label="Config" state="ready" done />
              <StepPill
                index={2}
                label="Daemon"
                state={created.connectionStatus === "online" ? "running" : "start"}
                done={created.connectionStatus === "online"}
              />
              <StepPill
                index={3}
                label="Link"
                state={
                  created.connectionStatus === "online"
                    ? "connected"
                    : created.connectionStatus
                }
                done={created.connectionStatus === "online"}
              />
            </div>
            <div className="flex flex-wrap justify-end gap-3 pt-2">
              <CopyButton value={created.setupScript} label="daemon script" />
              <button
                className="btn-brutal bg-brutal-yellow px-4 py-2 text-sm disabled:bg-black/10"
                disabled={busy}
                onClick={() => void check(true)}
              >
                Check Link
              </button>
              <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onClose}>
                Done
              </button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function AddAgentDialog({
  machine,
  onClose,
  onRefresh,
  onCreate,
}: {
  machine: MachineInfo;
  onClose: () => void;
  onRefresh: () => void;
  onCreate: (input: AgentCreateInput) => void | Promise<void>;
}) {
  const firstProvider = machine.providers[0] ?? null;
  const [providerId, setProviderId] = useState(firstProvider?.id ?? "");
  const [name, setName] = useState("");
  const [actorId, setActorId] = useState("");
  const [description, setDescription] = useState("");
  const [model, setModel] = useState(defaultModelForProvider(firstProvider));
  const [reasoningEffort, setReasoningEffort] = useState("medium");
  const [autostart, setAutostart] = useState(false);
  const [runtimeOpen, setRuntimeOpen] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const selectedProvider =
    machine.providers.find((provider) => provider.id === providerId) ??
    firstProvider;
  const modelChoices = modelChoicesForProvider(selectedProvider);
  const runtimes = runtimeRows(machine);

  const create = async () => {
    if (!name.trim() || !providerId || busy) return;
    setBusy(true);
    try {
      await onCreate({
        providerId,
        actorId: actorId.trim(),
        name: name.trim(),
        description: description.trim(),
        model: model.trim(),
        reasoningEffort: reasoningEffort.trim(),
        autostart,
      });
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const nextProvider = machine.providers.find(
      (provider) => provider.id === providerId,
    );
    setModel(defaultModelForProvider(nextProvider));
    if (nextProvider?.id !== "codex") setReasoningEffort("");
    if (nextProvider?.id === "codex" && !reasoningEffort) {
      setReasoningEffort("medium");
    }
  }, [providerId, machine.providers]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/60 p-4">
      <section className="card-brutal w-full max-w-lg p-6">
        <div className="mb-4 flex items-center justify-between">
          <div className="min-w-0">
            <h2 className="text-lg font-black uppercase">Create Agent</h2>
            <div className="truncate font-mono text-xs text-black/45">
              {machine.name} ({machine.id})
            </div>
          </div>
          <button className="btn-brutal-sm bg-white p-1" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <div className="space-y-4">
          <Field label="Computer" required>
            <div className="input-brutal flex w-full items-center gap-2 text-sm">
              <Monitor size={14} />
              <span className="min-w-0 flex-1 truncate">
                {machine.name} ({machine.id})
              </span>
            </div>
          </Field>
          <Field label="Name" required>
            <input
              autoFocus
              className="input-brutal w-full"
              placeholder="e.g. Reviewer"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label={`Initial Identity (${description.length}/3000)`} optional>
            <textarea
              className="input-brutal w-full resize-none"
              maxLength={3000}
              rows={3}
              placeholder="Seed identity.md on first start"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
          <Field label="Runtime" required>
            <div className="relative">
              <button
                className="input-brutal flex w-full items-center justify-between gap-2 text-sm"
                onClick={() => setRuntimeOpen((open) => !open)}
              >
                <span className="min-w-0 flex-1 text-left">
                  <span className="block truncate font-black">
                    {selectedProvider?.name ?? "No runtime installed"}
                  </span>
                  {selectedProvider && (
                    <span className="block truncate font-mono text-[11px] text-black/45">
                      {selectedProvider.command}
                    </span>
                  )}
                </span>
                <ChevronDown size={14} />
              </button>
              {runtimeOpen && (
                <div className="absolute left-0 right-0 top-full z-10 mt-1 max-h-56 overflow-y-auto border-2 border-black bg-white shadow-brutal">
                  {runtimes.map((runtime) => (
                    <button
                      key={runtime.id}
                      className={clsx(
                        "block w-full px-3 py-2 text-left",
                        runtime.installed
                          ? "hover:bg-brutal-yellow"
                          : "cursor-not-allowed bg-black/5 text-black/45",
                      )}
                      disabled={!runtime.installed}
                      onClick={() => {
                        if (!runtime.installed) return;
                        setProviderId(runtime.id);
                        setRuntimeOpen(false);
                      }}
                    >
                      <span className="block truncate text-sm font-black">
                        {runtime.name}
                      </span>
                      <span className="block truncate font-mono text-[11px] text-black/45">
                        {runtime.installed
                          ? runtime.provider?.command
                          : "not installed"}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </Field>
          <button
            className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs"
            onClick={onRefresh}
          >
            <RefreshCw size={13} /> Rescan Runtimes
          </button>
          {modelChoices.length > 0 ? (
            <Field label="Model">
              <SelectLike
                value={
                  modelChoices.find((choice) => choice.id === model)?.label ||
                  model ||
                  "Default"
                }
                options={modelChoices}
                onPick={setModel}
              />
            </Field>
          ) : (
            <Field label="Model" optional>
              <input
                className="input-brutal w-full"
                placeholder="Optional model id"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              />
            </Field>
          )}
          {selectedProvider?.id === "codex" && (
            <Field label="Reasoning Effort">
              <SelectLike
                value={reasoningEffort || "medium"}
                options={REASONING_CHOICES.map((id) => ({ id, label: id }))}
                onPick={setReasoningEffort}
              />
            </Field>
          )}
          <button
            className="flex items-center gap-1 text-sm font-black uppercase tracking-wide text-black/60 hover:text-black"
            onClick={() => setAdvanced((open) => !open)}
          >
            {advanced ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
            Advanced
          </button>
          {advanced && (
            <div className="space-y-3">
              <Field label="Actor ID" optional>
                <input
                  className="input-brutal w-full"
                  placeholder="Leave blank to generate"
                  value={actorId}
                  onChange={(e) => setActorId(e.target.value)}
                />
              </Field>
              <label className="flex items-center gap-2 text-sm font-bold">
                <input
                  type="checkbox"
                  checked={autostart}
                  onChange={(e) => setAutostart(e.target.checked)}
                  className="h-4 w-4 accent-black"
                />
                Autostart
              </label>
            </div>
          )}
          <div className="flex justify-end gap-3 pt-2">
            <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onClose}>
              Cancel
            </button>
            <button
              className="btn-brutal bg-brutal-pink px-4 py-2 text-sm disabled:bg-black/10"
              disabled={!name.trim() || !providerId || busy}
              onClick={() => void create()}
            >
              Create Agent
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const pushToast = useUI((s) => s.pushToast);
  return (
    <button
      className="btn-brutal bg-brutal-pink px-4 py-2 text-sm"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          pushToast("info", `${label} copied`);
        } catch {
          pushToast("info", value);
        }
      }}
    >
      Copy Command
    </button>
  );
}

function StepPill({
  index,
  label,
  state,
  done,
}: {
  index: number;
  label: string;
  state: string;
  done?: boolean;
}) {
  return (
    <div
      className={clsx(
        "flex min-w-0 items-center gap-2 border-2 border-black px-2 py-2",
        done ? "bg-brutal-lime" : "bg-white",
      )}
    >
      <span className="flex h-5 w-5 shrink-0 items-center justify-center border-2 border-black bg-white text-[11px] font-black">
        {done ? <CheckCircle2 size={13} /> : index}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs font-black uppercase">
          {label}
        </span>
        <span className="block truncate font-mono text-[11px] text-black/45">
          {state}
        </span>
      </span>
    </div>
  );
}

function Field({
  label,
  required,
  optional,
  children,
}: {
  label: string;
  required?: boolean;
  optional?: boolean;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="mb-1 block text-sm font-black uppercase tracking-wide">
        {label} {required && <span className="text-brutal-pink">*</span>}
        {optional && <span className="text-black/40 normal-case">(optional)</span>}
      </label>
      {children}
    </div>
  );
}

function SelectLike({
  value,
  options,
  onPick,
}: {
  value: string;
  options: Array<{ id: string; label?: string | null }>;
  onPick: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        className="input-brutal flex w-full items-center justify-between gap-2 text-sm"
        onClick={() => setOpen((value) => !value)}
      >
        <span className="truncate">{value}</span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <div className="absolute left-0 right-0 top-full z-10 mt-1 border-2 border-black bg-white shadow-brutal">
          {options.map((option) => (
            <button
              key={option.id}
              className="block w-full px-3 py-2 text-left text-sm font-bold hover:bg-brutal-yellow"
              onClick={() => {
                onPick(option.id);
                setOpen(false);
              }}
            >
              {option.label || option.id}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function InfoSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="border-b border-black/10 px-5 py-4">
      <div className="mb-3 text-xs font-black uppercase tracking-widest text-black/45">
        {title}
      </div>
      {children}
    </section>
  );
}

function InfoCell({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Server;
  label: string;
  value: string;
}) {
  return (
    <div className="flex min-w-0 items-start gap-2">
      <Icon className="mt-0.5 shrink-0 text-black/35" size={15} />
      <div className="min-w-0">
        <div className="text-xs font-black uppercase tracking-widest text-black/45">
          {label}
        </div>
        <div className="truncate font-mono text-xs">{value}</div>
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="border-2 border-black bg-white p-3">
      <div className="font-mono text-[11px] uppercase tracking-wider text-black/45">
        {label}
      </div>
      <div className="truncate font-black">{value}</div>
    </div>
  );
}

function runtimeRows(machine: MachineInfo) {
  const byId = new Map(machine.providers.map((provider) => [provider.id, provider]));
  const rows = KNOWN_RUNTIMES.map((runtime) => ({
    ...runtime,
    installed: byId.has(runtime.id),
    provider: byId.get(runtime.id),
  }));
  for (const provider of machine.providers) {
    if (!KNOWN_RUNTIMES.some((runtime) => runtime.id === provider.id)) {
      rows.push({
        id: provider.id,
        name: provider.name || provider.id,
        installed: true,
        provider,
      });
    }
  }
  return rows;
}

function defaultModelForProvider(provider: AgentProviderSummary | null | undefined) {
  if (!provider) return "";
  if (provider.defaultModel) return provider.defaultModel;
  if (provider.id === "codex") return CODEX_MODELS[0].id;
  return "";
}

function modelChoicesForProvider(provider: AgentProviderSummary | null | undefined) {
  if (!provider) return [];
  if (provider.modelChoices?.length) return provider.modelChoices;
  if (provider.id === "codex") return CODEX_MODELS;
  return [];
}

function agentProviderName(agent: AgentInfo) {
  const meta = agent.spec.actor._meta ?? {};
  return typeof meta.providerName === "string" ? meta.providerName : "provider";
}

function agentModelLabel(agent: AgentInfo) {
  const model = agent.spec.models?.default ?? agent.spec.transport.model;
  return model || agent.spec.transport.kind;
}

interface MachineCreateInput {
  name: string;
  dataRoot?: string;
}

interface AgentCreateInput {
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  model: string;
  reasoningEffort: string;
  autostart: boolean;
}
