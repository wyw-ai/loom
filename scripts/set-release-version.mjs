#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);

function optionValue(name) {
  const index = args.indexOf(name);
  if (index === -1) {
    return "";
  }
  if (!args[index + 1]) {
    throw new Error(`${name} requires a value`);
  }
  return args[index + 1];
}

const rootDir = path.resolve(optionValue("--root") || path.join(scriptDir, ".."));
const positional = args.filter((arg, index) => {
  if (arg === "--root") {
    return false;
  }
  if (index > 0 && args[index - 1] === "--root") {
    return false;
  }
  return !arg.startsWith("--");
});
const nextVersion = positional[0] || "";

if (!STABLE_VERSION.test(nextVersion)) {
  console.error("usage: set-release-version.mjs X.Y.Z [--root path]");
  process.exit(1);
}

function readText(relativePath) {
  return fs.readFileSync(path.join(rootDir, relativePath), "utf8");
}

function replaceSingle(source, pattern, replacement, label) {
  const matches = source.match(new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`));
  if (!matches || matches.length !== 1) {
    throw new Error(`${label}: expected one version field, found ${matches?.length || 0}`);
  }
  return source.replace(pattern, replacement);
}

function workspacePackageSection(cargoToml) {
  const section = cargoToml
    .split(/\r?\n(?=\[)/)
    .find((part) => part.trimStart().startsWith("[workspace.package]"));
  if (!section) {
    throw new Error("Cargo.toml is missing [workspace.package]");
  }
  return section;
}

function workspaceVersion(cargoToml) {
  const match = workspacePackageSection(cargoToml).match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("Cargo.toml [workspace.package] is missing version");
  }
  return match[1];
}

function workspaceMemberNames(cargoToml) {
  const membersMatch = cargoToml.match(/^members\s*=\s*\[([\s\S]*?)\]/m);
  if (!membersMatch) {
    throw new Error("Cargo.toml is missing workspace members");
  }

  return [...membersMatch[1].matchAll(/"([^"]+)"/g)].map((match) => {
    const memberPath = match[1];
    const memberToml = readText(`${memberPath}/Cargo.toml`);
    const packageSection = memberToml
      .split(/\r?\n(?=\[)/)
      .find((part) => part.trimStart().startsWith("[package]"));
    const name = packageSection?.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
    if (!name || !/^version\.workspace\s*=\s*true\s*$/m.test(packageSection)) {
      throw new Error(`${memberPath}/Cargo.toml must inherit its package version from the workspace`);
    }
    return name;
  });
}

function assertCurrentVersion(label, actual, expected) {
  if (actual !== expected) {
    throw new Error(`${label} has version ${actual}, expected ${expected}`);
  }
}

function updateJsonVersion(relativePath, oldVersion) {
  const source = readText(relativePath);
  const parsed = JSON.parse(source);
  assertCurrentVersion(relativePath, parsed.version, oldVersion);
  return replaceSingle(
    source,
    /("version"\s*:\s*")[^"]+(")/,
    `$1${nextVersion}$2`,
    relativePath,
  );
}

function updateCargoLock(source, packageNames, oldVersion) {
  const seen = new Set();
  const updated = source
    .split(/(?=^\[\[package\]\]\r?$)/m)
    .map((block) => {
      if (!/^\[\[package\]\]\r?$/m.test(block)) {
        return block;
      }

      const name = block.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
      if (!name || !packageNames.has(name)) {
        return block;
      }

      const version = block.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
      assertCurrentVersion(`Cargo.lock package ${name}`, version, oldVersion);
      seen.add(name);
      return block.replace(
        /^version\s*=\s*"[^"]+"/m,
        `version = "${nextVersion}"`,
      );
    })
    .join("");

  const missing = [...packageNames].filter((name) => !seen.has(name));
  if (missing.length > 0) {
    throw new Error(`Cargo.lock is missing workspace packages: ${missing.join(", ")}`);
  }
  return updated;
}

try {
  const cargoToml = readText("Cargo.toml");
  const oldVersion = workspaceVersion(cargoToml);
  if (!STABLE_VERSION.test(oldVersion)) {
    throw new Error(`workspace version is not stable semver: ${oldVersion}`);
  }
  if (oldVersion === nextVersion) {
    throw new Error(`release version is already ${nextVersion}`);
  }

  const packageNames = new Set(workspaceMemberNames(cargoToml));
  const portalSource = readText("pages/portal/release-downloads.js");
  const portalVersion = portalSource.match(/\bversion\s*:\s*"([^"]+)"/)?.[1];
  assertCurrentVersion("pages/portal/release-downloads.js", portalVersion, oldVersion);

  const updates = new Map([
    [
      "Cargo.toml",
      replaceSingle(
        cargoToml,
        /(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+(")/m,
        `$1${nextVersion}$2`,
        "Cargo.toml [workspace.package]",
      ),
    ],
    ["Cargo.lock", updateCargoLock(readText("Cargo.lock"), packageNames, oldVersion)],
    ["apps/gui-web/package.json", updateJsonVersion("apps/gui-web/package.json", oldVersion)],
    ["crates/gui/tauri.conf.json", updateJsonVersion("crates/gui/tauri.conf.json", oldVersion)],
    [
      "pages/portal/release-downloads.js",
      replaceSingle(
        portalSource,
        /(\bversion\s*:\s*")[^"]+(")/,
        `$1${nextVersion}$2`,
        "pages/portal/release-downloads.js",
      ),
    ],
  ]);

  for (const [relativePath, source] of updates) {
    fs.writeFileSync(path.join(rootDir, relativePath), source);
  }

  console.log(`release version updated: ${oldVersion} -> ${nextVersion}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
