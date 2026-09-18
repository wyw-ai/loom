export type ServerAuthErrorKind = "required" | "invalid";

export function serverAuthErrorKind(error: unknown): ServerAuthErrorKind | null {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("LOOM_AUTH_INVALID") || message.includes("code -32011")) {
    return "invalid";
  }
  if (message.includes("LOOM_AUTH_REQUIRED") || message.includes("code -32010")) {
    return "required";
  }
  return null;
}
