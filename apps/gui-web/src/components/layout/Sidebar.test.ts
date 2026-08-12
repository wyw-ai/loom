// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Channel, HumanAccount, Workspace } from "@/ipc/types";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("qrcode.react", async () => {
  const ReactModule = await import("react");
  return {
    QRCodeSVG: ({ value, title }: { value: string; title?: string }) =>
      ReactModule.createElement("div", {
        "data-testid": "connection-qr",
        "data-value": value,
        title,
      }),
  };
});

import { Sidebar } from "@/components/layout/Sidebar";

const roots: Root[] = [];

const channels: Channel[] = [
  { id: "channel-alpha", title: "alpha", visibility: "public", members: [] },
  { id: "channel-beta", title: "beta", visibility: "public", members: [] },
];

const account: HumanAccount = {
  provider: "local",
  staffId: "",
  nickname: "Canfeng",
  realName: "",
  email: "",
  actorId: "actor_human_local_canfeng",
  avatarUrl: "",
};

const workspace: Workspace = {
  id: "workspace-one",
  name: "Loom Test Server",
  serverUrl: "ws://loom.test:7878/rpc",
  actorId: "actor_human_local_canfeng",
  displayName: "Canfeng",
};

type SidebarProps = React.ComponentProps<typeof Sidebar>;

function defaultProps(): SidebarProps {
  return {
    view: "chat",
    setView: vi.fn(),
    busy: null,
    channels,
    channelGroups: [
      {
        id: "section-work",
        title: "Work",
        channelIds: ["channel-alpha"],
        collapsed: false,
      },
    ],
    connection: "open",
    account,
    workspace,
    hasWorkspace: true,
    workspaceId: "workspace-one",
    workspaceName: "Loom Test Server",
    workspaceServerUrl: "ws://loom.test:7878/rpc",
    activeChannelId: "channel-alpha",
    activeThreadId: null,
    inboxCount: 0,
    threadsByChannel: {},
    onAddChannel: vi.fn(),
    onAddChannelGroup: vi.fn(),
    onMoveChannelToGroup: vi.fn(),
    onDeleteChannel: vi.fn(),
    onRenameChannel: vi.fn(),
    onRemoveChannelGroup: vi.fn(),
    onRenameChannelGroup: vi.fn(),
    onLeaveServer: vi.fn(),
    onSelectChannel: vi.fn(),
    onSelectThread: vi.fn(),
    onToggleChannelGroup: vi.fn(),
  };
}

function renderSidebar(overrides: Partial<SidebarProps> = {}) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);
  const props = { ...defaultProps(), ...overrides };
  React.act(() => root.render(React.createElement(Sidebar, props)));
  return { container, props };
}

function buttonWithText(scope: ParentNode, label: string) {
  return Array.from(scope.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.trim() === label,
  );
}

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: undefined,
  });
  vi.restoreAllMocks();
});

describe("Sidebar server navigation", () => {
  it("keeps the fixed navigation but does not render an Agents list for Direct Messages", () => {
    const { container } = renderSidebar({ view: "direct" });

    expect(container.textContent).toContain("Direct Messages");
    expect(container.textContent).toContain("Actors");
    expect(container.textContent).toContain("Managed Hosts");
    expect(container.textContent).not.toContain("Agents");
  });

  it("shares the server address and confirms before leaving", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const onLeaveServer = vi.fn();
    const { container } = renderSidebar({ onLeaveServer });
    const serverMenuButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Server menu"]',
    );

    React.act(() => serverMenuButton!.click());
    expect(serverMenuButton?.getAttribute("aria-expanded")).toBe("true");
    expect(buttonWithText(container, "Create new section")).toBeDefined();
    expect(buttonWithText(container, "Create new channel")).toBeDefined();

    await React.act(async () => {
      buttonWithText(container, "Invite other people")!.click();
      await Promise.resolve();
    });
    expect(writeText).toHaveBeenCalledWith("ws://loom.test:7878/rpc");
    expect(container.querySelector('[role="status"]')?.textContent).toContain(
      "Server address copied",
    );

    React.act(() => buttonWithText(container, "Leave server")!.click());
    expect(onLeaveServer).not.toHaveBeenCalled();
    expect(container.textContent).toContain("Leave this server?");

    React.act(() => buttonWithText(container, "Confirm leave server")!.click());
    expect(onLeaveServer).toHaveBeenCalledTimes(1);
  });

  it("opens the mobile QR and switches to an identity-free invite", () => {
    const { container } = renderSidebar();

    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Server menu"]')!.click();
    });
    const mobileQrButton = buttonWithText(container, "Mobile QR Code");
    expect(mobileQrButton).toBeDefined();

    React.act(() => mobileQrButton!.click());

    const dialog = document.body.querySelector<HTMLElement>(
      '[role="dialog"][aria-labelledby="mobile-connect-title"]',
    );
    expect(dialog).not.toBeNull();
    const selfPayload = new URL(
      dialog!.querySelector<HTMLElement>('[data-testid="connection-qr"]')!.dataset.value!,
    );
    expect(selfPayload.searchParams.get("mode")).toBe("self");
    expect(selfPayload.searchParams.get("actorId")).toBe("actor_human_local_canfeng");
    expect(dialog!.textContent).toContain("actor_human_local_canfeng");

    React.act(() => buttonWithText(dialog!, "Invite someone")!.click());

    const invitePayload = new URL(
      dialog!.querySelector<HTMLElement>('[data-testid="connection-qr"]')!.dataset.value!,
    );
    expect(Object.fromEntries(invitePayload.searchParams)).toEqual({
      v: "1",
      mode: "invite",
      serverUrl: "ws://loom.test:7878/rpc",
    });
    expect(dialog!.textContent).not.toContain("actor_human_local_canfeng");
    expect(dialog!.textContent).not.toContain("Canfeng");
    expect(
      document.body.querySelector('button[aria-label="Close mobile connection QR code"]'),
    ).not.toBeNull();
  });

  it("disables leaving while the existing workspace removal is busy", () => {
    const { container } = renderSidebar({
      busy: "workspace:remove:workspace-one",
    });
    React.act(() => {
      container.querySelector<HTMLButtonElement>('button[aria-label="Server menu"]')!.click();
    });

    expect(buttonWithText(container, "Leave server")?.disabled).toBe(true);
  });
});

describe("Sidebar section and context menus", () => {
  it("reveals section controls on hover styling and moves a selected channel from the + menu", () => {
    const onMoveChannelToGroup = vi.fn();
    const { container } = renderSidebar({ onMoveChannelToGroup });
    const addButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Add channel to Work"]',
    );

    expect(addButton).not.toBeNull();
    expect(addButton?.parentElement?.className).toContain("opacity-0");
    expect(addButton?.parentElement?.className).toContain(
      "group-hover/channelgroup:opacity-100",
    );

    React.act(() => addButton!.click());
    const menu = document.body.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Channels available for Work"]',
    );
    const moveButton = buttonWithText(menu!, "Move #beta here");
    expect(menu).not.toBeNull();
    expect(container.contains(menu)).toBe(false);
    expect(moveButton).toBeDefined();

    React.act(() => moveButton!.click());
    expect(onMoveChannelToGroup).toHaveBeenCalledWith(
      "channel-beta",
      "section-work",
    );
  });

  it("provides focused context menus for blank space, channels, and sections", () => {
    const { container } = renderSidebar();
    const channelArea = container.querySelector<HTMLElement>(
      '[aria-label="Channel sections"]',
    );

    React.act(() => {
      channelArea!.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 80, clientY: 80 }),
      );
    });
    let menu = document.body.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Channel list actions"]',
    );
    expect(buttonWithText(menu!, "New channel")).toBeDefined();
    expect(buttonWithText(menu!, "New section")).toBeDefined();

    React.act(() => buttonWithText(menu!, "New section")!.click());
    expect(container.querySelector('input[placeholder="Section name"]')).not.toBeNull();

    React.act(() => container.querySelector<HTMLButtonElement>('button[aria-label="Server menu"]')!.click());
    const channelButton = Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent?.includes("alpha"),
    );
    React.act(() => {
      channelButton!.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 90, clientY: 90 }),
      );
    });
    menu = document.body.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Channel actions for alpha"]',
    );
    expect(buttonWithText(menu!, "Open")).toBeDefined();
    expect(buttonWithText(menu!, "Move to section")).toBeDefined();
    expect(buttonWithText(menu!, "Rename")).toBeDefined();
    expect(buttonWithText(menu!, "Delete")).toBeDefined();

    const sectionHeader = Array.from(
      container.querySelectorAll<HTMLElement>(".channel-group-header"),
    ).find((element) => element.textContent?.includes("Work"));
    React.act(() => {
      sectionHeader!.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 100, clientY: 100 }),
      );
    });
    menu = document.body.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Section actions for Work"]',
    );
    expect(buttonWithText(menu!, "Add channel")).toBeDefined();
    expect(buttonWithText(menu!, "Rename")).toBeDefined();
    expect(buttonWithText(menu!, "Delete")).toBeDefined();
  });
});
