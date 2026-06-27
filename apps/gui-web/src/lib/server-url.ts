import { localServerUrl } from "@/lib/constants";
import type { WorkspaceFormState } from "@/lib/types";

export const defaultServerHost = "127.0.0.1";
export const defaultServerPort = "7878";

function hasScheme(value: string) {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(value);
}

function coerceProtocol(protocol: string) {
  switch (protocol.toLowerCase()) {
    case "wss:":
    case "https:":
    case "rpcs:":
      return "wss:";
    case "ws:":
    case "http:":
    case "rpc:":
      return "ws:";
    default:
      throw new Error(`Unsupported server URL scheme ${protocol.replace(":", "")}`);
  }
}

function websocketCandidate(value: string) {
  const schemeMatch = value.match(/^([a-z][a-z0-9+.-]*):\/\//i);
  if (!schemeMatch) {
    return `ws://${value}`;
  }
  const protocol = coerceProtocol(`${schemeMatch[1]}:`);
  return `${protocol}//${value.slice(schemeMatch[0].length)}`;
}

function normalizePort(port: string) {
  const value = port.trim() || defaultServerPort;
  if (!/^\d+$/.test(value)) {
    throw new Error("Server port must be a number");
  }
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) {
    throw new Error("Server port must be between 1 and 65535");
  }
  return String(parsed);
}

function ensureRpcPath(url: URL) {
  if (!url.pathname || url.pathname === "/") {
    url.pathname = "/rpc";
    return;
  }
  if (!url.pathname.includes("/rpc")) {
    url.pathname = `/rpc${url.pathname.replace(/\/+$/, "")}`;
  }
}

export function normalizeServerUrl(input: string) {
  const trimmed = input.trim();
  if (!trimmed) return localServerUrl;

  const candidate = websocketCandidate(trimmed);
  const url = new URL(candidate);
  if (!url.hostname) {
    throw new Error("Server host is required");
  }

  if (!url.port) {
    url.port = defaultServerPort;
  }
  ensureRpcPath(url);
  return url.toString();
}

export function normalizeWorkspaceFormServerUrl(form: WorkspaceFormState) {
  if (form.advanced) {
    return normalizeServerUrl(form.serverUrl);
  }

  const host = form.host.trim();
  if (!host) {
    throw new Error("Server host is required");
  }
  if (hasScheme(host)) {
    return normalizeServerUrl(host);
  }

  const protocol = form.secure ? "wss" : "ws";
  const url = new URL(`${protocol}://${host}`);
  if (!url.hostname) {
    throw new Error("Server host is required");
  }
  url.port = normalizePort(form.port);
  ensureRpcPath(url);
  return url.toString();
}

export function workspaceFormFromServerUrl(
  name: string,
  serverUrl: string,
): WorkspaceFormState {
  const normalized = normalizeServerUrl(serverUrl);
  const url = new URL(normalized);
  return {
    name,
    host: url.hostname || defaultServerHost,
    port: url.port || defaultServerPort,
    secure: url.protocol === "wss:",
    advanced: false,
    serverUrl: normalized,
  };
}

export function defaultWorkspaceForm(): WorkspaceFormState {
  return workspaceFormFromServerUrl("Local", localServerUrl);
}
