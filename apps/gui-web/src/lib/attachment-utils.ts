/**
 * Attachment utilities for file selection, validation, and long-text conversion.
 *
 * Design per ARCH T3 spec (art_7627955bf61b):
 * - Long text threshold: 4000 chars → convert to .txt attachment
 * - Warning threshold: 3500 chars (show UI hint)
 * - Max file size: 25 MB per file
 * - Max attachments per message: 10
 * - Allowlist: text/image/pdf/data/archive
 * - Blocklist: executables (.exe/.bat/.cmd/.msi/.dll/.so/.dylib)
 */

export const LONG_TEXT_THRESHOLD = 4000;
export const LONG_TEXT_WARN_THRESHOLD = 3500;
export const MAX_FILE_SIZE_BYTES = 25 * 1024 * 1024; // 25 MB
export const MAX_ATTACHMENTS = 10;

const BLOCKED_EXTENSIONS = [
  ".exe",
  ".bat",
  ".cmd",
  ".msi",
  ".dll",
  ".so",
  ".dylib",
  ".sh",
  ".ps1",
  ".vbs",
  ".scr",
  ".com",
  ".app",
];

const ALLOWED_EXTENSIONS = [
  // Text
  ".txt",
  ".md",
  ".markdown",
  ".json",
  ".yaml",
  ".yml",
  ".toml",
  ".csv",
  ".tsv",
  ".xml",
  ".html",
  ".htm",
  ".css",
  ".js",
  ".ts",
  ".tsx",
  ".jsx",
  ".rs",
  ".py",
  ".go",
  ".java",
  ".c",
  ".cpp",
  ".h",
  ".hpp",
  ".sql",
  ".log",
  ".env",
  // Images
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".svg",
  ".bmp",
  ".ico",
  // Documents
  ".pdf",
  // Data
  ".db",
  ".sqlite",
  // Archives
  ".zip",
  ".tar",
  ".gz",
  ".tgz",
  ".bz2",
  ".7z",
];

export interface PendingAttachment {
  id: string;
  name: string;
  size: number;
  mediaType: string;
  /** Raw file bytes, read eagerly for upload. */
  bytes: Uint8Array;
}

/**
 * Metadata for an attachment that has been uploaded to the server.
 * Used to build the attachment summary block in message bodies.
 */
export interface UploadedAttachment {
  name: string;
  mediaType: string;
  size: number;
  artifactId: string;
}

export interface AttachmentValidationError {
  file: File;
  reason: string;
}

function fileExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot < 0) return "";
  return name.slice(dot).toLowerCase();
}

function detectMediaType(file: File, name: string): string {
  if (file.type && file.type !== "application/octet-stream") {
    return file.type;
  }
  const ext = fileExtension(name);
  const map: Record<string, string> = {
    ".txt": "text/plain",
    ".md": "text/markdown",
    ".markdown": "text/markdown",
    ".json": "application/json",
    ".yaml": "text/yaml",
    ".yml": "text/yaml",
    ".toml": "text/plain",
    ".csv": "text/csv",
    ".tsv": "text/tab-separated-values",
    ".xml": "text/xml",
    ".html": "text/html",
    ".htm": "text/html",
    ".css": "text/css",
    ".js": "text/javascript",
    ".ts": "text/typescript",
    ".tsx": "text/typescript",
    ".jsx": "text/javascript",
    ".rs": "text/rust",
    ".py": "text/x-python",
    ".go": "text/x-go",
    ".java": "text/x-java",
    ".c": "text/x-c",
    ".cpp": "text/x-c++",
    ".h": "text/x-c",
    ".hpp": "text/x-c++",
    ".sql": "application/sql",
    ".log": "text/plain",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".webp": "image/webp",
    ".svg": "image/svg+xml",
    ".bmp": "image/bmp",
    ".ico": "image/x-icon",
    ".pdf": "application/pdf",
    ".zip": "application/zip",
    ".tar": "application/x-tar",
    ".gz": "application/gzip",
    ".tgz": "application/gzip",
    ".bz2": "application/x-bzip2",
    ".7z": "application/x-7z-compressed",
  };
  return map[ext] ?? "application/octet-stream";
}

export function isBlockedFile(name: string): boolean {
  const ext = fileExtension(name);
  return BLOCKED_EXTENSIONS.includes(ext);
}

export function isAllowedFile(name: string): boolean {
  const ext = fileExtension(name);
  if (BLOCKED_EXTENSIONS.includes(ext)) return false;
  if (ALLOWED_EXTENSIONS.includes(ext)) return true;
  // Allow files with no extension only if they're text-like by content
  return false;
}

export function validateFile(file: File): string | null {
  if (isBlockedFile(file.name)) {
    return `${file.name}: executable files are not allowed`;
  }
  if (!isAllowedFile(file.name)) {
    return `${file.name}: file type not supported`;
  }
  if (file.size > MAX_FILE_SIZE_BYTES) {
    const mb = (file.size / (1024 * 1024)).toFixed(1);
    const maxMb = (MAX_FILE_SIZE_BYTES / (1024 * 1024)).toFixed(0);
    return `${file.name}: file is ${mb} MB, max is ${maxMb} MB`;
  }
  if (file.size === 0) {
    return `${file.name}: file is empty`;
  }
  return null;
}

export async function readFileBytes(file: File): Promise<Uint8Array> {
  const arrayBuffer = await file.arrayBuffer();
  return new Uint8Array(arrayBuffer);
}

export async function fileToPendingAttachment(file: File): Promise<PendingAttachment> {
  const bytes = await readFileBytes(file);
  return {
    id: `pending_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`,
    name: file.name,
    size: file.size,
    mediaType: detectMediaType(file, file.name),
    bytes,
  };
}

/**
 * Returns true if the draft text exceeds the long-text threshold and should
 * be converted to a .txt attachment instead of sent inline.
 */
export function shouldConvertLongText(draftLength: number): boolean {
  return draftLength >= LONG_TEXT_THRESHOLD;
}

/**
 * Returns true if the draft text is approaching the threshold and a UI
 * warning should be shown.
 */
export function shouldWarnLongText(draftLength: number): boolean {
  return draftLength >= LONG_TEXT_WARN_THRESHOLD && draftLength < LONG_TEXT_THRESHOLD;
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  for (const unit of units) {
    if (value < 1024) return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
    value /= 1024;
  }
  return `${value.toFixed(1)} TB`;
}

// ---------------------------------------------------------------------------
// Paste / clipboard support (ARCH D3 design)
// ---------------------------------------------------------------------------

/**
 * Extract files from a clipboard DataTransfer object (paste event).
 * Returns an empty array when the clipboard contains no files (pure text paste).
 */
export function extractFilesFromClipboard(clipboardData: DataTransfer): File[] {
  const files: File[] = [];
  if (!clipboardData || !clipboardData.items) return files;
  for (let i = 0; i < clipboardData.items.length; i++) {
    const item = clipboardData.items[i];
    if (item.kind === "file") {
      const file = item.getAsFile();
      if (file) files.push(file);
    }
  }
  return files;
}

/**
 * Ensure a pasted file has a reasonable name.
 * Screenshots often arrive with empty or generic names — auto-generate
 * `paste-<timestamp>.png` in that case.
 */
export function ensurePasteFileName(file: File): File {
  if (file.name && file.name.trim().length > 0 && file.name !== "image.png") {
    return file;
  }
  const ext = fileExtension(file.name || "image.png") || ".png";
  const timestamp = Date.now();
  const newName = `paste-${timestamp}${ext}`;
  // File objects are read-only — wrap in a new File with the corrected name
  return new File([file], newName, { type: file.type });
}

/**
 * Build the `[附件]` summary block appended to message bodies so that
 * AI actors can perceive attachments from the text alone.
 *
 * Format:
 * ```
 * {user text}
 *
 * [附件]
 * - report.pdf (application/pdf, 100KB) [artifact: art_xxx]
 * ```
 *
 * Returns an empty string when there are no attachments (backward compatible).
 */
export function buildAttachmentSummaryBlock(attachments: UploadedAttachment[]): string {
  if (attachments.length === 0) return "";
  const lines = attachments.map(
    (att) => `- ${att.name} (${att.mediaType}, ${formatFileSize(att.size)}) [artifact: ${att.artifactId}]`,
  );
  return `\n\n[附件]\n${lines.join("\n")}`;
}

/**
 * Copy an attachment blob to the system clipboard via the async Clipboard API.
 * Uses ClipboardItem when available; silently degrades on unsupported browsers
 * or non-image MIME types that ClipboardItem cannot handle.
 */
export async function copyAttachmentToClipboard(
  blob: Blob,
  mediaType: string,
): Promise<void> {
  const w = window as typeof window & {
    ClipboardItem?: typeof ClipboardItem;
  };
  if (!w.ClipboardItem || !navigator.clipboard?.write) {
    return; // silent degradation
  }
  try {
    const item = new w.ClipboardItem({ [mediaType]: blob });
    await navigator.clipboard.write([item]);
  } catch {
    // ClipboardItem may reject unsupported MIME types — silent degradation
  }
}
