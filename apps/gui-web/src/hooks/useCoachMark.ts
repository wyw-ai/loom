import { useCallback, useEffect, useState } from "react";

const coachMarkPrefix = "loom.coachmark.";

function readDismissed(key: string) {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(key) === "dismissed";
  } catch {
    return false;
  }
}

function writeDismissed(key: string) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(key, "dismissed");
  } catch {
    /* local-only preference; ignore quota or privacy-mode failures */
  }
}

export function useCoachMark(id: string, active = true) {
  const key = `${coachMarkPrefix}${id}`;
  const [dismissed, setDismissed] = useState(() => readDismissed(key));

  useEffect(() => {
    setDismissed(readDismissed(key));
  }, [key]);

  const dismiss = useCallback(() => {
    setDismissed(true);
    writeDismissed(key);
  }, [key]);

  return {
    visible: active && !dismissed,
    dismiss,
  };
}
