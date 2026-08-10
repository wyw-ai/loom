import {
  useEffect,
  useState,
} from "react";
import type { ComponentType } from "react";
import { HostDetailSection, HostInfoRow } from "@/components/shared/UIComponents";
import { HostMetric } from "@/components/settings/MachineComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useI18n } from "@/lib/i18n";
import { ArrowLeft, FileText, Settings, Split } from "lucide-react";
import type { MachineInfo } from "@/ipc/types";
import type { ServiceDetailTab, ServiceMemberEntry } from "@/lib/types";

export function ServiceListItem({
  entry,
  selected,
  onSelect,
}: {
  entry: ServiceMemberEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  const service = entry.service;

  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-colors",
        selected
          ? "border-[#bdb7ff] bg-[#f6f4ff] shadow-sm"
          : "border-transparent bg-transparent hover:border-[#dfe3ec] hover:bg-white",
      )}
      onClick={onSelect}
    >
      <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-[#edf0f5] bg-white text-[#503ed4]">
        <Split size={16} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-bold text-[#111827]">
            {serviceDisplayName(service)}
          </span>
          <span className={cn("h-2 w-2 shrink-0 rounded-full", serviceStatusDotClass(service))} />
        </span>
        <span className="mt-0.5 block truncate text-xs text-[#667085]">
          {t(service.kind)} / {entry.machine.name}
        </span>
      </span>
    </button>
  );
}

export function ServiceRosterOverview({
  entries,
  machines,
  onSelectService,
}: {
  entries: ServiceMemberEntry[];
  machines: MachineInfo[];
  onSelectService: (entry: ServiceMemberEntry) => void;
}) {
  const { t } = useI18n();
  const schedulerCount = entries.filter((entry) => entry.service.kind === "scheduler").length;
  const autostartCount = entries.filter((entry) => entry.service.autostart !== false).length;
  const hostCount = new Set(entries.map((entry) => entry.machine.id)).size;

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="min-w-0">
            <h2 className="text-xl font-bold text-[#111827]">{t("Services")}</h2>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
              <span>{t("{{count}} registered", { count: entries.length })}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{t("{{count}} scheduler", { count: schedulerCount })}</span>
              <span className="text-[#a0a6b3]">/</span>
              <span>{t("{{count}} hosts", { count: hostCount })}</span>
            </div>
          </div>
          <div className="flex flex-wrap items-start gap-6">
            <HostMetric label={t("Services")} value={entries.length} />
            <HostMetric label={t("Autostart")} value={autostartCount} />
            <HostMetric label={t("Hosts")} value={hostCount} />
          </div>
        </div>
      </section>

      <HostDetailSection title={t("Service Roster")} count={entries.length}>
        {entries.length === 0 ? (
          <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
            {t("No services registered.")}
          </div>
        ) : (
          <div className="space-y-2">
            {entries.map((entry) => (
              <button
                key={`${entry.machine.id}:${entry.service.id}`}
                type="button"
                className="w-full rounded-xl border border-[#edf0f5] bg-[#fbfbfd] px-4 py-3 text-left transition-colors hover:border-[#c8c1ff] hover:bg-white"
                onClick={() => onSelectService(entry)}
              >
                <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_160px_160px_110px] lg:items-center">
                  <div className="min-w-0 flex items-start gap-3">
                    <span className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-[#edf0f5] bg-white text-[#503ed4]">
                      <Split size={16} />
                    </span>
                    <div className="min-w-0">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-bold text-[#111827]">
                          {serviceDisplayName(entry.service)}
                        </span>
                        <Badge variant="secondary">{t(entry.service.kind)}</Badge>
                        <Badge variant={entry.service.autostart === false ? "warning" : "success"}>
                          {t(entry.service.autostart === false ? "manual" : "autostart")}
                        </Badge>
                      </div>
                      <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                        {entry.service.id}
                      </div>
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div className="text-[11px] font-semibold uppercase tracking-wide text-[#9aa1ae]">
                      {t("Host")}
                    </div>
                    <div className="mt-1 truncate text-sm font-semibold text-[#303849]">
                      {entry.machine.name}
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div className="text-[11px] font-semibold uppercase tracking-wide text-[#9aa1ae]">
                      {t("Lifecycle")}
                    </div>
                    <div className="mt-1 truncate text-sm font-semibold text-[#303849]">
                      {t(serviceLifecycleLabel(entry.service))}
                    </div>
                  </div>
                  <div className="text-right text-xs font-semibold text-[#503ed4] lg:text-left">
                    {t("Inspect")}
                  </div>
                </div>
              </button>
            ))}
          </div>
        )}
      </HostDetailSection>

      <HostDetailSection title={t("Host Coverage")} count={machines.length}>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {machines.length === 0 ? (
            <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
              {t("No managed hosts.")}
            </div>
          ) : (
            machines.map((machine) => (
              <div
                key={machine.id}
                className="rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-4"
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-bold text-[#111827]">
                      {machine.name}
                    </div>
                    <div className="mt-1 truncate text-xs text-[#667085]">
                      {t("{{runtimes}} runtimes / {{agents}} agents", {
                        runtimes: machine.providers.length,
                        agents: machine.agentCount,
                      })}
                    </div>
                  </div>
                  <Badge variant={machine.serviceCount > 0 ? "success" : "outline"}>
                    {t("{{count}} services", { count: machine.serviceCount })}
                  </Badge>
                </div>
              </div>
            ))
          )}
        </div>
      </HostDetailSection>
    </div>
  );
}

export function ServiceMemberDetail({
  entry,
  onBack,
}: {
  entry: ServiceMemberEntry;
  onBack: () => void;
}) {
  const { t } = useI18n();
  const { machine, service } = entry;
  const [activeTab, setActiveTab] = useState<ServiceDetailTab>("overview");
  const serviceKey = `${machine.id}:${service.id}`;
  const tabs: Array<{
    id: ServiceDetailTab;
    label: string;
    icon: ComponentType<{ size?: string | number; className?: string }>;
  }> = [
    { id: "overview", label: t("Overview"), icon: Split },
    { id: "spec", label: t("Spec"), icon: FileText },
    { id: "config", label: t("Config"), icon: Settings },
  ];

  useEffect(() => {
    setActiveTab("overview");
  }, [serviceKey]);

  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          className="mb-4 rounded-lg px-2 text-[#596174] hover:bg-[#f5f3ff] hover:text-[#503ed4]"
        >
          <ArrowLeft size={15} />
          {t("All Services")}
        </Button>
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="flex min-w-0 items-start gap-4">
            <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f83f7] to-[#4e3ad5] text-white shadow-sm">
              <Split size={25} />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-xl font-bold text-[#111827]">
                {serviceDisplayName(service)}
              </h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-[#667085]">
                <span className={cn("h-2 w-2 rounded-full", serviceStatusDotClass(service))} />
                <span>{t(service.autostart === false ? "Manual" : "Autostart")}</span>
                <span className="text-[#a0a6b3]">/</span>
                <span className="font-mono text-xs">{service.id}</span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge variant="secondary">{t(service.kind)}</Badge>
                <Badge variant="outline">{t(serviceLifecycleLabel(service))}</Badge>
                <Badge variant={service.autostart === false ? "warning" : "success"}>
                  {t(service.autostart === false ? "manual" : "autostart")}
                </Badge>
                <Badge variant="secondary">{machine.name}</Badge>
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-start gap-6">
            <HostMetric label={t("Jobs")} value={serviceJobCount(service)} />
            <HostMetric label={t("Config Keys")} value={serviceConfigKeyCount(service)} />
            <HostMetric label={t("Host Services")} value={machine.serviceCount} />
          </div>
        </div>
      </section>

      <div className="border-b border-[#dfe3ec] bg-[#fbfbfd] px-6 pt-4 lg:px-8">
        <div className="flex flex-wrap gap-2">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            const selected = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                className={cn(
                  "flex h-10 items-center gap-2 rounded-t-lg border border-b-0 px-3 text-sm font-semibold transition-colors",
                  selected
                    ? "border-[#dfe3ec] bg-white text-[#503ed4]"
                    : "border-transparent text-[#596174] hover:border-[#dfe3ec] hover:bg-white",
                )}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={15} />
                {tab.label}
              </button>
            );
          })}
        </div>
      </div>

      {activeTab === "overview" && (
        <>
          <HostDetailSection title={t("Runtime Summary")}>
            <div className="divide-y divide-[#edf0f5]">
              <HostInfoRow label={t("Display Name")}>
                {serviceDisplayName(service)}
              </HostInfoRow>
              <HostInfoRow label={t("Service ID")} mono>
                {service.id}
              </HostInfoRow>
              <HostInfoRow label={t("Actor ID")} mono>
                {service.actor?.id || t("Not set")}
              </HostInfoRow>
              <HostInfoRow label={t("Kind")}>
                {service.kind ? t(service.kind) : t("Not set")}
              </HostInfoRow>
              <HostInfoRow label={t("Lifecycle")}>
                {t(serviceLifecycleLabel(service))}
              </HostInfoRow>
              <HostInfoRow label={t("Autostart")}>
                {t(service.autostart === false ? "Off" : "On")}
              </HostInfoRow>
              <HostInfoRow label={t("Host")}>
                {machine.name}
              </HostInfoRow>
            </div>
          </HostDetailSection>

          <HostDetailSection title={t("Scheduler Jobs")} count={serviceJobs(service).length}>
            {serviceJobs(service).length === 0 ? (
              <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                {t("No scheduler jobs declared in this service config.")}
              </div>
            ) : (
              <div className="space-y-2">
                {serviceJobs(service).map((job, index) => (
                  <div
                    key={serviceJobId(job, index)}
                    className="rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-4"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-bold text-[#111827]">
                          {serviceJobId(job, index)}
                        </div>
                        <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                          {serviceJobCommand(job) || t("No command source")}
                        </div>
                      </div>
                      <Badge variant="outline">
                        {serviceJobSchedule(job) || t("unscheduled")}
                      </Badge>
                    </div>
                    <div className="mt-3 grid gap-2 text-xs text-[#667085] md:grid-cols-3">
                      <div>
                        <div className="font-semibold uppercase tracking-wide text-[#9aa1ae]">
                          {t("Target")}
                        </div>
                        <div className="mt-1 truncate font-medium text-[#303849]">
                          {serviceJobTarget(job) || t("Not set")}
                        </div>
                      </div>
                      <div>
                        <div className="font-semibold uppercase tracking-wide text-[#9aa1ae]">
                          {t("Scope")}
                        </div>
                        <div className="mt-1 truncate font-medium text-[#303849]">
                          {serviceJobScope(job) || t("Not set")}
                        </div>
                      </div>
                      <div>
                        <div className="font-semibold uppercase tracking-wide text-[#9aa1ae]">
                          {t("Dedupe")}
                        </div>
                        <div className="mt-1 truncate font-medium text-[#303849]">
                          {serviceJobDedupe(job) || t("Not set")}
                        </div>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </HostDetailSection>
        </>
      )}

      {activeTab === "spec" && (
        <HostDetailSection title={t("Service Spec")}>
          <JsonInspector value={service} />
        </HostDetailSection>
      )}

      {activeTab === "config" && (
        <HostDetailSection title={t("Config Payload")}>
          <JsonInspector value={serviceConfigValue(service)} />
        </HostDetailSection>
      )}
    </div>
  );
}

function serviceDisplayName(service: MachineInfo["services"][number]) {
  return service.displayName || service.actor?.displayName || service.id;
}

function serviceLifecycleLabel(service: MachineInfo["services"][number]) {
  const value = typeof service.lifecycle === "string" ? service.lifecycle : "";
  return value || "default";
}

function serviceStatusDotClass(service: MachineInfo["services"][number]) {
  return service.autostart === false ? "bg-amber-500" : "bg-emerald-500";
}

function serviceConfigValue(service: MachineInfo["services"][number]) {
  return service.config ?? {};
}

function serviceConfigKeyCount(service: MachineInfo["services"][number]) {
  const config = serviceConfigValue(service);
  return isPlainObject(config) ? Object.keys(config).length : 0;
}

function serviceJobs(service: MachineInfo["services"][number]): Record<string, unknown>[] {
  const config = serviceConfigValue(service);
  if (!isPlainObject(config)) return [];
  const jobs = config.jobs;
  if (!Array.isArray(jobs)) return [];
  return jobs.filter(isPlainObject);
}

function serviceJobCount(service: MachineInfo["services"][number]) {
  return serviceJobs(service).length;
}

function serviceJobId(job: Record<string, unknown>, index: number) {
  return typeof job.id === "string" && job.id.trim() ? job.id : `job_${index + 1}`;
}

function serviceJobSchedule(job: Record<string, unknown>) {
  return typeof job.schedule === "string" ? job.schedule : "";
}

function serviceJobTarget(job: Record<string, unknown>) {
  return typeof job.targetAgent === "string" ? job.targetAgent : "";
}

function serviceJobDedupe(job: Record<string, unknown>) {
  const dedupeBy = typeof job.dedupeBy === "string" ? job.dedupeBy : "";
  const cursorBy = typeof job.cursorBy === "string" ? job.cursorBy : "";
  return [dedupeBy, cursorBy].filter(Boolean).join(" / ");
}

function serviceJobScope(job: Record<string, unknown>) {
  const scope = job.scope;
  if (!isPlainObject(scope)) return "";
  const kind = typeof scope.kind === "string" ? scope.kind : "";
  const id = typeof scope.id === "string" ? scope.id : "";
  return [kind, id].filter(Boolean).join(":");
}

function serviceJobCommand(job: Record<string, unknown>) {
  const source = job.source;
  if (!isPlainObject(source)) return "";
  return typeof source.command === "string" ? source.command : "";
}

function JsonInspector({ value }: { value: unknown }) {
  return (
    <pre className="max-h-[520px] overflow-auto rounded-xl border border-[#edf0f5] bg-[#fbfbfd] p-4 font-mono text-xs leading-5 text-[#303849] soft-scrollbar">
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
