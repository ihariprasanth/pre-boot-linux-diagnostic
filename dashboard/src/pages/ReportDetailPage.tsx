import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ApiError, getReport } from "../api";
import { BootLine } from "../components/BootLine";
import { KeyValueGrid } from "../components/KeyValueGrid";
import { StatusPill } from "../components/StatusPill";
import { LoadingState, ErrorState } from "../components/AsyncState";
import { SectionPanel } from "../components/SectionPanel";
import {
  CpuInfoView,
  GpuInfoView,
  KernelInfoView,
  MemoryInfoView,
  NetworkInfoView,
  PciInfoView,
  SensorsInfoView,
  StorageInfoView,
  UsbInfoView,
} from "../components/SectionInfo";
import { absoluteTime, truncateId } from "../lib/format";
import type { ReportDetail } from "../types";

export function ReportDetailPage() {
  const { reportId = "" } = useParams();
  const [report, setReport] = useState<ReportDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setReport(null);
    setError(null);
    getReport(reportId)
      .then((r) => !cancelled && setReport(r))
      .catch((e) => !cancelled && setError(e instanceof ApiError ? e.message : "failed to load report"));
    return () => {
      cancelled = true;
    };
  }, [reportId]);

  if (error) {
    return (
      <>
        <p className="breadcrumbs">
          <Link to="/">devices</Link> / report
        </p>
        <ErrorState message={error} />
      </>
    );
  }

  if (!report) {
    return (
      <>
        <BootLine>{`fetching report ${truncateId(reportId)}\u2026`}</BootLine>
        <LoadingState label="reading report\u2026" />
      </>
    );
  }

  const s = report.raw_report.summary;
  const sections = report.raw_report.sections;

  return (
    <>
      <p className="breadcrumbs">
        <Link to="/">devices</Link> /{" "}
        <Link to={`/devices/${encodeURIComponent(report.device_id)}`}>{truncateId(report.device_id)}</Link> / report
      </p>
      <BootLine>{`report ${truncateId(report.report_id)} \u2014 boot ${truncateId(report.boot_id, 8, 4)}`}</BootLine>

      <h1 className="page-title">
        <StatusPill label={report.score_label} score={report.score} />
      </h1>
      <p className="page-sub">
        {s.passed} passed &middot; {s.warned} warned &middot; {s.failed} failed &middot; {s.skipped} skipped &middot;{" "}
        {s.total} total checks
      </p>

      <section className="panel">
        <div className="panel-header">
          <h2>report</h2>
        </div>
        <div className="panel-body">
          <KeyValueGrid
            items={[
              { label: "report id", value: report.report_id, dim: true },
              { label: "boot id", value: report.boot_id, dim: true },
              { label: "generated at", value: absoluteTime(report.generated_at) },
              { label: "received at", value: absoluteTime(report.received_at) },
              { label: "schema version", value: report.schema_version },
              { label: "agent version", value: report.agent_version },
            ]}
          />
        </div>
      </section>

      <div style={{ marginTop: 20 }}>
        <SectionPanel title="cpu" results={sections.cpu.results}>
          <CpuInfoView info={sections.cpu.info} />
        </SectionPanel>
        <SectionPanel title="memory" results={sections.memory.results}>
          <MemoryInfoView info={sections.memory.info} />
        </SectionPanel>
        <SectionPanel title="kernel" results={sections.kernel.results}>
          <KernelInfoView info={sections.kernel.info} />
        </SectionPanel>
        <SectionPanel title="storage" results={sections.storage.results}>
          <StorageInfoView info={sections.storage.info} />
        </SectionPanel>
        <SectionPanel title="pci" results={sections.pci.results}>
          <PciInfoView info={sections.pci.info} />
        </SectionPanel>
        <SectionPanel title="gpu" results={sections.gpu.results}>
          <GpuInfoView info={sections.gpu.info} />
        </SectionPanel>
        <SectionPanel title="network" results={sections.network.results}>
          <NetworkInfoView info={sections.network.info} />
        </SectionPanel>
        <SectionPanel title="usb" results={sections.usb.results}>
          <UsbInfoView info={sections.usb.info} />
        </SectionPanel>
        <SectionPanel title="sensors" results={sections.sensors.results}>
          <SensorsInfoView info={sections.sensors.info} />
        </SectionPanel>
      </div>
    </>
  );
}
