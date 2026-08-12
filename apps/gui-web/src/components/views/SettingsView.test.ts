// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Actor, MachineInfo } from "@/ipc/types";
import type { ActorWorkspaceSection, AgentFormState } from "@/lib/types";
import { defaultWakeSpec } from "@/lib/wake-utils";
import { SettingsView } from "@/components/views/SettingsView";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const roots: Root[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) {
    React.act(() => root.unmount());
  }
  document.body.innerHTML = "";
});

const agentForm: AgentFormState = {
  machineId: "",
  providerId: "",
  actorId: "",
  name: "Echo",
  description: "",
  instructions: "",
  model: "",
  autostart: true,
  env: {},
  wake: defaultWakeSpec(),
};

const managedHost: MachineInfo = {
  workspaceId: "space-1",
  ownerActorId: "actor_human_owner",
  id: "host-1",
  name: "Build Host",
  kind: "daemon",
  source: "remote",
  readOnly: false,
  canCommand: true,
  canOpenLocalPath: false,
  capabilities: ["machine.remove"],
  inventoryRevision: 1,
  inventoryObservedAt: null,
  status: "online",
  setupStatus: "ready",
  connectionStatus: "online",
  connectionActorId: "actor_service_host_1",
  dataRoot: "C:\\loom",
  configDir: "C:\\loom\\config",
  agentCount: 0,
  onlineAgentCount: 0,
  serviceCount: 0,
  providers: [],
  agents: [],
  services: [],
  serviceRuntimeStates: [],
  serveCommand: "loom serve",
  setupScript: "",
};

const schedulerHost: MachineInfo = {
  ...managedHost,
  serviceCount: 1,
  services: [
    {
      id: "ops_watchers",
      kind: "scheduler",
      displayName: "Ops Scheduler",
      actor: {
        id: "svc_scheduler",
        kind: "service",
        displayName: "Ops Scheduler",
      },
      autostart: true,
      lifecycle: "channel_singleton",
      channelId: "chan_ops",
      config: {
        jobs: [
          {
            id: "ci_watch",
            schedule: "*/10 * * * *",
            source: { kind: "command", command: "scripts/check-ci" },
            scope: { kind: "channel", id: "chan_ops" },
          },
        ],
      },
    },
  ],
};

function renderSettings(
  activeSection: ActorWorkspaceSection,
  machines: MachineInfo[] = [],
  options: {
    actors?: Record<string, Actor>;
  } = {},
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);

  React.act(() => {
    root.render(
      React.createElement(SettingsView, {
        actors: options.actors ?? {},
        busy: null,
        agentForm,
        setAgentForm: vi.fn(),
        machines,
        runs: {},
        targetAgentId: null,
        onConsumeTargetAgent: vi.fn(),
        activeSection,
        onSectionChange: vi.fn(),
        onCheckMachines: vi.fn(),
        onCreateMachine: vi.fn(() => null),
        onRemoveMachine: vi.fn(),
        onAddAgent: vi.fn(() => false),
        onUpdateAgent: vi.fn(),
        onAddAgentSkill: vi.fn(() => false),
        onRemoveAgent: vi.fn(),
        onOpenLocalPath: vi.fn(),
      }),
    );
  });

  return container;
}

describe("SettingsView navigation structure", () => {
  it("keeps Managed Hosts out of Actor Manage", () => {
    const container = renderSettings("agents");
    const actorManage = container.querySelector('aside[aria-label="Actor manage"]');

    expect(actorManage).not.toBeNull();
    expect(actorManage?.textContent).toContain("Actor Manage");
    expect(actorManage?.textContent).toContain("Humans");
    expect(actorManage?.textContent).toContain("Agents");
    expect(actorManage?.textContent).toContain("Services");
    expect(actorManage?.textContent).not.toContain("Managed Hosts");
    expect(container.textContent).not.toContain("System default");
  });

  it("renders the outer Managed Hosts destination as a list and detail split", () => {
    const container = renderSettings("hosts", [managedHost]);
    const hostList = container.querySelector('aside[aria-label="Managed hosts list"]');
    const hostDetails = container.querySelector(
      '[role="region"][aria-label="Managed host details"]',
    );

    expect(container.textContent).not.toContain("Actor Manage");
    expect(hostList?.textContent).toContain("Managed Hosts");
    expect(hostList?.textContent).toContain("Build Host");
    expect(hostDetails?.textContent).toContain("Build Host");
    expect(hostList?.querySelector('button[title="Refresh hosts"]')).not.toBeNull();
    expect(hostList?.querySelector('button[title="Register host"]')).not.toBeNull();
  });

  it("keeps service creation disabled until ServiceSpec deployment is wired", () => {
    const container = renderSettings("agents");
    const addActor = container.querySelector<HTMLButtonElement>('button[title="Add actor"]');

    React.act(() => addActor?.click());
    const serviceButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Coming Soon"),
    );

    expect(serviceButton).not.toBeUndefined();
    expect(serviceButton?.disabled).toBe(true);
    expect(serviceButton?.textContent).toContain("Coming Soon");
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it("does not mix standalone service actors into the ServiceSpec roster", () => {
    const service: Actor = {
      id: "svc_daily_digest",
      kind: "service",
      displayName: "Daily Digest",
    };
    const container = renderSettings("services", [], {
      actors: { [service.id]: service },
    });

    expect(container.textContent).not.toContain("Daily Digest");
    expect(container.textContent).toContain("No services registered.");
  });

  it("opens a machine-backed ServiceSpec with Overview, Spec, and Config tabs", () => {
    const container = renderSettings("services", [schedulerHost]);
    const serviceButton = Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent?.includes("Ops Scheduler"),
    );

    expect(serviceButton).not.toBeUndefined();
    React.act(() => serviceButton?.click());

    const detailButtons = Array.from(container.querySelectorAll<HTMLButtonElement>("button"));
    expect(detailButtons.some((button) => button.textContent?.trim() === "Overview")).toBe(true);
    expect(detailButtons.some((button) => button.textContent?.trim() === "Spec")).toBe(true);
    expect(detailButtons.some((button) => button.textContent?.trim() === "Config")).toBe(true);
    expect(container.textContent).toContain("Runtime Summary");
    expect(container.textContent).toContain("ci_watch");

    React.act(() =>
      detailButtons.find((button) => button.textContent?.trim() === "Spec")?.click(),
    );
    expect(container.textContent).toContain("Service Spec");
    expect(container.textContent).toContain('"id": "ops_watchers"');

    React.act(() =>
      Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent?.trim() === "Config")
        ?.click(),
    );
    expect(container.textContent).toContain("Config Payload");
    expect(container.textContent).toContain('"jobs"');
  });
});
