import type { WakeSpec } from "@/ipc/types";

export function defaultWakeSpec(): WakeSpec {
  return {
    coalesce: true,
    debounceMs: 750,
    replyReminder: "first-turn",
    onHumanMessageWhileBusy: "queue",
    contextTokenBudget: 900,
    turnInputStyle: "minimal",
  };
}

export function normalizeWakeSpec(wake: WakeSpec | null | undefined): WakeSpec {
  const defaults = defaultWakeSpec();
  return {
    coalesce: wake?.coalesce ?? defaults.coalesce,
    debounceMs: wake?.debounceMs ?? defaults.debounceMs,
    replyReminder: wake?.replyReminder ?? defaults.replyReminder,
    onHumanMessageWhileBusy: wake?.onHumanMessageWhileBusy ?? defaults.onHumanMessageWhileBusy,
    contextTokenBudget: wake?.contextTokenBudget ?? defaults.contextTokenBudget,
    turnInputStyle: wake?.turnInputStyle ?? defaults.turnInputStyle,
  };
}

// ---------------------------------------------------------------------------
// Wake presets — one dropdown instead of five raw fields (analysis doc §5.2).
// Each preset maps onto the raw WakeSpec fields; "custom" appears when the
// current spec doesn't match any preset (edited via Advanced).
// ---------------------------------------------------------------------------

export type WakePresetId = "queue-merge" | "queue-serial" | "interrupt-append";

export interface WakePreset {
  id: WakePresetId;
  label: string;
  description: string;
  /** Fields the preset controls; other WakeSpec fields are left untouched. */
  patch: Pick<WakeSpec, "coalesce" | "debounceMs" | "onHumanMessageWhileBusy">;
}

export const WAKE_PRESETS: WakePreset[] = [
  {
    id: "queue-merge",
    label: "Queue + merge (recommended)",
    description:
      "New messages queue behind the running turn; queued messages are merged into one turn so the agent answers a burst at once.",
    patch: { coalesce: true, debounceMs: 750, onHumanMessageWhileBusy: "queue" },
  },
  {
    id: "queue-serial",
    label: "Queue + one by one",
    description:
      "New messages queue behind the running turn and each message gets its own full turn, in order.",
    patch: { coalesce: false, debounceMs: 0, onHumanMessageWhileBusy: "queue" },
  },
  {
    id: "interrupt-append",
    label: "Interrupt + append",
    description:
      "A new human message cancels the running turn; the interrupted work is re-queued and merged with the new message so the agent restarts with the latest intent.",
    patch: {
      coalesce: true,
      debounceMs: 250,
      onHumanMessageWhileBusy: "cancel_and_requeue",
    },
  },
];

/** Identify which preset (if any) the given wake spec matches. */
export function wakePresetIdFor(wake: WakeSpec | null | undefined): WakePresetId | "custom" {
  const normalized = normalizeWakeSpec(wake);
  for (const preset of WAKE_PRESETS) {
    if (
      (normalized.coalesce ?? true) === (preset.patch.coalesce ?? true) &&
      (normalized.debounceMs ?? 0) === (preset.patch.debounceMs ?? 0) &&
      (normalized.onHumanMessageWhileBusy ?? "queue") ===
        (preset.patch.onHumanMessageWhileBusy ?? "queue")
    ) {
      return preset.id;
    }
  }
  return "custom";
}

/** Apply a preset onto an existing wake spec, preserving unrelated fields. */
export function applyWakePreset(
  wake: WakeSpec | null | undefined,
  presetId: WakePresetId,
): WakeSpec {
  const preset = WAKE_PRESETS.find((p) => p.id === presetId);
  const normalized = normalizeWakeSpec(wake);
  if (!preset) return normalized;
  return { ...normalized, ...preset.patch };
}
