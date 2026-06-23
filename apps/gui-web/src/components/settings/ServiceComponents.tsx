import { HostDetailSection } from "@/components/shared/UIComponents";

export function ServiceRosterOverview() {
  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <h2 className="text-xl font-bold text-[#111827]">Services</h2>
        <div className="mt-2 text-sm text-[#667085]">0 registered</div>
      </section>
      <HostDetailSection title="Service Roster" count={0}>
        <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
          No services registered.
        </div>
      </HostDetailSection>
    </div>
  );
}
