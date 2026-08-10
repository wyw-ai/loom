// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

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

import { ThreadComposer } from "@/components/chat/ThreadComposer";
import { LONG_TEXT_THRESHOLD } from "@/lib/attachment-utils";

const roots: Root[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) React.act(() => root.unmount());
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("ThreadComposer layout", () => {
  it("keeps Attach and Send together inside the textarea shell", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    roots.push(root);

    React.act(() => {
      root.render(
        React.createElement(ThreadComposer, {
          draft: "Ready to send",
          setDraft: vi.fn(),
          disabled: false,
          busy: false,
          mentionAgents: [],
          onSend: vi.fn(),
        }),
      );
    });

    const row = container.querySelector<HTMLElement>(".thread-composer-input-row");
    const input = container.querySelector<HTMLElement>(".thread-composer-input");
    const textarea = container.querySelector<HTMLTextAreaElement>(
      ".thread-composer-textarea",
    );
    const actions = container.querySelector<HTMLElement>(".composer-action-row");
    const attach = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Attach files"]',
    );
    const send = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Send message"]',
    );

    expect(row).not.toBeNull();
    expect(input).not.toBeNull();
    expect(textarea).not.toBeNull();
    expect(actions).not.toBeNull();
    expect(attach).not.toBeNull();
    expect(send).not.toBeNull();
    expect(input?.parentElement).toBe(row);
    expect(textarea?.parentElement).toBe(input);
    expect(actions?.parentElement).toBe(input);
    expect(attach?.parentElement).toBe(actions);
    expect(send?.parentElement).toBe(actions);
    expect(attach?.nextElementSibling).toBe(send);
    expect(actions?.classList.contains("absolute")).toBe(false);
    expect(send?.classList.contains("absolute")).toBe(false);
  });

  it("pins the complete narrow-width containment chain in the stylesheet", () => {
    const css = readFileSync(
      path.join(process.cwd(), "src", "design", "globals.css"),
      "utf8",
    );
    const footerRule = css.match(/\.thread-composer\s*\{([^}]*)\}/)?.[1];
    const resizableRule = css.match(
      /\.thread-composer-resizable,\s*\.thread-composer-frame\s*\{([^}]*)\}/,
    )?.[1];
    const attachmentRule = css.match(
      /\.thread-composer \.composer-attachment-bar\s*\{([^}]*)\}/,
    )?.[1];
    const rowRule = css.match(/\.thread-composer-input-row\s*\{([^}]*)\}/)?.[1];
    const inputRule = css.match(/\.thread-composer-input\s*\{([^}]*)\}/)?.[1];
    const textareaRule = css.match(/\.composer-textarea-with-actions\s*\{([^}]*)\}/)?.[1];
    const actionsRule = css.match(/\.composer-action-row\s*\{([^}]*)\}/)?.[1];
    const controlRule = css.match(
      /\.composer-action-attach,\s*\.composer-action-send\s*\{([^}]*)\}/,
    )?.[1];
    const sendRule = [
      ...css.matchAll(/\.composer-action-send\s*\{([^}]*)\}/g),
    ].at(-1)?.[1];

    expect(footerRule).toMatch(/min-width:\s*0/);
    expect(footerRule).toMatch(/max-width:\s*100%/);
    expect(footerRule).toMatch(/overflow-x:\s*clip/);
    expect(resizableRule).toMatch(/width:\s*100%\s*!important/);
    expect(resizableRule).toMatch(/min-width:\s*0/);
    expect(resizableRule).toMatch(/max-width:\s*100%/);
    expect(attachmentRule).toMatch(/max-width:\s*100%/);
    expect(attachmentRule).toMatch(/overflow-x:\s*auto/);
    expect(rowRule).toMatch(/display:\s*block/);
    expect(rowRule).toMatch(/height:\s*100%/);
    expect(rowRule).toMatch(/min-width:\s*0/);
    expect(rowRule).toMatch(/max-width:\s*100%/);
    expect(inputRule).toMatch(/position:\s*relative/);
    expect(inputRule).toMatch(/width:\s*100%/);
    expect(inputRule).toMatch(/min-width:\s*0/);
    expect(inputRule).toMatch(/max-width:\s*100%/);
    expect(textareaRule).toMatch(/padding-right:\s*96px\s*!important/);
    expect(actionsRule).toMatch(/position:\s*absolute/);
    expect(actionsRule).toMatch(/right:\s*8px/);
    expect(actionsRule).toMatch(/bottom:\s*8px/);
    expect(actionsRule).toMatch(/display:\s*flex/);
    expect(actionsRule).toMatch(/max-width:\s*calc\(100% - 16px\)/);
    expect(actionsRule).toMatch(/flex-shrink:\s*0/);
    expect(controlRule).toMatch(/position:\s*static/);
    expect(controlRule).toMatch(/flex-shrink:\s*0/);
    expect(sendRule).toMatch(/flex:\s*0 0 36px/);
  });

  it("keeps status banners outside the fixed-height input surface", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    roots.push(root);

    React.act(() => {
      root.render(
        React.createElement(ThreadComposer, {
          draft: "x".repeat(LONG_TEXT_THRESHOLD),
          setDraft: vi.fn(),
          disabled: false,
          busy: false,
          mentionAgents: [],
          onSend: vi.fn(),
        }),
      );
    });

    const footer = container.querySelector<HTMLElement>(".thread-composer");
    const banner = container.querySelector<HTMLElement>(".thread-composer-banner");
    const resizable = container.querySelector<HTMLElement>(
      ".thread-composer-resizable",
    );
    const actions = container.querySelector<HTMLElement>(".composer-action-row");

    expect(banner).not.toBeNull();
    expect(banner?.parentElement).toBe(footer);
    expect(resizable?.contains(banner)).toBe(false);
    expect(resizable?.contains(actions)).toBe(true);
  });
});
