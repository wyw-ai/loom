export const MOBILE_CONNECT_PAYLOAD_VERSION = "1";

export type MobileConnectMode = "self" | "invite";

export type MobileSelfConnectConfig = {
  /** Omitted for backwards compatibility with the original payload builder. */
  mode?: "self";
  serverUrl: string;
  actorId: string;
  displayName: string;
};

export type MobileInviteConnectConfig = {
  mode: "invite";
  serverUrl: string;
};

export type MobileConnectConfig = MobileSelfConnectConfig | MobileInviteConnectConfig;

/**
 * Build the versioned deep-link encoded by the desktop connection QR code.
 *
 * The mobile client can treat this as connection configuration rather than a
 * navigation link. URLSearchParams keeps WebSocket URLs and display names
 * unambiguous while the version allows the payload to grow compatibly.
 */
export function buildMobileConnectPayload(config: MobileConnectConfig): string {
  const serverUrl = config.serverUrl.trim();
  if (!serverUrl) throw new Error("Server URL is required");

  const params = new URLSearchParams({
    v: MOBILE_CONNECT_PAYLOAD_VERSION,
    mode: config.mode ?? "self",
    serverUrl,
  });

  // Invite payloads deliberately use an allowlist. Do not copy arbitrary
  // fields from config: a QR code shared with another person must never carry
  // the desktop user's identity.
  if (config.mode === "invite") {
    return `loom://connect?${params.toString()}`;
  }

  const actorId = config.actorId.trim();
  const displayName = config.displayName.trim() || actorId;
  if (!actorId) throw new Error("Actor ID is required");

  params.set("actorId", actorId);
  params.set("displayName", displayName);
  return `loom://connect?${params.toString()}`;
}

export function isLoopbackServerUrl(serverUrl: string): boolean {
  try {
    const hostname = new URL(serverUrl).hostname.toLowerCase();
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1" || hostname === "[::1]";
  } catch {
    return false;
  }
}
