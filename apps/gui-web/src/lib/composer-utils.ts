/** Composer resize constants and helpers. */

/** Minimum composer height in pixels (single-line comfort height). */
export const COMPOSER_MIN_HEIGHT = 44;

/** Maximum composer height as a fraction of viewport height. */
export const COMPOSER_MAX_HEIGHT_RATIO = 0.5;

/** Default height cap in pixels for auto-grow before switching to scroll. */
export const COMPOSER_DEFAULT_CAP = 52;

/** Maximum rows the textarea grows to in auto mode before scrolling. */
export const COMPOSER_AUTO_MAX_ROWS = 5;

/** Thread composer uses smaller constraints. */
export const THREAD_COMPOSER_MIN_HEIGHT = 42;
export const THREAD_COMPOSER_DEFAULT_CAP = 80;

/** Composer type key for persistence. */
export type ComposerType = "channel" | "thread";

/** localStorage key prefix for persisted composer heights. */
const STORAGE_KEY = "loom:composer-height";

/** Resolve max height in pixels from the viewport ratio. */
export function composerMaxHeightPx(): number {
  return Math.floor(window.innerHeight * COMPOSER_MAX_HEIGHT_RATIO);
}

/** Read persisted height for a composer type, or null if unset/invalid. */
export function loadComposerHeight(type: ComposerType): number | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Record<string, number>;
    const value = parsed[type];
    if (typeof value === "number" && value > 0) return value;
    return null;
  } catch {
    return null;
  }
}

/** Persist height for a composer type. */
export function saveComposerHeight(type: ComposerType, height: number): void {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const existing = raw ? (JSON.parse(raw) as Record<string, number>) : {};
    existing[type] = height;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(existing));
  } catch {
    // Ignore storage errors (quota, private mode, etc.)
  }
}

/** Clear persisted height for a composer type (reset to auto). */
export function clearComposerHeight(type: ComposerType): void {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const existing = JSON.parse(raw) as Record<string, number>;
    delete existing[type];
    localStorage.setItem(STORAGE_KEY, JSON.stringify(existing));
  } catch {
    // Ignore
  }
}
