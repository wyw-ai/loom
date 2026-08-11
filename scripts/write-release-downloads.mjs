#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(scriptDir, "..");

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      throw new Error(`unexpected argument: ${arg}`);
    }
    const key = arg.slice(2).replace(/-([a-z])/g, (_, char) => char.toUpperCase());
    const next = argv[i + 1];
    if (!next || next.startsWith("--")) {
      out[key] = "true";
    } else {
      out[key] = next;
      i += 1;
    }
  }
  return out;
}

function readWorkspaceVersion() {
  const cargoToml = fs.readFileSync(path.join(rootDir, "Cargo.toml"), "utf8");
  const section = cargoToml
    .split(/\r?\n(?=\[)/)
    .find((part) => part.trimStart().startsWith("[workspace.package]"));
  const match = section?.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("failed to read workspace version from Cargo.toml");
  }
  return match[1];
}

function sha256(filePath) {
  return crypto.createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function classify(fileName, version) {
  if (fileName === "SHA256SUMS") {
    return { kind: "checksums", label: "SHA256 checksums" };
  }
  if (fileName === "install.sh") {
    return { kind: "installer", label: "Install script" };
  }
  if (fileName === "manifest.txt") {
    return { kind: "manifest", label: "Release manifest" };
  }

  const runtimePrefix = `loom-runtime-${version}-`;
  if (fileName.startsWith(runtimePrefix)) {
    for (const extension of [".tar.gz", ".zip"]) {
      if (fileName.endsWith(extension)) {
        return {
          kind: "runtime",
          label: fileName.slice(runtimePrefix.length, -extension.length),
        };
      }
    }
  }

  const guiPrefix = `loom-gui-${version}-`;
  if (fileName.startsWith(guiPrefix) && fileName.endsWith(".dmg")) {
    return {
      kind: "gui",
      label: fileName.slice(guiPrefix.length, -".dmg".length),
    };
  }

  return { kind: "artifact", label: fileName };
}

const args = parseArgs(process.argv.slice(2));
const version = args.version || process.env.RELEASE_VERSION || readWorkspaceVersion();
const packageDir = path.resolve(rootDir, args.packageDir || "dist/packages");
const outFile = path.resolve(rootDir, args.out || "pages/portal/release-downloads.js");
const repo = args.repository || process.env.GITHUB_REPOSITORY || "";
const tag = args.tag || process.env.GITHUB_REF_NAME || `v${version}`;
const downloadBaseUrl =
  args.downloadBaseUrl ||
  process.env.DOWNLOAD_BASE_URL ||
  (repo ? `https://github.com/${repo}/releases/download/${tag}` : "");

if (!fs.existsSync(packageDir)) {
  throw new Error(`package directory does not exist: ${packageDir}`);
}
if (!downloadBaseUrl) {
  throw new Error("missing --download-base-url or GITHUB_REPOSITORY/GITHUB_REF_NAME");
}

const artifacts = fs
  .readdirSync(packageDir)
  .filter((fileName) => fs.statSync(path.join(packageDir, fileName)).isFile())
  .filter((fileName) => /\.(tar\.gz|zip|dmg)$/.test(fileName) || ["SHA256SUMS", "install.sh", "manifest.txt"].includes(fileName))
  .sort((a, b) => a.localeCompare(b))
  .map((fileName) => {
    const filePath = path.join(packageDir, fileName);
    const info = classify(fileName, version);
    return {
      fileName,
      label: info.label,
      kind: info.kind,
      size: fs.statSync(filePath).size,
      sha256: sha256(filePath),
      downloadUrl: `${downloadBaseUrl.replace(/\/$/, "")}/${encodeURIComponent(fileName)}`,
    };
  });

if (artifacts.length === 0) {
  throw new Error(`no release artifacts found in ${packageDir}`);
}

const release = {
  version,
  gitSha: (args.gitSha || process.env.GITHUB_SHA || "unknown").slice(0, 12),
  generatedAt: args.generatedAt || process.env.GENERATED_AT || new Date().toISOString(),
  group: args.group || tag,
  artifacts,
};

fs.mkdirSync(path.dirname(outFile), { recursive: true });
fs.writeFileSync(outFile, `window.LOOM_RELEASE_DOWNLOADS = ${JSON.stringify(release, null, 2)};\n`);
console.log(`wrote ${path.relative(rootDir, outFile)} with ${artifacts.length} artifacts`);
