"""
Pydantic models mirroring schemas/diagnostic-report.schema.json exactly.
Keep this file and the JSON schema in sync by hand — Phase 5 validates
incoming reports against these models (extra="forbid" == additionalProperties: false).
"""
from __future__ import annotations
from datetime import datetime
from enum import Enum
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, field_validator


class Status(str, Enum):
    PASS = "PASS"
    WARN = "WARN"
    FAIL = "FAIL"
    SKIPPED = "SKIPPED"
    UNKNOWN = "UNKNOWN"


class Severity(str, Enum):
    INFO = "INFO"
    WARNING = "WARNING"
    ERROR = "ERROR"
    CRITICAL = "CRITICAL"


class ScoreLabel(str, Enum):
    GOOD = "GOOD"
    WARNING = "WARNING"
    POOR = "POOR"
    CRITICAL = "CRITICAL"


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class DiagnosticResult(Strict):
    test: str
    component: str
    status: Status
    severity: Severity
    message: str
    duration_ms: int = Field(ge=0)


class DeviceIdentity(Strict):
    device_id: str
    hostname: Optional[str] = None


class Summary(Strict):
    total: int = Field(ge=0)
    passed: int = Field(ge=0)
    warned: int = Field(ge=0)
    failed: int = Field(ge=0)
    skipped: int = Field(ge=0)
    score: int = Field(ge=0, le=100)
    score_label: ScoreLabel


class CpuInfo(Strict):
    model: Optional[str] = None
    vendor: Optional[str] = None
    architecture: str
    physical_cores: Optional[int] = Field(default=None, ge=0)
    logical_threads: Optional[int] = Field(default=None, ge=0)
    online_cpus: list[int]
    offline_cpus: list[int]
    flags: list[str]
    current_freq_mhz: Optional[int] = Field(default=None, ge=0)
    max_freq_mhz: Optional[int] = Field(default=None, ge=0)
    governor: Optional[str] = None
    temperature_celsius: Optional[float] = None


class MemoryInfo(Strict):
    total_bytes: Optional[int] = Field(default=None, ge=0)
    available_bytes: Optional[int] = Field(default=None, ge=0)
    free_bytes: Optional[int] = Field(default=None, ge=0)
    swap_total_bytes: Optional[int] = Field(default=None, ge=0)
    swap_free_bytes: Optional[int] = Field(default=None, ge=0)
    memory_online_blocks: int = Field(ge=0)
    memory_offline_blocks: int = Field(ge=0)
    ecc: Optional[bool] = None


class KernelLogEntry(Strict):
    severity: Severity
    line: str


class KernelInfo(Strict):
    version: Optional[str] = None
    cmdline: Optional[str] = None
    tainted: bool
    taint_code: Optional[int] = Field(default=None, ge=0)
    log_entries: list[KernelLogEntry]


class PciDevice(Strict):
    address: str
    vendor_id: Optional[str] = None
    device_id: Optional[str] = None
    class_: Optional[str] = Field(default=None, alias="class")
    driver: Optional[str] = None
    link_speed: Optional[str] = None

    model_config = ConfigDict(extra="forbid", populate_by_name=True)


class StorageDevice(Strict):
    name: str
    model: Optional[str] = None
    size_bytes: Optional[int] = Field(default=None, ge=0)
    removable: bool
    is_nvme: bool
    smart_healthy: Optional[bool] = None


class GpuDevice(Strict):
    name: str
    vendor_id: Optional[str] = None
    device_id: Optional[str] = None
    driver: Optional[str] = None
    temperature_celsius: Optional[float] = None


class UsbDevice(Strict):
    bus_path: str
    vendor_id: Optional[str] = None
    product_id: Optional[str] = None
    manufacturer: Optional[str] = None
    product: Optional[str] = None
    speed_mbps: Optional[str] = None


class NetworkInterface(Strict):
    name: str
    oper_state: Optional[str] = None
    carrier: Optional[bool] = None
    mac_address: Optional[str] = None
    speed_mbps: Optional[int] = None
    is_wireless: bool


class SensorKind(str, Enum):
    TEMPERATURE = "TEMPERATURE"
    FAN = "FAN"
    VOLTAGE = "VOLTAGE"


class SensorReading(Strict):
    chip: str
    label: str
    kind: SensorKind
    value: float
    unit: str


class CpuSection(Strict):
    info: CpuInfo
    results: list[DiagnosticResult]


class MemorySection(Strict):
    info: MemoryInfo
    results: list[DiagnosticResult]


class KernelSection(Strict):
    info: KernelInfo
    results: list[DiagnosticResult]


class PciSection(Strict):
    info: list[PciDevice]
    results: list[DiagnosticResult]


class StorageSection(Strict):
    info: list[StorageDevice]
    results: list[DiagnosticResult]


class GpuSection(Strict):
    info: list[GpuDevice]
    results: list[DiagnosticResult]


class UsbSection(Strict):
    info: list[UsbDevice]
    results: list[DiagnosticResult]


class NetworkSection(Strict):
    info: list[NetworkInterface]
    results: list[DiagnosticResult]


class SensorsSection(Strict):
    info: list[SensorReading]
    results: list[DiagnosticResult]


class Sections(Strict):
    cpu: CpuSection
    memory: MemorySection
    kernel: KernelSection
    pci: PciSection
    storage: StorageSection
    # Phase 8 collectors — added here in Phase 11 to match the JSON schema
    # and agent, which have required these since Phase 8. Sections has
    # extra="forbid" (via Strict), so before this fix any report from a
    # Phase 8+ agent was rejected with 422 at the schema-validation step,
    # never reaching storage.
    gpu: GpuSection
    usb: UsbSection
    network: NetworkSection
    sensors: SensorsSection


class DiagnosticReport(Strict):
    schema_version: str = Field(pattern=r"^[0-9]+\.[0-9]+$")
    report_id: str
    boot_id: str
    agent_version: str
    generated_at: datetime
    device: DeviceIdentity
    summary: Summary
    sections: Sections

    @field_validator("schema_version")
    @classmethod
    def major_version_supported(cls, v: str) -> str:
        # Backend rejects reports whose MAJOR version it doesn't understand
        # (docs/architecture.md — report lifecycle). Minor bumps are additive.
        major = int(v.split(".")[0])
        from app.config import get_settings
        supported = get_settings().schema_major_version
        if major != supported:
            raise ValueError(
                f"unsupported schema major version {major}, backend supports {supported}"
            )
        return v


# ---- Request/response shapes for the 5 endpoints -----------------------

class RegisterDeviceRequest(Strict):
    device_id: str
    hostname: Optional[str] = None
    # Phase 7: base64 Ed25519 public key. Required for new devices;
    # ignored (not compared/updated) for already-registered ones — the
    # key is immutable once set, see app/security.py.
    public_key: str


class RegisterDeviceResponse(Strict):
    id: UUID
    device_id: str
    hostname: Optional[str]
    first_seen_at: datetime
    last_seen_at: datetime


class SubmitDiagnosticsResponse(Strict):
    report_id: str
    stored: bool
    ack: bool = True


class ReportSummaryOut(Strict):
    report_id: str
    boot_id: str
    generated_at: datetime
    received_at: datetime
    score: int
    score_label: ScoreLabel


class ReportOut(ReportSummaryOut):
    schema_version: str
    agent_version: str
    device_id: str
    raw_report: dict


class DeviceOut(Strict):
    id: UUID
    device_id: str
    hostname: Optional[str]
    first_seen_at: datetime
    last_seen_at: datetime
    report_count: int
    latest_report: Optional[ReportSummaryOut] = None


class HealthOut(Strict):
    status: str
    db: str


# ---- Phase 11: list endpoints backing the dashboard --------------------

class DeviceListOut(Strict):
    devices: list[DeviceOut]
    total: int
    limit: int
    offset: int


class ReportListOut(Strict):
    reports: list[ReportSummaryOut]
    total: int
    limit: int
    offset: int
