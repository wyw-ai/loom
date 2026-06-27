import { localServerUrl } from "@/lib/constants";
import type { WorkspaceFormState } from "@/lib/types";

export const defaultServerPort = "7878";
export const serverHostPlaceholder = "your-server-host";
export const serverUrlPreviewPlaceholder = `ws://${serverHostPlaceholder}:${defaultServerPort}/rpc`;

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

  const url = new URL(`ws://${host}`);
  if (!url.hostname) {
    throw new Error("Server host is required");
  }
  if (!url.port) {
    url.port = defaultServerPort;
  }
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
    host: url.host,
    advanced: false,
    serverUrl: normalized,
  };
}

export function defaultWorkspaceForm(): WorkspaceFormState {
  return {
    name: "Local",
    host: "",
    advanced: false,
    serverUrl: "",
  };
}
