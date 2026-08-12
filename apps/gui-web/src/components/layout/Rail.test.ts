// @vitest-environment jsdom
import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Rail } from "@/components/layout/Rail";
import { I18nProvider, LANGUAGE_PREFERENCE_STORAGE_KEY } from "@/lib/i18n";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const roots: Root[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) {
    React.act(() => root.unmount());
  }
  window.localStorage.removeItem(LANGUAGE_PREFERENCE_STORAGE_KEY);
  document.body.innerHTML = "";
});

function renderRail({
  accountActive = false,
  systemSettingsActive = false,
  chinese = false,
}: {
  accountActive?: boolean;
  systemSettingsActive?: boolean;
  chinese?: boolean;
} = {}) {
  if (chinese) window.localStorage.setItem(LANGUAGE_PREFERENCE_STORAGE_KEY, "zh-CN");
  const onOpenAccount = vi.fn();
  const onOpenSystemSettings = vi.fn();
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  roots.push(root);
  const rail = React.createElement(Rail, {
    account: null,
    busy: null,
    connection: "open",
    workspace: null,
    workspaces: [],
    onSelectWorkspace: vi.fn(),
    onOpenSpaces: vi.fn(),
    onOpenAccount,
    onOpenSystemSettings,
    spacesActive: false,
    accountActive,
    systemSettingsActive,
  });

  React.act(() => {
    root.render(chinese ? React.createElement(I18nProvider, null, rail) : rail);
  });

  return { container, onOpenAccount, onOpenSystemSettings };
}

function openAccountMenu(container: HTMLElement) {
  const trigger = container.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]');
  expect(trigger).not.toBeNull();
  React.act(() => trigger!.click());
  return trigger!;
}

describe("Rail account menu", () => {
  it("opens profile and system settings as separate destinations", () => {
    const { container, onOpenAccount, onOpenSystemSettings } = renderRail({
      systemSettingsActive: true,
    });
    const trigger = openAccountMenu(container);
    const menu = container.querySelector<HTMLElement>('[role="menu"]');
    const items = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    );

    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(menu?.getAttribute("aria-label")).toBe("Account menu");
    expect(items.map((item) => item.textContent?.trim())).toEqual([
      "Personal Profile",
      "System Settings",
    ]);
    expect(document.activeElement).toBe(items[0]);
    expect(items[0]?.hasAttribute("aria-current")).toBe(false);
    expect(items[1]?.getAttribute("aria-current")).toBe("page");

    React.act(() => items[0]!.click());
    expect(onOpenAccount).toHaveBeenCalledOnce();
    expect(onOpenSystemSettings).not.toHaveBeenCalled();
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it("closes from an outside pointer or Escape and exposes Chinese labels", () => {
    const { container } = renderRail({ accountActive: true, chinese: true });
    const trigger = openAccountMenu(container);

    expect(container.textContent).toContain("个人资料");
    expect(container.textContent).toContain("系统设置");
    React.act(() => {
      document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });
    expect(container.querySelector('[role="menu"]')).toBeNull();

    React.act(() => trigger.click());
    React.act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(container.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});
