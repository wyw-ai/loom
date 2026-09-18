// @vitest-environment jsdom
import React from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChatHeader } from "@/components/layout/ChatHeader";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ChatHeader status responsibilities", () => {
  it("keeps the channel topic in the subtitle and has no run status or Stop control", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    React.act(() => {
      root.render(React.createElement(ChatHeader, {
        channel: {
          id: "channel-status",
          title: "product",
          topic: "Product planning",
          visibility: "public",
          members: [],
        },
        target: "#product",
        connection: "open",
        activePanel: null,
        onOpenPanel: vi.fn(),
      }));
    });

    expect(container.textContent).toContain("Product planning");
    expect(container.textContent).not.toContain("is thinking");
    expect(
      Array.from(container.querySelectorAll("button")).some((button) =>
        button.textContent?.includes("Stop"),
      ),
    ).toBe(false);

    React.act(() => root.unmount());
  });

  it("changes a regular channel from private to public through channel settings", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const channel = {
      id: "channel-private",
      title: "roadmap",
      topic: "",
      visibility: "private" as const,
      members: ["actor-owner"],
    };
    const onUpdateVisibility = vi.fn(async () => true);

    await React.act(async () => {
      root.render(React.createElement(ChatHeader, {
        channel,
        target: "#roadmap",
        connection: "open",
        activePanel: null,
        onOpenPanel: vi.fn(),
        onUpdateVisibility,
        currentActorId: "actor-owner",
      }));
    });

    const settingsButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Channel settings"]',
    );
    expect(settingsButton).not.toBeNull();
    await React.act(async () => {
      settingsButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain(
      "Everyone on this server can find and open this channel.",
    );
    const publicOption = Array.from(
      document.body.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    ).find((button) => button.textContent?.includes("Public"));
    await React.act(async () => {
      publicOption?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    const saveButton = Array.from(document.body.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.trim() === "Save visibility");
    await React.act(async () => {
      saveButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(onUpdateVisibility).toHaveBeenCalledWith(channel, "public");
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain("Instructions");
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain("Skills");
    await React.act(async () => root.unmount());
  });

  it("does not show channel settings for a direct-message channel", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await React.act(async () => {
      root.render(React.createElement(ChatHeader, {
        channel: {
          id: "dm-channel",
          title: "dm:actor-owner:actor-peer",
          visibility: "private",
          members: ["actor-owner", "actor-peer"],
        },
        target: "dm:@actor-peer",
        connection: "open",
        activePanel: null,
        onOpenPanel: vi.fn(),
        onUpdateVisibility: vi.fn(async () => true),
        currentActorId: "actor-owner",
      }));
    });

    expect(
      container.querySelector('button[aria-label="Channel settings"]'),
    ).toBeNull();
    await React.act(async () => root.unmount());
  });

  it("keeps the original settings available while making visibility read-only for other readers", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await React.act(async () => {
      root.render(React.createElement(ChatHeader, {
        channel: {
          id: "public-channel",
          title: "announcements",
          visibility: "public",
          members: ["actor-owner"],
        },
        target: "#announcements",
        connection: "open",
        activePanel: null,
        onOpenPanel: vi.fn(),
        onUpdateVisibility: vi.fn(async () => true),
        currentActorId: "actor-reader",
      }));
    });

    const settingsButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Channel settings"]',
    );
    expect(settingsButton).not.toBeNull();
    await React.act(async () => {
      settingsButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const dialog = document.body.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain("Instructions");
    expect(dialog?.textContent).toContain("Skills");
    expect(dialog?.textContent).toContain("Only the channel creator can change visibility.");
    expect(
      Array.from(dialog?.querySelectorAll<HTMLButtonElement>('[role="radio"]') ?? [])
        .every((button) => button.disabled),
    ).toBe(true);
    await React.act(async () => root.unmount());
  });
});
