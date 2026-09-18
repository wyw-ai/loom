// @vitest-environment jsdom
import React from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@/components/settings/CacheManagementSection", () => ({
  CacheManagementSection: () => React.createElement("div", {
    "data-testid": "cache-management",
  }, "Cache Management"),
}));

import { SystemSettingsView } from "@/components/views/SystemSettingsView";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("SystemSettingsView", () => {
  it("owns language and cache settings", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    React.act(() => root.render(React.createElement(SystemSettingsView)));

    expect(container.textContent).toContain("System Settings");
    expect(container.textContent).toContain("Language");
    expect(container.textContent).toContain("System default");
    expect(container.querySelector('[data-testid="cache-management"]')).not.toBeNull();

    React.act(() => root.unmount());
  });
});
