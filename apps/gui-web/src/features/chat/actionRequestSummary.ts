import type { ActionChoice } from "@/ipc/types";

export interface ActionRequestSummary {
  title: string;
  description: string;
  reason?: string;
  command?: string;
  rawInput?: string;
  requestId?: string;
  choices: ActionChoice[];
}

export function summarizeActionRequest(
  payload: Record<string, unknown>,
): ActionRequestSummary {
  const title = asString(payload.title) || "(action)";
  const description = asString(payload.description);
  const requestId = asString(payload.requestId) || asString(payload.actionId);
  const choices = Array.isArray(payload.choices)
    ? (payload.choices as ActionChoice[])
    : [];

  const toolCall = parseToolCall(payload, description);
  const rawInput = asRecord(toolCall?.rawInput) ?? asRecord(payload.rawInput);
  const reason = asString(rawInput?.reason);
  const command = extractCommand(rawInput) ?? extractCommand(asRecord(toolCall));
  const toolKind = asString(toolCall?.kind);

  if (reason || command) {
    return {
      title: title.startsWith("Permission required:")
        ? "Permission required"
        : title,
      description: compactDescription(reason, command),
      reason,
      command,
      requestId,
      choices,
    };
  }

  if (toolKind === "other" && rawInput) {
    const rawInputText = formatRawInput(rawInput);
    return {
      title: asString(toolCall?.title) || stripPermissionPrefix(title),
      description: rawInputText,
      rawInput: rawInputText,
      requestId,
      choices,
    };
  }

  return {
    title,
    description,
    requestId,
    choices,
  };
}

function stripPermissionPrefix(title: string) {
  return title.replace(/^Permission required:\s*/i, "") || title;
}

function formatRawInput(rawInput: Record<string, unknown>) {
  try {
    return JSON.stringify(rawInput, null, 2);
  } catch {
    return String(rawInput);
  }
}

function parseToolCall(
  payload: Record<string, unknown>,
  description: string,
): Record<string, unknown> | undefined {
  const direct = asRecord(payload.toolCall) ?? asRecord(payload.content);
  if (direct) return direct;
  if (!description.trim().startsWith("{")) return undefined;
  try {
    return asRecord(JSON.parse(description));
  } catch {
    return undefined;
  }
}

function extractCommand(source: Record<string, unknown> | undefined) {
  if (!source) return undefined;
  const parsed = source.parsed_cmd;
  if (Array.isArray(parsed)) {
    const first = asRecord(parsed[0]);
    const cmd = asString(first?.cmd);
    if (cmd) return cmd;
  }

  const command = source.command;
  if (typeof command === "string") return command;
  if (!Array.isArray(command)) return undefined;
  const parts = command.map((part) => String(part));
  if (parts.length >= 3 && isShell(parts[0]) && parts[1] === "-lc") {
    return parts.slice(2).join(" ");
  }
  return parts.map(shellQuote).join(" ");
}

function compactDescription(reason?: string, command?: string) {
  return [reason, command ? `$ ${command}` : undefined]
    .filter(Boolean)
    .join("\n");
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function isShell(binary: string) {
  return /(?:^|\/)(?:zsh|bash|sh)$/.test(binary);
}

function shellQuote(value: string) {
  return /^[A-Za-z0-9_/:=.,@%+-]+$/.test(value)
    ? value
    : `'${value.replace(/'/g, "'\\''")}'`;
}
