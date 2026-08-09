/**
 * Mirrors server/app/schemas.py and schemas/diagnostic-report.schema.json.
 * Keep in sync by hand, same discipline the backend uses to track the
 * agent's Rust types — see the note at the top of schemas.py.
 */

export type ScoreLabel = "GOOD" | "WARNING" | "POOR" | "CRITICAL";
export type ResultStatus = "PASS" | "WARN" | "FAIL" | "SKIPPED" | "UNKNOWN";
export type Severity = "INFO" | "WARNING" | "ERROR" | "CRITICAL";
export type SensorKind = "TEMPERATURE" | "FAN" | "VOLTAGE";

export interface ReportSummary {
  report_id: string;
  boot_id: string;
  generated_at: string;
  received_at: string;
  score: number;
  score_label: ScoreLabel;
}

export interface Device {
  id: string;
  device_id: string;
  hostname: string | null;
  first_seen_at: string;
  last_seen_at: string;
  report_count: number;
  latest_report: ReportSummary | null;
}

export interface DeviceList {
  devices: Device[];
  total: number;
  limit: number;
  offset: number;
}

export interface ReportList {
  reports: ReportSummary[];
  total: number;
  limit: number;
  offset: number;
}

export interface DiagnosticResult {
  test: string;
  component: string;
  status: ResultStatus;
  severity: Severity;
  message: string;
  duration_ms: number;
}

export interface CpuInfo {
  model: string | null;
  vendor: string | null;
  architecture: string;
  physical_cores: number | null;
  logical_threads: number | null;
  online_cpus: number[];
  offline_cpus: number[];
  flags: string[];
  current_freq_mhz: number | null;
  max_freq_mhz: number | null;
  governor: string | null;
  temperature_celsius: number | null;
}

export interface MemoryInfo {
  total_bytes: number | null;
  available_bytes: number | null;
  free_bytes: number | null;
  swap_total_bytes: number | null;
  swap_free_bytes: number | null;
  memory_online_blocks: number;
  memory_offline_blocks: number;
  ecc: boolean | null;
}

export interface KernelLogEntry {
  severity: Severity;
  line: string;
}

export interface KernelInfo {
  version: string | null;
  cmdline: string | null;
  tainted: boolean;
  taint_code: number | null;
  log_entries: KernelLogEntry[];
}

export interface PciDevice {
  address: string;
  vendor_id: string | null;
  device_id: string | null;
  class?: string | null;
  driver: string | null;
  link_speed: string | null;
}

export interface StorageDevice {
  name: string;
  model: string | null;
  size_bytes: number | null;
  removable: boolean;
  is_nvme: boolean;
  smart_healthy: boolean | null;
}

export interface GpuDevice {
  name: string;
  vendor_id: string | null;
  device_id: string | null;
  driver: string | null;
  temperature_celsius: number | null;
}

export interface UsbDevice {
  bus_path: string;
  vendor_id: string | null;
  product_id: string | null;
  manufacturer: string | null;
  product: string | null;
  speed_mbps: string | null;
}

export interface NetworkInterface {
  name: string;
  oper_state: string | null;
  carrier: boolean | null;
  mac_address: string | null;
  speed_mbps: number | null;
  is_wireless: boolean;
}

export interface SensorReading {
  chip: string;
  label: string;
  kind: SensorKind;
  value: number;
  unit: string;
}

export interface Section<TInfo> {
  info: TInfo;
  results: DiagnosticResult[];
}

export interface Sections {
  cpu: Section<CpuInfo>;
  memory: Section<MemoryInfo>;
  kernel: Section<KernelInfo>;
  pci: Section<PciDevice[]>;
  storage: Section<StorageDevice[]>;
  gpu: Section<GpuDevice[]>;
  usb: Section<UsbDevice[]>;
  network: Section<NetworkInterface[]>;
  sensors: Section<SensorReading[]>;
}

export interface Summary {
  total: number;
  passed: number;
  warned: number;
  failed: number;
  skipped: number;
  score: number;
  score_label: ScoreLabel;
}

export interface DeviceIdentity {
  device_id: string;
  hostname: string | null;
}

export interface RawDiagnosticReport {
  schema_version: string;
  report_id: string;
  boot_id: string;
  agent_version: string;
  generated_at: string;
  device: DeviceIdentity;
  summary: Summary;
  sections: Sections;
}

export interface ReportDetail extends ReportSummary {
  schema_version: string;
  agent_version: string;
  device_id: string;
  raw_report: RawDiagnosticReport;
}

export interface Health {
  status: string;
  db: string;
}
