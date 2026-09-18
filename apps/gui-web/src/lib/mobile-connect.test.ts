import { describe, expect, it } from "vitest";
import {
  buildMobileConnectPayload,
  isLoopbackServerUrl,
  MOBILE_CONNECT_PAYLOAD_VERSION,
} from "@/lib/mobile-connect";
import type { MobileConnectConfig } from "@/lib/mobile-connect";

describe("buildMobileConnectPayload", () => {
  it("encodes a versioned mobile connection deep-link", () => {
    const payload = buildMobileConnectPayload({
      serverUrl: "ws://canfuu.com:7878/rpc",
      actorId: "actor_human_local_canfeng",
      displayName: "Can Feng",
    });
    const parsed = new URL(payload);

    expect(parsed.protocol).toBe("loom:");
    expect(parsed.hostname).toBe("connect");
    expect(parsed.searchParams.get("v")).toBe(MOBILE_CONNECT_PAYLOAD_VERSION);
    expect(parsed.searchParams.get("mode")).toBe("self");
    expect(parsed.searchParams.get("serverUrl")).toBe("ws://canfuu.com:7878/rpc");
    expect(parsed.searchParams.get("actorId")).toBe("actor_human_local_canfeng");
    expect(parsed.searchParams.get("displayName")).toBe("Can Feng");
  });

  it("trims fields and falls back to the actor id for a blank display name", () => {
    const parsed = new URL(buildMobileConnectPayload({
      serverUrl: "  wss://loom.example/rpc  ",
      actorId: "  actor_human_alice  ",
      displayName: "   ",
    }));

    expect(parsed.searchParams.get("serverUrl")).toBe("wss://loom.example/rpc");
    expect(parsed.searchParams.get("actorId")).toBe("actor_human_alice");
    expect(parsed.searchParams.get("displayName")).toBe("actor_human_alice");
  });

  it("encodes an explicit self mode with the current identity", () => {
    const parsed = new URL(buildMobileConnectPayload({
      mode: "self",
      serverUrl: "wss://loom.example/rpc",
      actorId: "actor_human_alice",
      displayName: "Alice",
    }));

    expect(parsed.searchParams.get("mode")).toBe("self");
    expect(parsed.searchParams.get("actorId")).toBe("actor_human_alice");
    expect(parsed.searchParams.get("displayName")).toBe("Alice");
  });

  it("allowlists invite fields and never serializes the current identity", () => {
    const configWithAccidentalIdentity = {
      mode: "invite",
      serverUrl: "  wss://loom.example/rpc  ",
      actorId: "actor_human_secret",
      displayName: "Secret Person",
    } as unknown as MobileConnectConfig;
    const parsed = new URL(buildMobileConnectPayload(configWithAccidentalIdentity));

    expect(Object.fromEntries(parsed.searchParams)).toEqual({
      v: MOBILE_CONNECT_PAYLOAD_VERSION,
      mode: "invite",
      serverUrl: "wss://loom.example/rpc",
    });
    expect(parsed.searchParams.has("actorId")).toBe(false);
    expect(parsed.searchParams.has("displayName")).toBe(false);
  });

  it("rejects missing required connection fields", () => {
    expect(() => buildMobileConnectPayload({
      serverUrl: "",
      actorId: "actor_human_alice",
      displayName: "Alice",
    })).toThrow("Server URL is required");
    expect(() => buildMobileConnectPayload({
      serverUrl: "ws://loom.example/rpc",
      actorId: "",
      displayName: "Alice",
    })).toThrow("Actor ID is required");
    expect(() => buildMobileConnectPayload({
      mode: "invite",
      serverUrl: "  ",
    })).toThrow("Server URL is required");
  });
});

describe("isLoopbackServerUrl", () => {
  it.each([
    "ws://127.0.0.1:7878/rpc",
    "ws://localhost:7878/rpc",
    "ws://[::1]:7878/rpc",
  ])("recognizes loopback address %s", (serverUrl) => {
    expect(isLoopbackServerUrl(serverUrl)).toBe(true);
  });

  it("does not mark a remote server as loopback", () => {
    expect(isLoopbackServerUrl("ws://canfuu.com:7878/rpc")).toBe(false);
  });
});
