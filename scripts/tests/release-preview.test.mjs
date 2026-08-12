import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { planReleasePreview } from "../plan-release-preview.mjs";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

test("creates the next minor 0.x preview when no newer candidate exists", () => {
  assert.deepEqual(
    planReleasePreview({
      currentVersion: "0.1.8",
      branches: ["preview", "preview-0.1.8", "preview-not-semver"],
    }),
    {
      action: "create",
      branch: "preview-0.2.0",
      version: "0.2.0",
      reason: "no newer release preview exists",
    },
  );
});

test("reuses the nearest newer preview across every preview-X.Y.Z candidate", () => {
  const plan = planReleasePreview({
    currentVersion: "0.1.8",
    branches: [
      "preview-1.0.0",
      "preview-0.10.0",
      "refs/heads/preview-0.2.0",
      "preview-0.1.9",
    ],
  });
  assert.equal(plan.action, "reuse");
  assert.equal(plan.branch, "preview-0.1.9");
});

test("uses numeric semver ordering instead of lexical branch ordering", () => {
  const plan = planReleasePreview({
    currentVersion: "0.1.8",
    branches: ["preview-0.10.0", "preview-0.2.0"],
  });
  assert.equal(plan.branch, "preview-0.2.0");
});

test("keeps the rollover inside the current major release line", () => {
  const plan = planReleasePreview({
    currentVersion: "0.1.8",
    branches: ["preview-1.0.0"],
  });
  assert.equal(plan.action, "create");
  assert.equal(plan.branch, "preview-0.2.0");
});

test("skips a minor version that has already been tagged", () => {
  const plan = planReleasePreview({
    currentVersion: "0.1.8",
    tags: ["v0.2.0"],
  });
  assert.equal(plan.action, "create");
  assert.equal(plan.branch, "preview-0.3.0");
});

test("version updater changes every release version in lockstep", () => {
  const testRoot = fs.mkdtempSync(path.join(os.tmpdir(), "loom-preview-version-"));
  try {
    for (const relativePath of [
      "Cargo.toml",
      "Cargo.lock",
      "apps/gui-web/package.json",
      "crates/gui/tauri.conf.json",
      "pages/portal/release-downloads.js",
    ]) {
      const destination = path.join(testRoot, relativePath);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.copyFileSync(path.join(rootDir, relativePath), destination);
    }

    const cargoToml = fs.readFileSync(path.join(rootDir, "Cargo.toml"), "utf8");
    const members = [...cargoToml.match(/^members\s*=\s*\[([\s\S]*?)\]/m)[1].matchAll(/"([^"]+)"/g)];
    for (const [, member] of members) {
      const relativePath = `${member}/Cargo.toml`;
      const destination = path.join(testRoot, relativePath);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.copyFileSync(path.join(rootDir, relativePath), destination);
    }

    execFileSync(
      process.execPath,
      [path.join(rootDir, "scripts/set-release-version.mjs"), "0.2.0", "--root", testRoot],
      { stdio: "pipe" },
    );

    assert.match(fs.readFileSync(path.join(testRoot, "Cargo.toml"), "utf8"), /version = "0\.2\.0"/);
    assert.equal(JSON.parse(fs.readFileSync(path.join(testRoot, "apps/gui-web/package.json"))).version, "0.2.0");
    assert.equal(JSON.parse(fs.readFileSync(path.join(testRoot, "crates/gui/tauri.conf.json"))).version, "0.2.0");
    assert.match(
      fs.readFileSync(path.join(testRoot, "pages/portal/release-downloads.js"), "utf8"),
      /version: "0\.2\.0"/,
    );

    const lock = fs.readFileSync(path.join(testRoot, "Cargo.lock"), "utf8");
    for (const packageName of [
      "agent-runtime",
      "loom-cli",
      "loom-gui",
      "loom-platform",
      "loom-server",
      "loom-shell",
      "proto",
    ]) {
      assert.match(
        lock,
        new RegExp(`name = "${packageName}"\\r?\\nversion = "0\\.2\\.0"`),
      );
    }
  } finally {
    fs.rmSync(testRoot, { recursive: true, force: true });
  }
});
