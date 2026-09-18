// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Message } from "@/ipc/types";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("re-resizable", async () => {
  const ReactModule = await import("react");
  return {
    Resizable: ({
      children,
      className,
    }: {
      children?: React.ReactNode;
      className?: string;
    }) => ReactModule.createElement("div", { className }, children),
  };
});

import { Composer } from "@/components/chat/Composer";

const roots: Root[] = [];

function renderComposer({
  draft = "First line\nSecond line",
  replyTo = null,
}: {
  draft?: string;
  replyTo?: Message | null;
} = {}) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);

  React.act(() => {
    root.render(
      React.createElement(Composer, {
        draft,
        setDraft: vi.fn(),
        disabled: false,
        replyTo,
        actorName: "Canfeng",
        onClearReply: vi.fn(),
        onSend: vi.fn(),
        mentionAgents: [],
        busy: false,
      }),
    );
  });

  return container;
}

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("Channel Composer action layout", () => {
  it("keeps Attach and Send in one anchored row inside the input", () => {
    const container = renderComposer();
    const box = container.querySelector<HTMLElement>(".composer-box");
    const row = container.querySelector<HTMLElement>(".composer-action-row");
    const attach = container.querySelector<HTMLButtonElement>(".composer-action-attach");
    const send = container.querySelector<HTMLButtonElement>(".composer-action-send");
    const textarea = container.querySelector<HTMLTextAreaElement>("textarea");

    expect(box).not.toBeNull();
    expect(row?.parentElement).toBe(box);
    expect(attach?.parentElement).toBe(row);
    expect(send?.parentElement).toBe(row);
    expect(attach?.nextElementSibling).toBe(send);

    expect(attach?.classList.contains("absolute")).toBe(false);
    expect(send?.classList.contains("absolute")).toBe(false);
    expect(attach?.classList.contains("shrink-0")).toBe(true);
    expect(send?.classList.contains("shrink-0")).toBe(true);
    expect(textarea?.classList.contains("min-w-0")).toBe(true);
    expect(textarea?.classList.contains("composer-textarea-with-actions")).toBe(true);
    expect(textarea?.classList.contains("mr-12")).toBe(false);
  });

  it("keeps reply banners and the mention popup outside the action row", () => {
    const container = renderComposer({
      draft: "@",
      replyTo: {} as Message,
    });
    const box = container.querySelector<HTMLElement>(".composer-box");
    const row = container.querySelector<HTMLElement>(".composer-action-row");
    const mentionMenu = Array.from(
      container.querySelectorAll<HTMLElement>(".composer-box > div"),
    ).find((element) => element.textContent?.includes("Mentions"));
    const replyBanner = Array.from(container.querySelectorAll<HTMLElement>("div"))
      .find((element) => element.textContent?.includes("Replying to Canfeng"));

    expect(mentionMenu?.parentElement).toBe(box);
    expect(row?.contains(mentionMenu ?? null)).toBe(false);
    expect(replyBanner).toBeDefined();
    expect(row?.contains(replyBanner ?? null)).toBe(false);
    expect(row?.children).toHaveLength(2);
  });
});
