// @vitest-environment jsdom
import React from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/components/settings/CacheManagementSection", () => ({
  CacheManagementSection: () => null,
}));

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

import { AccountView } from "@/components/views/AccountView";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("AccountView mobile connection QR", () => {
  it("switches between self and identity-free invite payloads", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    React.act(() => {
      root.render(React.createElement(AccountView, {
        account: {
          provider: "local",
          staffId: "account-user",
          nickname: "Account Name",
          realName: "",
          email: "",
          actorId: "actor_human_account",
          avatarUrl: "",
        },
        workspace: {
          id: "space-1",
          name: "Remote Loom",
          serverUrl: "ws://canfuu.com:7878/rpc",
          actorId: "actor_human_workspace",
          displayName: "Workspace Name",
        },
        busy: null,
        onLogout: vi.fn(),
        onAvatarChange: vi.fn(),
      }));
    });

    const qrButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Connect Loom Mobile"]',
    );
    const signOutButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Sign Out"),
    );
    expect(qrButton).not.toBeNull();
    expect(signOutButton).not.toBeUndefined();
    expect(container.textContent).not.toContain("System default");
    expect(container.textContent).not.toContain("Cache Management");
    expect(
      qrButton!.compareDocumentPosition(signOutButton!) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    qrButton!.focus();
    React.act(() => {
      qrButton!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    const encoded = document.querySelector<HTMLElement>('[data-testid="connection-qr"]')
      ?.dataset.value;
    expect(encoded).toBeTruthy();
    const payload = new URL(encoded!);
    expect(payload.searchParams.get("mode")).toBe("self");
    expect(payload.searchParams.get("serverUrl")).toBe("ws://canfuu.com:7878/rpc");
    expect(payload.searchParams.get("actorId")).toBe("actor_human_workspace");
    expect(payload.searchParams.get("displayName")).toBe("Workspace Name");

    const dialog = document.body.querySelector<HTMLElement>(
      '[role="dialog"][aria-labelledby="mobile-connect-title"]',
    );
    const closeButton = dialog!.querySelector<HTMLButtonElement>(
      'button[aria-label="Close mobile connection QR code"]',
    );
    expect(document.activeElement).toBe(closeButton);
    const inviteButton = Array.from(dialog!.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent?.trim() === "Invite someone",
    );
    expect(inviteButton).toBeDefined();
    expect(inviteButton?.getAttribute("aria-pressed")).toBe("false");

    React.act(() => inviteButton!.click());

    const inviteEncoded = dialog!.querySelector<HTMLElement>('[data-testid="connection-qr"]')
      ?.dataset.value;
    const invitePayload = new URL(inviteEncoded!);
    expect(Object.fromEntries(invitePayload.searchParams)).toEqual({
      v: "1",
      mode: "invite",
      serverUrl: "ws://canfuu.com:7878/rpc",
    });
    expect(dialog!.textContent).not.toContain("actor_human_workspace");
    expect(dialog!.textContent).not.toContain("Workspace Name");
    expect(dialog!.textContent).toContain("Your identity is not shared");
    expect(inviteButton?.getAttribute("aria-pressed")).toBe("true");

    React.act(() => closeButton!.click());
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(qrButton);

    React.act(() => root.unmount());
  });
});
