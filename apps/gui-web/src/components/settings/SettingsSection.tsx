import type { ReactNode } from "react";

export function SettingsSection({
  title,
  detail,
  action,
  children,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-[#dfe3ec] bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-bold text-[#111827]">{title}</div>
          <div className="mt-1 text-sm text-[#667085]">{detail}</div>
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
