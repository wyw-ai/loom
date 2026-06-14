import { FileText } from "lucide-react";
import { attachmentTitle, attachmentKind } from "@/lib/message-utils";

function AttachmentCard({ attachment }: { attachment: string }) {
  const title = attachmentTitle(attachment);
  const kind = attachmentKind(attachment);
  return (
    <div className="attachment-card">
      <div className="attachment-icon">
        <FileText size={18} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-bold text-[#303849]">{title}</div>
        <div className="mt-0.5 truncate text-xs font-medium text-[#667085]">{kind}</div>
      </div>
      <div className="attachment-preview" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

export function AttachmentStack({ attachments }: { attachments: string[] }) {
  return (
    <div className="mt-3 grid max-w-[560px] gap-2">
      {attachments.slice(0, 3).map((attachment) => (
        <AttachmentCard key={attachment} attachment={attachment} />
      ))}
    </div>
  );
}
