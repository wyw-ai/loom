import { useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Github,
  Loader2,
  MonitorCog,
  Power,
  RefreshCw,
  Server,
  Trash2,
  UserRound,
  type LucideIcon,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type {
  HumanAccount,
  MachineAgentProviderInfo,
  MachineInfo,
  Workspace,
} from "@/ipc/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { localServerCommand } from "@/lib/constants";
import { accountName, workspaceInitials } from "@/lib/format-utils";
import {
  normalizeWorkspaceFormServerUrl,
  serverUrlPreviewPlaceholder,
} from "@/lib/server-url";
import { cn } from "@/lib/utils";
import type { ConnectionState, WorkspaceFormState } from "@/lib/types";

type OnboardingStep = "prep" | "identity" | "server" | "host";

const defaultUserId = "canfeng";
const defaultNickname = "Canfeng";
const userIdPattern = /^[A-Za-z0-9_-]{1,48}$/;
const actorIdPattern = /^[A-Za-z0-9_.:-]{1,64}$/;

export function OnboardingView({
  account,
  busy,
  connection,
  error,
  machines,
  workspace,
  workspaceForm,
  setWorkspaceForm,
  workspaces,
  onSaveIdentity,
  onAddWorkspace,
  onSelectWorkspace,
  onRemoveWorkspace,
  onCheckMachines,
  onStartLocalHost,
  onFinish,
}: {
  account: HumanAccount | null;
  busy: string | null;
  connection: ConnectionState;
  error: string | null;
  machines: MachineInfo[];
  workspace: Workspace | null;
  workspaceForm: WorkspaceFormState;
  setWorkspaceForm: (form: WorkspaceFormState) => void;
  workspaces: Workspace[];
  onSaveIdentity: (args: { userId: string; nickname: string; actorId: string }) => Promise<boolean> | boolean;
  onAddWorkspace: () => Promise<Workspace | null> | Workspace | null;
  onSelectWorkspace: (workspaceId: string) => Promise<Workspace | null> | Workspace | null | void;
  onRemoveWorkspace: (workspaceId: string) => Promise<void> | void;
  onCheckMachines: () => void;
  onStartLocalHost: () => Promise<boolean> | boolean;
  onFinish: () => void;
}) {
  const [step, setStep] = useState<OnboardingStep>(
    account ? (connection === "open" ? "host" : "server") : "prep",
  );
  const [prepComplete, setPrepComplete] = useState(Boolean(account));
  const [userId, setUserId] = useState("");
  const [nickname, setNickname] = useState("");
  const [actorId, setActorId] = useState("");
  const [providers, setProviders] = useState<MachineAgentProviderInfo[]>([]);
  const [localProviderStatus, setLocalProviderStatus] =
    useState<"loading" | "ready" | "error">("loading");

  useEffect(() => {
    let alive = true;
    setLocalProviderStatus("loading");
    void ipc
      .localProviderCheck()
      .then((result) => {
        if (!alive) return;
        setProviders(result.providers);
        setLocalProviderStatus("ready");
      })
      .catch(() => {
        if (!alive) return;
        setProviders([]);
        setLocalProviderStatus("error");
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (!account) {
      setUserId("");
      setNickname("");
      setActorId("");
      return;
    }
    setPrepComplete(true);
    setUserId(account.staffId);
    setNickname(account.nickname || account.realName);
    setActorId(account.actorId);
  }, [account?.actorId, account?.nickname, account?.realName, account?.staffId]);

  useEffect(() => {
    if (!account) {
      if (step === "server" || step === "host") {
        setStep("prep");
      }
      return;
    }
    if (step === "host" && connection !== "open") {
      setStep("server");
    }
  }, [account, connection, step]);

  useEffect(() => {
    if (!account || workspaces.length > 0) return;
    if (workspaceForm.advanced || workspaceForm.host.trim() || workspaceForm.serverUrl.trim()) {
      return;
    }
    setWorkspaceForm({ ...workspaceForm, host: "127.0.0.1:7878" });
  }, [account, setWorkspaceForm, workspaceForm, workspaces.length]);

  const suggestedUserId = account?.staffId || defaultUserId;
  const suggestedNickname = account?.nickname || account?.realName || defaultNickname;
  const effectiveUserId = userId.trim() || suggestedUserId;
  const effectiveNickname = nickname.trim() || suggestedNickname;
  const suggestedActorId = account?.actorId || `actor_human_local_${effectiveUserId}`;
  const effectiveActorId = actorId.trim() || suggestedActorId;
  const userIdValid = !userId.trim() || userIdPattern.test(userId.trim());
  const actorIdValid = !actorId.trim() || actorIdPattern.test(actorId.trim());
  const canSaveIdentity = userIdValid && actorIdValid && busy !== "account:set-local";
  const canVisitServer = Boolean(account);
  const canVisitHost = Boolean(account && connection === "open");
  const hasServerTarget = Boolean(
    workspaceForm.advanced
      ? workspaceForm.serverUrl.trim()
      : workspaceForm.host.trim(),
  );
  const serverPreview = useMemo(() => {
    if (!hasServerTarget) {
      return { text: serverUrlPreviewPlaceholder, valid: false };
    }
    try {
      return {
        text: normalizeWorkspaceFormServerUrl(workspaceForm),
        valid: true,
      };
    } catch {
      return { text: "Invalid server target", valid: false };
    }
  }, [hasServerTarget, workspaceForm]);
  const pendingHost = machines.find((machine) => machine.source === "local_registration") ?? null;
  const liveHost =
    machines.find(
      (machine) =>
        machine.source === "server_inventory" && machine.connectionStatus === "online",
    ) ?? null;
  const hasLocalProvider = providers.length > 0;
  const hostBusy = busy === "machine:create" || busy === "machine:start";

  async function submitIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSaveIdentity) return;
    const saved = await onSaveIdentity({
      userId: effectiveUserId,
      nickname: effectiveNickname,
      actorId: effectiveActorId,
    });
    if (saved) setStep("server");
  }

  async function submitServer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!serverPreview.valid || busy === "workspace:add") return;
    const saved = await onAddWorkspace();
    if (saved) setStep("host");
  }

  async function selectSavedServer(workspaceId: string) {
    const selected = workspaces.find((item) => item.id === workspaceId);
    const result = await onSelectWorkspace(workspaceId);
    if (result || (selected?.id === workspace?.id && connection === "open")) {
      setStep("host");
    }
  }

  async function removeSavedServer(item: Workspace) {
    const ok = window.confirm(`Remove ${item.name || "this server"} from Loom?`);
    if (!ok) return;
    await onRemoveWorkspace(item.id);
    if (item.id === workspace?.id) {
      setStep("server");
    }
  }

  function startSetup() {
    setPrepComplete(true);
    setStep("identity");
  }

  return (
    <div className="grid h-screen w-screen grid-cols-1 overflow-hidden bg-[#f5f6fa] text-[#111827] lg:grid-cols-[300px_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r border-[#e2e6ef] bg-[#f8f9fc] px-6 py-6">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#111827] text-base font-bold text-white">
            L
          </div>
          <div className="min-w-0">
            <div className="text-sm font-bold text-[#111827]">Loom</div>
            <div className="text-xs font-semibold text-[#667085]">First run setup</div>
          </div>
        </div>

        <div className="mt-8 space-y-1">
          <StepRow
            active={step === "prep"}
            complete={prepComplete}
            icon={Check}
            title="Before you start"
            detail="How Loom is wired"
            onSelect={() => setStep("prep")}
          />
          <StepRow
            active={step === "identity"}
            complete={Boolean(account)}
            icon={UserRound}
            title="Your identity"
            detail={account ? accountName(account) : "User ID and nickname"}
            onSelect={() => setStep("identity")}
          />
          <StepRow
            active={step === "server"}
            complete={connection === "open"}
            disabled={!canVisitServer}
            icon={Server}
            title="Server"
            detail={workspace ? workspace.name : "Choose a shared space"}
            onSelect={() => {
              if (canVisitServer) setStep("server");
            }}
          />
          <StepRow
            active={step === "host"}
            complete={Boolean(liveHost)}
            disabled={!canVisitHost}
            icon={MonitorCog}
            title="Host"
            detail={
              liveHost
                ? liveHost.name
                : hasLocalProvider
                  ? `${providers.length} runtime${providers.length === 1 ? "" : "s"} detected`
                  : "Optional"
            }
            onSelect={() => {
              if (canVisitHost) setStep("host");
            }}
          />
        </div>

        <div className="mt-auto border-t border-[#e2e6ef] pt-4">
          <div className="text-xs font-semibold text-[#667085]">Current status</div>
          <div className="mt-2 space-y-2 text-sm">
            <StatusLine label="Account" value={account ? accountName(account) : "Not set"} />
            <StatusLine label="Connection" value={connectionLabelEn(connection)} />
          </div>
        </div>
      </aside>

      <main className="min-h-0 overflow-y-auto bg-white">
        <div className="mx-auto flex min-h-full w-full max-w-4xl flex-col px-6 py-8 lg:px-10">
          {error ? (
            <div className="mb-4 rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm font-medium text-red-700">
              {error}
            </div>
          ) : null}

          {step === "prep" ? (
            <PrepStep onContinue={startSetup} />
          ) : step === "identity" ? (
            <section className="flex flex-1 flex-col justify-center">
              <div className="max-w-2xl">
                <StepEyebrow icon={UserRound}>Step 1</StepEyebrow>
                <h1 className="mt-4 text-3xl font-bold tracking-normal text-[#111827]">
                  Set your Loom identity
                </h1>
                <p className="mt-3 max-w-xl text-sm leading-6 text-[#667085]">
                  This identity is reused across every server. User ID, nickname, and Actor ID can be left blank to use the suggested values.
                </p>

                <form className="mt-7 overflow-hidden rounded-lg border border-[#dfe3ec] bg-white" onSubmit={submitIdentity}>
                  <div className="border-b border-[#e6e9f0] bg-[#fbfbfd] px-5 py-4">
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <div className="text-sm font-bold text-[#111827]">Custom Identity</div>
                        <div className="mt-1 text-sm leading-5 text-[#667085]">
                          No account registration required. Loom stores this locally to identify your messages and ownership.
                        </div>
                      </div>
                      <Badge variant="success">Current method</Badge>
                    </div>
                  </div>

                  <div className="p-5">
                    <div className="grid gap-4">
                      <FormField
                        label="User ID"
                        hint="Used to generate the default Actor ID. Letters, numbers, underscores, and hyphens are supported."
                      >
                        <Input
                          value={userId}
                          onChange={(event) => setUserId(event.target.value)}
                          className="mt-2 h-11"
                          placeholder={suggestedUserId}
                          autoFocus
                        />
                      </FormField>
                      <FormField label="Nickname" hint="Shown in chat and lists when available.">
                        <Input
                          value={nickname}
                          onChange={(event) => setNickname(event.target.value)}
                          className="mt-2 h-11"
                          placeholder={suggestedNickname}
                        />
                      </FormField>
                      <FormField
                        label="Actor ID"
                        hint="Change this only when you need a stable custom identity. The default is fine for most users."
                      >
                        <Input
                          value={actorId}
                          onChange={(event) => setActorId(event.target.value)}
                          className="mt-2 h-11 font-mono"
                          placeholder={suggestedActorId}
                        />
                      </FormField>
                    </div>

                    <div
                      className={cn(
                        "mt-4 rounded-lg border px-3 py-2 text-xs",
                        (userId.trim() && !userIdValid) || (actorId.trim() && !actorIdValid)
                          ? "border-red-200 bg-red-50 text-red-700"
                          : "border-[#dfe3ec] bg-[#fbfbfd] text-[#667085]",
                      )}
                    >
                      {userId.trim() && !userIdValid
                        ? "User ID can only use letters, numbers, underscores, and hyphens."
                        : actorId.trim() && !actorIdValid
                          ? "Actor ID can use letters, numbers, underscores, hyphens, dots, and colons, up to 64 characters."
                          : (
                            <>
                              <span className="font-semibold">Will use</span>
                              <code className="ml-2 font-mono">{effectiveActorId}</code>
                            </>
                          )}
                    </div>

                    <div className="mt-5 flex flex-wrap gap-3">
                      <Button type="submit" disabled={!canSaveIdentity} className="h-11 rounded-lg">
                        {busy === "account:set-local" ? (
                          <Loader2 className="animate-spin" size={16} />
                        ) : (
                          <ArrowRight size={16} />
                        )}
                        Continue
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        className="h-11 rounded-lg"
                        onClick={() => setStep("prep")}
                      >
                        <ArrowLeft size={15} />
                        Back to overview
                      </Button>
                    </div>
                  </div>
                </form>

                {!account ? (
                  <div className="mt-5 overflow-hidden rounded-lg border border-[#dfe3ec] bg-[#fbfbfd]">
                    <DisabledProviderRow
                      icon={Github}
                      title="GitHub sign-in"
                      detail="Not supported yet. The entry stays here; use Custom Identity in this build."
                    />
                    <DisabledProviderRow
                      title="Google sign-in"
                      detail="Not supported yet. This row will become available after integration."
                    />
                  </div>
                ) : null}
              </div>
            </section>
          ) : step === "server" ? (
            <section className="flex flex-1 flex-col py-2">
              <Button
                type="button"
                variant="outline"
                className="mb-5 w-fit rounded-lg"
                onClick={() => setStep("identity")}
              >
                <ArrowLeft size={15} />
                Back to identity
              </Button>
              <StepEyebrow icon={Server}>Step 2</StepEyebrow>
              <h1 className="mt-4 text-3xl font-bold tracking-normal text-[#111827]">
                Choose a server
              </h1>
              <p className="mt-3 max-w-xl text-sm leading-6 text-[#667085]">
                A server is Loom's shared space. Connect an existing address or add a reachable server. This will not create a new user identity.
              </p>

              {workspaces.length > 0 ? (
                <div className="mt-7">
                  <div className="mb-2 text-xs font-semibold tracking-wide text-[#596174]">
                    Saved Servers
                  </div>
                  <div className="overflow-hidden rounded-lg border border-[#dfe3ec] bg-white">
                    {workspaces.map((item, index) => {
                      const selected = item.id === workspace?.id;
                      const connecting = busy === `connect:${item.id}`;
                      return (
                        <div
                          key={item.id}
                          className={cn(
                            "flex min-h-20 items-center gap-3 border-[#e6e9f0] px-4 py-3 transition-colors",
                            index > 0 && "border-t",
                            selected ? "bg-[#f6f4ff]" : "hover:bg-[#fbfbfd]",
                          )}
                        >
                          <button
                            type="button"
                            className="flex min-w-0 flex-1 items-center gap-3 text-left"
                            onClick={() => {
                              void selectSavedServer(item.id);
                            }}
                            disabled={connecting}
                          >
                            <span
                              className={cn(
                                "flex h-9 w-9 shrink-0 items-center justify-center rounded-full border text-sm font-bold",
                                selected
                                  ? "border-[#8f82ff] bg-white text-[#5843d7]"
                                  : "border-[#dfe3ec] bg-[#fbfbfd] text-[#303849]",
                              )}
                            >
                              {connecting ? (
                                <Loader2 className="animate-spin" size={16} />
                              ) : (
                                workspaceInitials(item)
                              )}
                            </span>
                            <span className="min-w-0 flex-1">
                              <span className="block truncate text-sm font-bold text-[#111827]">
                                {item.name}
                              </span>
                              <span className="mt-1 block truncate font-mono text-xs text-[#667085]">
                                {item.serverUrl}
                              </span>
                            </span>
                          </button>
                          {selected && connection === "open" ? (
                            <Badge variant="success">Connected</Badge>
                          ) : null}
                          <button
                            type="button"
                            title="Remove server"
                            aria-label="Remove server"
                            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-[#667085] transition-colors hover:bg-white hover:text-[#b42318]"
                            onClick={() => {
                              void removeSavedServer(item);
                            }}
                            disabled={Boolean(busy)}
                          >
                            <Trash2 size={15} />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                </div>
              ) : null}

              <form className="mt-7 rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] p-4" onSubmit={submitServer}>
                <div className="mb-3">
                  <div className="text-sm font-bold text-[#111827]">Add a server</div>
                  <div className="mt-1 text-sm text-[#667085]">
                    Enter a host and port. Loom will turn it into a WebSocket RPC address.
                  </div>
                </div>
                <div className="grid gap-3 lg:grid-cols-[180px_minmax(0,1fr)_auto]">
                  <Input
                    value={workspaceForm.name}
                    aria-label="Server name"
                    onChange={(event) =>
                      setWorkspaceForm({ ...workspaceForm, name: event.target.value })
                    }
                    placeholder="Local"
                  />
                  <Input
                    value={
                      workspaceForm.advanced
                        ? workspaceForm.serverUrl
                        : workspaceForm.host
                    }
                    aria-label={workspaceForm.advanced ? "Server URL" : "Server host"}
                    onChange={(event) =>
                      setWorkspaceForm(
                        workspaceForm.advanced
                          ? { ...workspaceForm, serverUrl: event.target.value }
                          : { ...workspaceForm, host: event.target.value },
                      )
                    }
                    placeholder={
                      workspaceForm.advanced
                        ? serverUrlPreviewPlaceholder
                        : "127.0.0.1:7878"
                    }
                  />
                  <Button
                    type="submit"
                    disabled={
                      busy === "workspace:add" ||
                      !workspaceForm.name.trim() ||
                      !serverPreview.valid
                    }
                  >
                    {busy === "workspace:add" ? (
                      <Loader2 className="animate-spin" size={15} />
                    ) : (
                      <Power size={15} />
                    )}
                    Connect
                  </Button>
                </div>
                <div
                  className={cn(
                    "mt-3 flex min-h-9 items-center gap-2 rounded-md border px-3 py-2 text-xs",
                    !hasServerTarget || serverPreview.valid
                      ? "border-[#dfe3ec] bg-white text-[#667085]"
                      : "border-red-200 bg-red-50 text-red-700",
                  )}
                >
                  <span className="shrink-0 font-semibold tracking-wide">RPC</span>
                  <code className="min-w-0 truncate font-mono">{serverPreview.text}</code>
                  <button
                    type="button"
                    className="ml-auto text-xs font-bold text-[#3155a6]"
                    onClick={() =>
                      setWorkspaceForm({
                        ...workspaceForm,
                        advanced: !workspaceForm.advanced,
                        serverUrl: serverPreview.valid ? serverPreview.text : workspaceForm.serverUrl,
                      })
                    }
                  >
                    {workspaceForm.advanced ? "Host mode" : "Full URL"}
                  </button>
                </div>
              </form>
            </section>
          ) : (
            <section className="flex flex-1 flex-col py-2">
              <Button
                type="button"
                variant="outline"
                className="mb-5 w-fit rounded-lg"
                onClick={() => setStep("server")}
              >
                <ArrowLeft size={15} />
                Back to Server
              </Button>
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <StepEyebrow icon={MonitorCog}>Step 3</StepEyebrow>
                  <h1 className="mt-4 text-3xl font-bold tracking-normal text-[#111827]">
                    Start a Host (optional)
                  </h1>
                  <p className="mt-3 max-w-xl text-sm leading-6 text-[#667085]">
                    A Host is the machine that runs agent CLIs. Start it here if the agents run on this computer. If they run elsewhere, open Loom on that machine, connect to the same server, then run the generated Host command there.
                  </p>
                </div>
                <Button variant="outline" onClick={onCheckMachines} disabled={busy === "machine:check"}>
                  {busy === "machine:check" ? (
                    <Loader2 className="animate-spin" size={15} />
                  ) : (
                    <RefreshCw size={15} />
                  )}
                  Refresh
                </Button>
              </div>

              <div className="mt-7 grid gap-4 lg:grid-cols-[minmax(0,1fr)_280px]">
                <div className="rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-sm font-bold text-[#111827]">
                        {localProviderStatus === "loading"
                          ? "Checking local runtimes"
                          : hasLocalProvider
                            ? "Runtimes detected"
                            : "No local runtimes detected"}
                      </div>
                      <div className="mt-1 text-sm text-[#667085]">
                        {liveHost
                          ? `${liveHost.name} is online.`
                          : pendingHost
                            ? `${pendingHost.name} is prepared.`
                            : hasLocalProvider
                              ? "Loom can start a local Host for this server."
                              : "You can finish now and connect agents later."}
                      </div>
                    </div>
                    {liveHost ? (
                      <Badge variant="success">Online</Badge>
                    ) : pendingHost ? (
                      <Badge variant="warning">Prepared</Badge>
                    ) : null}
                  </div>

                  <div className="mt-4 flex flex-wrap gap-2">
                    {localProviderStatus === "loading" ? (
                      <Badge variant="secondary">Scanning PATH</Badge>
                    ) : providers.length > 0 ? (
                      providers.map((provider) => (
                        <Badge key={provider.id} variant="outline">
                          {provider.name}
                        </Badge>
                      ))
                    ) : (
                      <Badge variant="secondary">No CLI found</Badge>
                    )}
                  </div>

                  <div className="mt-5 flex flex-wrap gap-3">
                    <Button
                      onClick={() => {
                        void onStartLocalHost();
                      }}
                      disabled={!hasLocalProvider || Boolean(liveHost) || hostBusy}
                      className="rounded-lg"
                    >
                      {hostBusy ? (
                        <Loader2 className="animate-spin" size={15} />
                      ) : liveHost ? (
                        <Check size={15} />
                      ) : (
                        <Power size={15} />
                      )}
                      {liveHost ? "Host Online" : "Start Local Host"}
                    </Button>
                    <Button variant="outline" onClick={onFinish} className="rounded-lg">
                      Finish
                    </Button>
                  </div>
                </div>

                <div className="rounded-lg border border-[#dfe3ec] bg-white p-4">
                  <div className="text-xs font-semibold text-[#667085]">
                    Connected Server
                  </div>
                  <div className="mt-3 flex items-center gap-3">
                    <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-[#dfe3ec] bg-[#fbfbfd] text-sm font-bold text-[#303849]">
                      {workspace ? workspaceInitials(workspace) : "SV"}
                    </span>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-bold text-[#111827]">
                        {workspace?.name ?? "Server"}
                      </div>
                      <div className="truncate font-mono text-xs text-[#667085]">
                        {workspace?.serverUrl ?? ""}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </section>
          )}
        </div>
      </main>
    </div>
  );
}

function PrepStep({ onContinue }: { onContinue: () => void }) {
  return (
    <section className="flex flex-1 flex-col justify-center">
      <div className="max-w-3xl">
        <StepEyebrow icon={Check}>Before you start</StepEyebrow>
        <h1 className="mt-4 text-3xl font-bold tracking-normal text-[#111827]">
          A quick map before setup
        </h1>
        <p className="mt-3 max-w-2xl text-sm leading-6 text-[#667085]">
          First run has three jobs: identify yourself, connect to a server, and decide whether this computer should run agents as a Host.
        </p>

        <div className="mt-8 overflow-hidden rounded-lg border border-[#dfe3ec] bg-white">
          <ConceptRow
            icon={UserRound}
            title="Identity"
            body="Your User ID and nickname identify you. Connecting a new server does not create another user."
            detail="Leave fields blank to use the defaults"
          />
          <ConceptRow
            icon={Server}
            title="Server"
            body="The server is the shared space for channels, messages, tasks, and registered Hosts. Connect an existing address, or start a server on one machine first."
            detail={localServerCommand}
          />
          <ConceptRow
            icon={MonitorCog}
            title="Host"
            body="A Host is a computer that runs agents. If this machine runs agents, start the local Host after connecting. If agents live on another machine, connect that machine to the same server and run its generated Host command there."
            detail="Run locally, or run the Host command on the agent machine"
          />
        </div>

        <div className="mt-6 flex flex-wrap gap-3">
          <Button onClick={onContinue} className="h-11 rounded-lg">
            Start setup
            <ArrowRight size={16} />
          </Button>
        </div>
      </div>
    </section>
  );
}

function StepRow({
  active,
  complete,
  detail,
  disabled = false,
  icon: Icon,
  onSelect,
  title,
}: {
  active: boolean;
  complete: boolean;
  detail: string;
  disabled?: boolean;
  icon: LucideIcon;
  onSelect: () => void;
  title: string;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "group flex w-full items-center gap-3 rounded-lg px-2.5 py-2.5 text-left transition-colors",
        active ? "bg-white shadow-sm ring-1 ring-[#dfe3ec]" : "hover:bg-white/70",
        disabled && "cursor-not-allowed opacity-50 hover:bg-transparent",
      )}
    >
      <span
        className={cn(
          "flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-xs",
          active
            ? "border-[#8f82ff] bg-[#f6f4ff] text-[#5843d7]"
            : complete
              ? "border-[#98d6aa] bg-[#f0fdf4] text-[#027a48]"
              : "border-[#d8deea] bg-white text-[#667085]",
        )}
      >
        {complete ? <Check size={14} /> : <Icon size={14} />}
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-bold text-[#111827]">{title}</span>
        <span className="mt-0.5 block truncate text-xs text-[#667085]">{detail}</span>
      </span>
    </button>
  );
}

function StatusLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-[#667085]">{label}</span>
      <span className="truncate font-semibold text-[#303849]">{value}</span>
    </div>
  );
}

function StepEyebrow({
  children,
  icon: Icon,
}: {
  children: string;
  icon: LucideIcon;
}) {
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-[#dfe3ec] bg-[#fbfbfd] px-3 py-1.5 text-xs font-bold text-[#485063]">
      <Icon size={14} />
      {children}
    </div>
  );
}

function FormField({
  children,
  hint,
  label,
}: {
  children: ReactNode;
  hint: string;
  label: string;
}) {
  return (
    <label className="block">
      <span className="text-xs font-semibold tracking-wide text-[#596174]">
        {label}
      </span>
      {children}
      <span className="mt-1.5 block text-xs leading-5 text-[#667085]">{hint}</span>
    </label>
  );
}

function DisabledProviderRow({
  detail,
  icon: Icon,
  title,
}: {
  detail: string;
  icon?: LucideIcon;
  title: string;
}) {
  return (
    <div className="flex items-center gap-3 border-t border-[#e6e9f0] px-4 py-3 first:border-t-0">
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-[#dfe3ec] bg-white text-[#667085]">
        {Icon ? <Icon size={15} /> : <span className="text-xs font-bold">G</span>}
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-bold text-[#111827]">{title}</div>
        <div className="mt-0.5 truncate text-xs text-[#667085]">{detail}</div>
      </div>
      <Badge variant="secondary">Not supported</Badge>
    </div>
  );
}

function ConceptRow({
  body,
  detail,
  icon: Icon,
  title,
}: {
  body: string;
  detail: string;
  icon: LucideIcon;
  title: string;
}) {
  return (
    <div className="grid gap-3 border-t border-[#e6e9f0] px-5 py-4 first:border-t-0 sm:grid-cols-[112px_minmax(0,1fr)]">
      <div className="flex items-center gap-2 text-sm font-bold text-[#111827]">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-[#dfe3ec] bg-[#fbfbfd] text-[#485063]">
          <Icon size={15} />
        </span>
        {title}
      </div>
      <div className="min-w-0">
        <p className="text-sm leading-6 text-[#667085]">{body}</p>
        <code className="mt-2 block truncate rounded-md border border-[#dfe3ec] bg-[#fbfbfd] px-2.5 py-1.5 font-mono text-xs font-semibold text-[#485063]">
          {detail}
        </code>
      </div>
    </div>
  );
}

function connectionLabelEn(connection: ConnectionState) {
  if (connection === "open") return "Connected";
  if (connection === "connecting") return "Connecting";
  if (connection === "error") return "Connection error";
  if (connection === "closed") return "Disconnected";
  return "Idle";
}
