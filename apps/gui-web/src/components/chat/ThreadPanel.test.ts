// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/components/chat/ActorActivityBanner", async () => {
  const ReactModule = await import("react");
  return {
    ActorActivityBanner: () =>
      ReactModule.createElement("div", { "data-layout": "thread-activity" }),
  };
});

vi.mock("@/components/chat/ThreadComposer", async () => {
  const ReactModule = await import("react");
  return {
    ThreadComposer: () =>
      ReactModule.createElement("footer", { "data-layout": "thread-composer" }),
  };
});

import { ThreadPanel } from "@/components/chat/ThreadPanel";

const roots: Root[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("ThreadPanel vertical layout", () => {
  it("owns a full-height flex column and keeps the composer as its final child", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    roots.push(root);
    const props: React.ComponentProps<typeof ThreadPanel> = {
      actors: {},
      channel: {
        id: "channel-one",
        title: "General",
        visibility: "public",
        members: [],
      },
      channelMessages: [],
      currentActorId: "actor-human",
      disabled: false,
      draft: "",
      mentionAgents: [],
      machines: [],
      runs: {},
      messages: [],
      setDraft: vi.fn(),
      task: null,
      thread: {
        id: "thread-one",
        channelId: "channel-one",
        title: "Layout",
        rootMessageId: "message-missing",
      },
      busy: null,
      className: "border-l",
      onClose: vi.fn(),
      onSend: vi.fn(),
      onToggleReaction: vi.fn(),
      onOpenAgentSettings: vi.fn(),
    };

    React.act(() => root.render(React.createElement(ThreadPanel, props)));

    const panel = container.querySelector<HTMLElement>("aside.thread-panel");
    const activity = panel?.querySelector<HTMLElement>(
      ':scope > [data-layout="thread-activity"]',
    );
    const composer = panel?.querySelector<HTMLElement>(
      ':scope > [data-layout="thread-composer"]',
    );

    expect(panel).not.toBeNull();
    expect(panel?.classList.contains("flex")).toBe(true);
    expect(panel?.classList.contains("h-full")).toBe(true);
    expect(panel?.classList.contains("min-w-0")).toBe(true);
    expect(panel?.classList.contains("max-w-full")).toBe(true);
    expect(panel?.classList.contains("flex-1")).toBe(true);
    expect(panel?.classList.contains("flex-col")).toBe(true);
    expect(panel?.classList.contains("overflow-hidden")).toBe(true);
    expect(activity).not.toBeNull();
    expect(composer).not.toBeNull();
    expect(activity?.previousElementSibling?.classList.contains("flex-1")).toBe(true);
    expect(activity?.nextElementSibling).toBe(composer);
    expect(panel?.lastElementChild).toBe(composer);
  });

  it("shows separate back and default-size controls when maximized", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    roots.push(root);
    const onClose = vi.fn();
    const onRestore = vi.fn();

    React.act(() => root.render(React.createElement(ThreadPanel, {
      actors: {},
      channel: {
        id: "channel-one",
        title: "General",
        visibility: "public",
        members: [],
      },
      channelMessages: [],
      currentActorId: "actor-human",
      disabled: false,
      draft: "",
      mentionAgents: [],
      machines: [],
      runs: {},
      messages: [],
      setDraft: vi.fn(),
      task: null,
      thread: {
        id: "thread-one",
        channelId: "channel-one",
        title: "Maximized controls",
        rootMessageId: "message-missing",
      },
      busy: null,
      maximized: true,
      onClose,
      onRestore,
      onSend: vi.fn(),
      onToggleReaction: vi.fn(),
      onOpenAgentSettings: vi.fn(),
    })));

    const back = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Back to channel"]',
    );
    const restore = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Restore default size"]',
    );

    expect(back).not.toBeNull();
    expect(restore).not.toBeNull();
    expect(container.querySelector('button[aria-label="Close thread"]')).toBeNull();

    React.act(() => back?.click());
    React.act(() => restore?.click());
    expect(onClose).toHaveBeenCalledOnce();
    expect(onRestore).toHaveBeenCalledOnce();
  });
});
