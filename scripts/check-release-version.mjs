#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(scriptDir, "..");
const args = process.argv.slice(2);
const printVersion = args.includes("--print-version");
const explicitTagArg = args.find((arg) => !arg.startsWith("--")) || "";
const tagArg =
  explicitTagArg || (process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME || "" : "");

function readText(relativePath) {
  return fs.readFileSync(path.join(rootDir, relativePath), "utf8");
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function workspaceVersion() {
  const cargoToml = readText("Cargo.toml");
  const section = cargoToml
    .split(/\r?\n(?=\[)/)
    .find((part) => part.trimStart().startsWith("[workspace.package]"));
  if (!section) {
    throw new Error("Cargo.toml is missing [workspace.package]");
  }
  const match = section.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("Cargo.toml [workspace.package] is missing version");
  }
  return match[1];
}

function cargoPackageVersion(relativePath, rootVersion) {
  const cargoToml = readText(relativePath);
  const section = cargoToml
    .split(/\r?\n(?=\[)/)
    .find((part) => part.trimStart().startsWith("[package]"));
  if (!section) {
    throw new Error(`${relativePath} is missing [package]`);
  }

  const explicitVersion = section.match(/^version\s*=\s*"([^"]+)"/m);
  if (explicitVersion) {
    return explicitVersion[1];
  }
  if (/^version\.workspace\s*=\s*true\s*$/m.test(section)) {
    return rootVersion;
  }
  throw new Error(`${relativePath} [package] is missing a version`);
}

function releaseDownloadsVersion() {
  const source = readText("pages/portal/release-downloads.js");
  const match = source.match(/\bversion\s*:\s*"([^"]+)"/);
  if (!match) {
    throw new Error("pages/portal/release-downloads.js is missing version");
  }
  return match[1];
}

function normalizeTag(tag) {
  return tag.replace(/^refs\/tags\//, "").replace(/^v/, "");
}

const rootVersion = workspaceVersion();
const versions = [
  ["Cargo.toml", rootVersion],
  ["crates/loom-shell/Cargo.toml", cargoPackageVersion("crates/loom-shell/Cargo.toml", rootVersion)],
  ["crates/gui/tauri.conf.json", readJson("crates/gui/tauri.conf.json").version],
  ["apps/gui-web/package.json", readJson("apps/gui-web/package.json").version],
  ["pages/portal/release-downloads.js", releaseDownloadsVersion()],
];

const mismatches = versions.filter(([, version]) => version !== rootVersion);
if (mismatches.length > 0) {
  for (const [file, version] of mismatches) {
    console.error(`${file} has version ${version}, expected ${rootVersion}`);
  }
  process.exit(1);
}

if (tagArg) {
  const tagVersion = normalizeTag(tagArg);
  if (tagVersion !== rootVersion) {
    console.error(`release tag ${tagArg} resolves to ${tagVersion}, expected ${rootVersion}`);
    process.exit(1);
  }
}

if (printVersion) {
  console.log(rootVersion);
} else {
  console.log(`release versions ok: ${rootVersion}`);
}
