import { describe, expect, it } from "vitest";
import { serverAuthErrorKind } from "./server-auth";

describe("serverAuthErrorKind", () => {
  it("recognizes native and JSON-RPC password errors", () => {
    expect(serverAuthErrorKind("LOOM_AUTH_REQUIRED: server password required")).toBe("required");
    expect(serverAuthErrorKind(new Error("rpc failed (code -32011)"))).toBe("invalid");
  });

  it("ignores ordinary transport failures", () => {
    expect(serverAuthErrorKind(new Error("connection refused"))).toBeNull();
  });
});
