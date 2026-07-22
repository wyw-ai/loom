import { useCallback, useRef, useState } from "react";
import {
  fileToPendingAttachment,
  MAX_ATTACHMENTS,
  validateFile,
  type PendingAttachment,
} from "@/lib/attachment-utils";

export interface UseAttachmentsResult {
  attachments: PendingAttachment[];
  error: string | null;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  addFiles: (files: FileList | File[]) => Promise<void>;
  removeAttachment: (id: string) => void;
  clearAttachments: () => void;
  openFilePicker: () => void;
}

export function useAttachments(): UseAttachmentsResult {
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const addFiles = useCallback(async (files: FileList | File[]) => {
    const fileArray = Array.from(files);
    if (fileArray.length === 0) return;
    setError(null);

    const errors: string[] = [];
    const valid: File[] = [];

    for (const file of fileArray) {
      const err = validateFile(file);
      if (err) {
        errors.push(err);
      } else {
        valid.push(file);
      }
    }

    if (errors.length > 0) {
      setError(errors.join("; "));
    }

    if (valid.length === 0) return;

    setAttachments((current) => {
      const remaining = MAX_ATTACHMENTS - current.length;
      if (remaining <= 0) {
        setError(`Maximum ${MAX_ATTACHMENTS} attachments per message`);
        return current;
      }
      const toAdd = valid.slice(0, remaining);
      if (toAdd.length < valid.length) {
        setError(`Only ${remaining} more attachment${remaining === 1 ? "" : "s"} allowed (max ${MAX_ATTACHMENTS})`);
      }
      // Convert files synchronously-ish; we'll update state once all are ready
      Promise.all(toAdd.map(fileToPendingAttachment))
        .then((newAttachments) => {
          setAttachments((curr) => {
            const existingNames = new Set(curr.map((a) => a.name));
            const deduped = newAttachments.map((att) => {
              if (!existingNames.has(att.name)) {
                existingNames.add(att.name);
                return att;
              }
              // Auto-suffix: file.txt -> file(1).txt
              const dot = att.name.lastIndexOf(".");
              const base = dot > 0 ? att.name.slice(0, dot) : att.name;
              const ext = dot > 0 ? att.name.slice(dot) : "";
              let counter = 1;
              let candidate: string;
              do {
                candidate = `${base}(${counter})${ext}`;
                counter++;
              } while (existingNames.has(candidate));
              existingNames.add(candidate);
              return { ...att, name: candidate };
            });
            return [...curr, ...deduped];
          });
        })
        .catch((err) => {
          setError(`Failed to read file: ${err instanceof Error ? err.message : String(err)}`);
        });
      return current;
    });
  }, []);

  const removeAttachment = useCallback((id: string) => {
    setAttachments((current) => current.filter((a) => a.id !== id));
    setError(null);
  }, []);

  const clearAttachments = useCallback(() => {
    setAttachments([]);
    setError(null);
  }, []);

  const openFilePicker = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  return {
    attachments,
    error,
    fileInputRef,
    addFiles,
    removeAttachment,
    clearAttachments,
    openFilePicker,
  };
}
