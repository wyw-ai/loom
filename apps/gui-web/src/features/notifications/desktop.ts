import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

let granted = false;
let pendingPermission: Promise<boolean> | null = null;

async function canNotify(): Promise<boolean> {
  if (granted) return true;
  pendingPermission ??= (async () => {
    try {
      if (await isPermissionGranted()) return true;
      return (await requestPermission()) === "granted";
    } catch {
      return false;
    }
  })();
  granted = await pendingPermission;
  pendingPermission = null;
  return granted;
}

export async function notifyDesktop(title: string, body: string) {
  if (!(await canNotify())) return;
  try {
    sendNotification({ title, body });
  } catch {
    /* System notification support is best-effort. */
  }
}
