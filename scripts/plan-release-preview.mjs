#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const PREVIEW_BRANCH = /^preview-((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))$/;

export function parseStableVersion(value) {
  const match = STABLE_VERSION.exec(value);
  if (!match) {
    throw new Error(`invalid stable version: ${value}`);
  }

  const parts = match.slice(1).map((part) => Number(part));
  if (parts.some((part) => !Number.isSafeInteger(part))) {
    throw new Error(`version component is too large: ${value}`);
  }

  return {
    major: parts[0],
    minor: parts[1],
    patch: parts[2],
    version: value,
  };
}

export function compareVersions(left, right) {
  return (
    left.major - right.major ||
    left.minor - right.minor ||
    left.patch - right.patch
  );
}

function normalizeRef(value) {
  return value.trim().replace(/^refs\/heads\//, "");
}

function previewFromBranch(value) {
  const branch = normalizeRef(value);
  const match = PREVIEW_BRANCH.exec(branch);
  if (!match) {
    return null;
  }
  return { branch, ...parseStableVersion(match[1]) };
}

function versionFromTag(value) {
  const tag = value.trim().replace(/^refs\/tags\//, "").replace(/^v/, "");
  try {
    return parseStableVersion(tag);
  } catch {
    return null;
  }
}

export function planReleasePreview({ currentVersion, branches = [], tags = [] }) {
  const current = parseStableVersion(currentVersion);
  const previews = branches.map(previewFromBranch).filter(Boolean);
  const nextExisting = previews
    .filter(
      (candidate) =>
        candidate.major === current.major && compareVersions(candidate, current) > 0,
    )
    .sort(compareVersions)[0];

  if (nextExisting) {
    return {
      action: "reuse",
      branch: nextExisting.branch,
      version: nextExisting.version,
      reason: "a newer release preview already exists",
    };
  }

  const usedVersions = new Set([
    ...previews.map((candidate) => candidate.version),
    ...tags.map(versionFromTag).filter(Boolean).map((tag) => tag.version),
  ]);

  let minor = current.minor + 1;
  let version = `${current.major}.${minor}.0`;
  while (usedVersions.has(version)) {
    minor += 1;
    version = `${current.major}.${minor}.0`;
  }

  return {
    action: "create",
    branch: `preview-${version}`,
    version,
    reason: "no newer release preview exists",
  };
}

function readLines(filePath) {
  if (!filePath) {
    return [];
  }
  return fs
    .readFileSync(filePath, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function optionValue(args, name) {
  const index = args.indexOf(name);
  if (index === -1) {
    return "";
  }
  if (!args[index + 1]) {
    throw new Error(`${name} requires a value`);
  }
  return args[index + 1];
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    const currentVersion = optionValue(args, "--current");
    if (!currentVersion) {
      throw new Error(
        "usage: plan-release-preview.mjs --current X.Y.Z [--branches-file path] [--tags-file path]",
      );
    }

    const plan = planReleasePreview({
      currentVersion,
      branches: readLines(optionValue(args, "--branches-file")),
      tags: readLines(optionValue(args, "--tags-file")),
    });
    process.stdout.write(`${JSON.stringify(plan)}\n`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
