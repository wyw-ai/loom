// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("WorkspaceShell maximized thread layout", () => {
  it("keeps the thread in an independent layer starting at the channel-list edge", () => {
    const css = readFileSync(resolve("src/design/globals.css"), "utf8");

    expect(css).toMatch(
      /\.detail-panel-shell\.detail-panel-shell-thread-maximized\s*\{[^}]*position:\s*fixed[^}]*inset:\s*0 0 0 calc\(72px \+ var\(--sidebar-width\) \+ 8px\)[^}]*width:\s*auto/s,
    );
    expect(css).not.toContain("app-shell-thread-underlay");
  });
});
