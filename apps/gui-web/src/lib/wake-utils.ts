import type { WakeSpec } from "@/ipc/types";

export function defaultWakeSpec(): WakeSpec {
  return {
    coalesce: true,
    debounceMs: 750,
    replyReminder: "first-turn",
    onHumanMessageWhileBusy: "queue",
    contextTokenBudget: 900,
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
  };
}
