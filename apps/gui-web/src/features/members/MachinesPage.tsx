import { useEffect, useState, type ReactNode } from "react";
import clsx from "clsx";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  Monitor,
  Plus,
  RefreshCw,
  Terminal,
  Trash2,
  UserPlus,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { AgentInfo, AgentProviderSummary, MachineInfo } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { PixelAvatar } from "@/features/common/PixelAvatar";

export function MachinesPage() {
  const upsertMany = useActors((s) => s.upsertMany);
  const pushToast = useUI((s) => s.pushToast);
  const openModal = useUI((s) => s.openModal);
  const activeWorkspace = useWorkspaces(
    (s) => s.workspaces.find((workspace) => workspace.id === s.activeId) ?? null,
  );
  const [machines, setMachines] = useState<MachineInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const applyMachines = (rows: MachineInfo[]) => {
    setMachines(rows);
    upsertMany(
      rows.flatMap((machine) =>
        machine.agents.map((agent) => agent.spec.actor),
      ),
    );
  };

  const load = async (check = false, showLoading = true) => {
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const result = check ? await ipc.machineCheck() : await ipc.machineList();
      applyMachines(result.machines);
      return result.machines;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      if (showLoading) pushToast("error", `machine/list failed: ${message}`);
      return machines;
    } finally {
      if (showLoading) setLoading(false);
    }
  };

  useEffect(() => {
    void load(true);
    const interval = window.setInterval(() => void load(true, false), 4000);
    return () => window.clearInterval(interval);
  }, [activeWorkspace?.id]);

  const addMachine = async (input: MachineCreateInput) => {
    const before = new Set(machines.map((machine) => machine.id));
    const result = await ipc.machineCreate(input);
    applyMachines(result.machines);
    const created =
      result.machines.find((machine) => !before.has(machine.id)) ??
      result.machines[result.machines.length - 1];
    pushToast("info", `${created.name} configured`);
    return created;
  };

  const createAgent = async (machine: MachineInfo, input: AgentCreateInput) => {
    const result = await ipc.machineAgentCreate({
      machineId: machine.id,
      providerId: input.providerId,
      actorId: input.actorId,
      name: input.name,
      description: input.description,
      model: input.model,
      autostart: input.autostart,
    });
    applyMachines(result.machines);
    pushToast("info", `${input.name} registered on ${machine.name}`);
  };

  const removeMachine = (machine: MachineInfo) => {
    if (machines.length <= 1) {
      pushToast("warn", "At least one machine is required");
      return;
    }
    openModal({
      type: "confirm",
      title: `Remove ${machine.name}?`,
      body: "This removes the local machine profile only. Agent spec files and data directories stay on disk.",
      confirmLabel: "Remove",
      danger: true,
      onConfirm: async () => {
        const result = await ipc.machineRemove(machine.id);
        applyMachines(result.machines);
        pushToast("info", `${machine.name} removed`);
      },
    });
  };

  const removeAgent = (machine: MachineInfo, agent: AgentInfo) => {
    openModal({
      type: "confirm",
      title: `Remove ${agent.spec.actor.displayName || agent.spec.actor.id}?`,
      body: `This removes the agent registration from ${machine.name}.`,
      confirmLabel: "Remove",
      danger: true,
      onConfirm: async () => {
        const result = await ipc.machineAgentRemove(
          machine.id,
          agent.spec.actor.id,
        );
        applyMachines(result.machines);
        pushToast(
          "info",
          `${agent.spec.actor.displayName || agent.spec.actor.id} removed`,
        );
      },
    });
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-white text-black">
      <header className="flex h-panel-header shrink-0 items-center gap-3 border-b-2 border-black px-5">
        <div className="flex size-icon-header items-center justify-center border-2 border-black bg-brutal-yellow">
          <Monitor size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-base font-black">Machines</div>
          <div className="truncate font-mono text-xs text-black/45">
            {activeWorkspace
              ? `${activeWorkspace.name}: setup scripts, live serve state, and local agents`
              : "setup scripts, live serve state, and local agents"}
          </div>
        </div>
        <button
          className="btn-brutal-sm gap-1 bg-brutal-pink px-3 py-1.5 text-xs"
          onClick={() => setAddOpen(true)}
        >
          <Plus size={13} /> Add Machine
        </button>
        <button
          className="btn-brutal-sm gap-1 bg-white px-3 py-1.5 text-xs"
          disabled={loading}
          onClick={() => void load(true)}
        >
          <RefreshCw size={13} /> Check
        </button>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-auto p-5">
        {error && machines.length === 0 ? (
          <div className="border-2 border-black bg-danger px-4 py-3 text-sm font-bold">
            {error}
          </div>
        ) : machines.length === 0 ? (
          <div className="mt-10 text-center font-mono text-sm text-black/40">
            {loading ? "Loading machines..." : "No machines configured."}
          </div>
        ) : (
          <div className="grid gap-4 xl:grid-cols-2">
            {machines.map((machine) => (
              <MachineCard
                key={machine.id}
                machine={machine}
                canRemove={machines.length > 1}
                onRefresh={() => void load(true)}
                onCreateAgent={(input) => createAgent(machine, input)}
                onRemove={() => removeMachine(machine)}
                onRemoveAgent={(agent) => removeAgent(machine, agent)}
              />
            ))}
          </div>
        )}
      </div>

      {addOpen && (
        <AddMachineDialog
          onClose={() => setAddOpen(false)}
          onCreate={addMachine}
          onRefresh={() => load(true)}
        />
      )}
    </div>
  );
}

function MachineCard({
  machine,
  canRemove,
  onRefresh,
  onCreateAgent,
  onRemove,
  onRemoveAgent,
}: {
  machine: MachineInfo;
  canRemove: boolean;
  onRefresh: () => void;
  onCreateAgent: (input: AgentCreateInput) => void | Promise<void>;
  onRemove: () => void;
  onRemoveAgent: (agent: AgentInfo) => void;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [scriptOpen, setScriptOpen] = useState(false);
  const [agentOpen, setAgentOpen] = useState(false);
  const online = machine.connectionStatus === "online";

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("info", `${label} copied`);
    } catch {
      pushToast("info", value);
    }
  };

  return (
    <section className="card-brutal flex min-h-[32rem] flex-col p-4">
      <div className="mb-3 flex items-center gap-3">
        <div
          className={clsx(
            "flex h-10 w-10 items-center justify-center border-2 border-black",
            online ? "bg-brutal-lime" : "bg-brutal-cyan",
          )}
        >
          {online ? <Wifi size={18} /> : <WifiOff size={18} />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate font-black">{machine.name}</div>
          <div className="font-mono text-xs text-black/45">
            {machine.kind} - {machine.connectionStatus}
          </div>
        </div>
        <button
          className="btn-brutal-sm bg-white p-1.5 disabled:cursor-not-allowed disabled:opacity-40"
          title={canRemove ? "Remove machine" : "Last machine cannot be removed"}
          disabled={!canRemove}
          onClick={onRemove}
        >
          <Trash2 size={14} />
        </button>
      </div>

      <div className="grid grid-cols-3 gap-2 text-sm">
        <Metric label="Setup" value={machine.setupStatus} />
        <Metric
          label="Agents"
          value={`${machine.onlineAgentCount}/${machine.agentCount}`}
        />
        <Metric label="Providers" value={String(machine.providers.length)} />
      </div>

      <div className="mt-4 grid gap-2 sm:grid-cols-3">
        <StepPill index={1} label="Config" state={machine.setupStatus} done />
        <StepPill
          index={2}
          label="Serve"
          state={online ? "running" : "start"}
          done={online}
        />
        <StepPill
          index={3}
          label="Link"
          state={online ? "connected" : "waiting"}
          done={online}
        />
      </div>

      <div className="mt-4 space-y-2">
        <PathRow label="Specs" value={machine.specsDir} onCopy={copy} />
        <PathRow label="Data" value={machine.dataRoot} onCopy={copy} />
      </div>

      <div className="mt-4 border-2 border-black bg-white">
        <div className="flex items-center gap-2 border-b-2 border-black px-3 py-2">
          <span className="text-xs font-black uppercase tracking-widest text-black/45">
            Providers
          </span>
          <span className="font-mono text-xs text-black/40">
            {machine.providers.length}
          </span>
        </div>
        {machine.providers.length === 0 ? (
          <div className="px-3 py-3 font-mono text-xs text-black/40">
            No provider specs in this machine.
          </div>
        ) : (
          <div className="max-h-32 overflow-y-auto">
            {machine.providers.map((provider) => (
              <ProviderRow key={provider.id} provider={provider} />
            ))}
          </div>
        )}
      </div>

      <div className="mt-4 border-2 border-black bg-brutal-cream">
        <div className="flex items-center gap-2 border-b-2 border-black bg-white px-3 py-2">
          <Terminal size={14} />
          <span className="text-xs font-black uppercase tracking-widest text-black/50">
            Setup Script
          </span>
          <button
            className="btn-brutal-sm ml-auto gap-1 bg-brutal-pink px-2 py-1 text-xs"
            onClick={() => void copy(machine.setupScript, "setup script")}
          >
            <Copy size={12} /> Copy
          </button>
          <button
            className="btn-brutal-sm bg-white px-2 py-1 text-xs"
            onClick={() => setScriptOpen((open) => !open)}
          >
            {scriptOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          </button>
        </div>
        {scriptOpen && (
          <textarea
            className="block h-36 w-full resize-none border-0 bg-brutal-cream p-3 font-mono text-xs outline-none"
            readOnly
            value={machine.setupScript}
          />
        )}
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        <button
          className="btn-brutal-sm gap-2 bg-white px-3 py-1.5 text-xs"
          onClick={() => void copy(machine.serveCommand, "serve command")}
        >
          <Terminal size={13} /> Copy Command
        </button>
        <button
          className="btn-brutal-sm gap-2 bg-brutal-yellow px-3 py-1.5 text-xs"
          onClick={onRefresh}
        >
          <RefreshCw size={13} /> Check Link
        </button>
      </div>

      <div className="mt-4 flex min-h-0 flex-1 flex-col border-2 border-black bg-white">
        <div className="flex items-center gap-2 border-b-2 border-black px-3 py-2">
          <span className="text-xs font-black uppercase tracking-widest text-black/45">
            Agents
          </span>
          <span className="font-mono text-xs text-black/40">
            {machine.agentCount}
          </span>
          <button
            className="btn-brutal-sm ml-auto gap-1 bg-brutal-pink px-2 py-1 text-xs disabled:cursor-not-allowed disabled:bg-black/10"
            disabled={machine.providers.length === 0}
            title={
              machine.providers.length === 0
                ? "Add a provider spec before adding agents"
                : "Add agent actor"
            }
            onClick={() => setAgentOpen(true)}
          >
            <UserPlus size={12} /> Add
          </button>
        </div>
        {machine.agents.length === 0 ? (
          <div className="flex min-h-[7rem] items-center justify-center px-3 py-4 text-center font-mono text-sm text-black/40">
            No agent actors in specs.
          </div>
        ) : (
          <div className="max-h-56 overflow-y-auto">
            {machine.agents.map((agent) => (
              <AgentRow
                key={agent.spec.actor.id}
                agent={agent}
                onRemove={() => onRemoveAgent(agent)}
              />
            ))}
          </div>
        )}
      </div>

      {agentOpen && (
        <AddAgentDialog
          machine={machine}
          onClose={() => setAgentOpen(false)}
          onCreate={async (input) => {
            await onCreateAgent(input);
            setAgentOpen(false);
          }}
        />
      )}
    </section>
  );
}

function ProviderRow({ provider }: { provider: AgentProviderSummary }) {
  return (
    <div className="border-b border-black/10 px-3 py-2 last:border-b-0">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-black">
          {provider.name || provider.id}
        </span>
        <span className="border border-black bg-brutal-cyan px-1.5 py-0.5 font-mono text-[10px] font-black">
          {provider.transportKind}
        </span>
        <span className="font-mono text-[11px] text-black/40">
          {provider.actorCount}
        </span>
      </div>
      <div className="truncate font-mono text-[11px] text-black/45">
        {[provider.command, ...(provider.args ?? [])].filter(Boolean).join(" ")}
      </div>
    </div>
  );
}

function AgentRow({
  agent,
  onRemove,
}: {
  agent: AgentInfo;
  onRemove: () => void;
}) {
  const online = agent.status === "online";
  return (
    <div className="flex items-center gap-2 border-b border-black/10 px-3 py-2 last:border-b-0">
      <PixelAvatar
        id={agent.spec.actor.id}
        label={agent.spec.actor.displayName}
        size={22}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-black">
          {agent.spec.actor.displayName || agent.spec.actor.id}
        </div>
        <div className="truncate font-mono text-[11px] text-black/45">
          {agentProviderName(agent)} - {agent.spec.transport.kind}
        </div>
      </div>
      <span
        className={clsx(
          "h-2.5 w-2.5 shrink-0 rounded-full border border-black",
          online ? "bg-brutal-lime" : "bg-black/20",
        )}
        title={agent.status}
      />
      <button className="btn-brutal-sm bg-white p-1" onClick={onRemove}>
        <Trash2 size={12} />
      </button>
    </div>
  );
}

function AddMachineDialog({
  onClose,
  onCreate,
  onRefresh,
}: {
  onClose: () => void;
  onCreate: (input: MachineCreateInput) => Promise<MachineInfo>;
  onRefresh: () => Promise<MachineInfo[]>;
}) {
  const pushToast = useUI((s) => s.pushToast);
  const [name, setName] = useState("");
  const [specsDir, setSpecsDir] = useState("");
  const [dataRoot, setDataRoot] = useState("");
  const [created, setCreated] = useState<MachineInfo | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      setCreated(
        await onCreate({
          name: name.trim(),
          specsDir: specsDir.trim(),
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
            {created ? "Start Machine" : "Add Machine"}
          </h2>
          <button className="btn-brutal-sm bg-white p-1" onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        {!created ? (
          <div className="space-y-4">
            <Field label="Name" required>
              <input
                autoFocus
                className="input-brutal w-full"
                placeholder="e.g. MacBook Agent Host"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </Field>
            <Field label="Specs Directory" optional>
              <input
                className="input-brutal w-full"
                placeholder="Leave blank for a new specs directory"
                value={specsDir}
                onChange={(e) => setSpecsDir(e.target.value)}
              />
            </Field>
            <Field label="Data Root" optional>
              <input
                className="input-brutal w-full"
                placeholder="Leave blank for a new data root"
                value={dataRoot}
                onChange={(e) => setDataRoot(e.target.value)}
              />
            </Field>
            <div className="flex justify-end gap-3 pt-2">
              <button className="btn-brutal bg-white px-4 py-2 text-sm" onClick={onClose}>
                Cancel
              </button>
              <button
                className="btn-brutal bg-brutal-pink px-4 py-2 text-sm disabled:bg-black/10"
                disabled={!name.trim() || busy}
                onClick={() => void create()}
              >
                Generate Script
              </button>
            </div>
          </div>
        ) : (
          <div className="space-y-4">
            <div className="grid gap-2 sm:grid-cols-3">
              <StepPill index={1} label="Config" state="ready" done />
              <StepPill
                index={2}
                label="Serve"
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
            <textarea
              className="h-44 w-full resize-none border-2 border-black bg-brutal-cream p-3 font-mono text-xs outline-none"
              readOnly
              value={created.setupScript}
            />
            <div className="flex flex-wrap justify-end gap-3 pt-2">
              <CopyButton value={created.setupScript} label="setup script" />
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
  onCreate,
}: {
  machine: MachineInfo;
  onClose: () => void;
  onCreate: (input: AgentCreateInput) => void | Promise<void>;
}) {
  const firstProvider = machine.providers[0] ?? null;
  const [providerId, setProviderId] = useState(firstProvider?.id ?? "");
  const [name, setName] = useState("");
  const [actorId, setActorId] = useState("");
  const [description, setDescription] = useState("");
  const [model, setModel] = useState(firstProvider?.defaultModel ?? "");
  const [autostart, setAutostart] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const selectedProvider =
    machine.providers.find((provider) => provider.id === providerId) ??
    firstProvider;
  const modelChoices = selectedProvider?.modelChoices ?? [];

  const create = async () => {
    if (!name.trim() || !providerId || busy) return;
    setBusy(true);
    try {
      await onCreate({
        providerId,
        actorId: actorId.trim(),
        name: name.trim(),
        description,
        model: model.trim(),
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
    setModel(nextProvider?.defaultModel ?? "");
  }, [providerId, machine.providers]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/60 p-4">
      <section className="card-brutal w-full max-w-md p-6">
        <div className="mb-4 flex items-center justify-between">
          <div className="min-w-0">
            <h2 className="text-lg font-black uppercase">Add Agent Actor</h2>
            <div className="truncate font-mono text-xs text-black/45">
              {machine.name}
            </div>
          </div>
          <button className="btn-brutal-sm bg-white p-1" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <div className="space-y-4">
          <Field label="Provider" required>
            <ProviderSelect
              providers={machine.providers}
              value={providerId}
              onPick={setProviderId}
            />
          </Field>
          <Field label="Name" required>
            <input
              autoFocus
              className="input-brutal w-full"
              placeholder="e.g. Builder"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label="Description" optional>
            <textarea
              className="input-brutal w-full resize-none"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
          {modelChoices.length > 0 ? (
            <Field label="Model">
              <SelectLike
                value={
                  modelChoices.find((choice) => choice.id === model)?.label ||
                  model ||
                  "Default"
                }
                options={modelChoices.map((choice) => choice.id)}
                onPick={setModel}
              />
            </Field>
          ) : selectedProvider?.defaultModel ? (
            <Field label="Model">
              <input className="input-brutal w-full" readOnly value={selectedProvider.defaultModel} />
            </Field>
          ) : null}
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
              {modelChoices.length === 0 && (
                <Field label="Model Override" optional>
                  <input
                    className="input-brutal w-full"
                    placeholder="Optional runtime model id"
                    value={model}
                    onChange={(e) => setModel(e.target.value)}
                  />
                </Field>
              )}
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
              Add Actor
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
      Copy Script
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
  options: string[];
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
              key={option}
              className="block w-full px-3 py-2 text-left text-sm font-bold hover:bg-brutal-yellow"
              onClick={() => {
                onPick(option);
                setOpen(false);
              }}
            >
              {option}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function ProviderSelect({
  providers,
  value,
  onPick,
}: {
  providers: AgentProviderSummary[];
  value: string;
  onPick: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const selected =
    providers.find((provider) => provider.id === value) ?? providers[0];
  return (
    <div className="relative">
      <button
        className="input-brutal flex w-full items-center justify-between gap-2 text-sm"
        onClick={() => setOpen((next) => !next)}
      >
        <span className="min-w-0 flex-1 text-left">
          <span className="block truncate font-black">
            {selected?.name ?? "No providers"}
          </span>
          {selected && (
            <span className="block truncate font-mono text-[11px] text-black/45">
              {selected.transportKind} - {selected.command}
            </span>
          )}
        </span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <div className="absolute left-0 right-0 top-full z-10 mt-1 max-h-48 overflow-y-auto border-2 border-black bg-white shadow-brutal">
          {providers.map((provider) => (
            <button
              key={provider.id}
              className="block w-full px-3 py-2 text-left hover:bg-brutal-yellow"
              onClick={() => {
                onPick(provider.id);
                setOpen(false);
              }}
            >
              <span className="block truncate text-sm font-black">
                {provider.name || provider.id}
              </span>
              <span className="block truncate font-mono text-[11px] text-black/45">
                {provider.transportKind} - {provider.command}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function PathRow({
  label,
  value,
  onCopy,
}: {
  label: string;
  value: string;
  onCopy: (value: string, label: string) => Promise<void>;
}) {
  return (
    <div className="flex items-center gap-2 border-2 border-black bg-white px-2 py-1.5">
      <span className="w-12 shrink-0 text-[11px] font-black uppercase tracking-wider text-black/45">
        {label}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-xs">{value}</span>
      <button
        className="btn-brutal-sm bg-white p-1"
        title={`Copy ${label}`}
        onClick={() => void onCopy(value, label)}
      >
        <Copy size={12} />
      </button>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="border-2 border-black bg-white p-2">
      <div className="font-mono text-[11px] uppercase tracking-wider text-black/45">
        {label}
      </div>
      <div className="truncate font-black">{value}</div>
    </div>
  );
}

function agentProviderName(agent: AgentInfo) {
  const meta = agent.spec.actor._meta ?? {};
  return typeof meta.providerName === "string" ? meta.providerName : "provider spec";
}

interface MachineCreateInput {
  name: string;
  specsDir?: string;
  dataRoot?: string;
}

interface AgentCreateInput {
  providerId: string;
  actorId: string;
  name: string;
  description: string;
  model: string;
  autostart: boolean;
}
