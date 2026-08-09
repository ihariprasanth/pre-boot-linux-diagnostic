//! Assembles every collector's output into the final JSON diagnostic
//! report and serializes it.
//!
//! This module owns the *shape* of the report — it mirrors
//! `schemas/diagnostic-report.schema.json` field-for-field, and that
//! file is the source of truth the backend (Phase 5) will validate
//! against. If you change a field here, update the schema (and bump
//! `SCHEMA_VERSION` if the change isn't backwards compatible) in the
//! same commit.
//!
//! What this module deliberately does NOT do yet:
//!   - decide device identity/auth (`device_id` here is a placeholder,
//!     non-cryptographic best-effort value — Phase 7 replaces it with
//!     the privacy-conscious hashed identity from docs/architecture.md
//!     "Security model")
//!   - sign or encrypt anything (Phase 7)
//!   - send the report anywhere (Phase 5/6)
//!
//! `build_report()` is a pure function (no I/O) so it's unit-testable
//! without touching `/proc` or `/sys` — all the I/O (reading IDs,
//! getting the current time) lives in the small helper functions below
//! it, which `main.rs` calls separately.

use serde::Serialize;

use crate::collectors::cpu::CpuInfo;
use crate::collectors::gpu::GpuDevice;
use crate::collectors::kernel::KernelInfo;
use crate::collectors::memory::MemoryInfo;
use crate::collectors::network::NetworkInterface;
use crate::collectors::pci::PciDevice;
use crate::collectors::sensors::SensorReading;
use crate::collectors::storage::StorageDevice;
use crate::collectors::usb::UsbDevice;
use crate::result::{DiagnosticResult, Severity, Status};
use crate::sysfs::read_trimmed;

/// Schema version this agent build emits. Bump on any breaking change
/// to `schemas/diagnostic-report.schema.json`'s required shape; additive,
/// backwards-compatible fields don't need a bump.
pub const SCHEMA_VERSION: &str = "1.0";

/// Agent binary version, taken from Cargo.toml at compile time — kept
/// separate from `SCHEMA_VERSION` because the agent can gain features
/// (e.g. Phase 8 collectors) without the report *shape* changing.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// See `schemas/diagnostic-report.schema.json#/$defs/device_identity`.
#[derive(Debug, Serialize)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub hostname: Option<String>,
}

/// See `schemas/diagnostic-report.schema.json#/properties/summary`.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
    pub skipped: usize,
    pub score: i64,
    pub score_label: String,
}

/// Generic "one collector's info + the DiagnosticResults it produced"
/// pairing, reused for every section so we don't hand-write five
/// near-identical structs.
#[derive(Debug, Serialize)]
pub struct CollectorSection<I> {
    pub info: I,
    pub results: Vec<DiagnosticResult>,
}

/// See `schemas/diagnostic-report.schema.json#/properties/sections`.
#[derive(Debug, Serialize)]
pub struct Sections {
    pub cpu: CollectorSection<CpuInfo>,
    pub memory: CollectorSection<MemoryInfo>,
    pub kernel: CollectorSection<KernelInfo>,
    pub pci: CollectorSection<Vec<PciDevice>>,
    pub storage: CollectorSection<Vec<StorageDevice>>,
    /// Phase 8 sections. Additive/optional in the schema so older report
    /// consumers built against the Phase 4-7 shape don't break.
    pub gpu: CollectorSection<Vec<GpuDevice>>,
    pub usb: CollectorSection<Vec<UsbDevice>>,
    pub network: CollectorSection<Vec<NetworkInterface>>,
    pub sensors: CollectorSection<Vec<SensorReading>>,
}

/// The top-level document. See `schemas/diagnostic-report.schema.json`.
#[derive(Debug, Serialize)]
pub struct DiagnosticReport {
    pub schema_version: String,
    pub report_id: String,
    pub boot_id: String,
    pub agent_version: String,
    pub generated_at: String,
    pub device: DeviceIdentity,
    pub summary: Summary,
    pub sections: Sections,
}

/// Simple, transparent scoring per docs/architecture.md "Overall Health
/// Score": critical/error findings cost the most, warnings a little,
/// and skipped/unavailable-but-optional checks cost nothing. Moved here
/// (from main.rs) in Phase 4 since it's now part of what goes in the
/// report, not just the console summary.
pub fn compute_score(results: &[DiagnosticResult]) -> i64 {
    let mut score: i64 = 100;
    for r in results {
        match (r.status, r.severity) {
            (Status::Fail, Severity::Critical) => score -= 30,
            (Status::Fail, Severity::Error) | (Status::Fail, _) => score -= 20,
            (Status::Warn, Severity::Error) => score -= 10,
            (Status::Warn, _) => score -= 5,
            (Status::Skipped, _) | (Status::Unknown, _) => {} // no penalty — unavailable != unhealthy
            (Status::Pass, _) => {}
        }
    }
    score.clamp(0, 100)
}

pub fn score_label(score: i64) -> &'static str {
    match score {
        90..=100 => "GOOD",
        70..=89 => "WARNING",
        40..=69 => "POOR",
        _ => "CRITICAL",
    }
}

/// Everything a single collector hands back: its typed info plus the
/// DiagnosticResults it emitted. Just a tuple alias to keep
/// `build_report`'s signature readable.
pub type Collected<I> = (I, Vec<DiagnosticResult>);

/// Pure assembly: takes already-collected data (no I/O) and produces
/// the final report. Kept separate from any `/proc`/`/sys` reads so it
/// can be unit-tested with synthetic data on any machine.
#[allow(clippy::too_many_arguments)]
pub fn build_report(
    device: DeviceIdentity,
    boot_id: String,
    report_id: String,
    generated_at: String,
    cpu: Collected<CpuInfo>,
    memory: Collected<MemoryInfo>,
    kernel: Collected<KernelInfo>,
    pci: Collected<Vec<PciDevice>>,
    storage: Collected<Vec<StorageDevice>>,
    gpu: Collected<Vec<GpuDevice>>,
    usb: Collected<Vec<UsbDevice>>,
    network: Collected<Vec<NetworkInterface>>,
    sensors: Collected<Vec<SensorReading>>,
) -> DiagnosticReport {
    let mut all_results: Vec<&DiagnosticResult> = Vec::new();
    all_results.extend(&cpu.1);
    all_results.extend(&memory.1);
    all_results.extend(&kernel.1);
    all_results.extend(&pci.1);
    all_results.extend(&storage.1);
    all_results.extend(&gpu.1);
    all_results.extend(&usb.1);
    all_results.extend(&network.1);
    all_results.extend(&sensors.1);

    let total = all_results.len();
    let passed = all_results.iter().filter(|r| r.status == Status::Pass).count();
    let warned = all_results.iter().filter(|r| r.status == Status::Warn).count();
    let failed = all_results.iter().filter(|r| r.status == Status::Fail).count();
    let skipped = all_results.iter().filter(|r| r.status == Status::Skipped).count();

    let owned_results: Vec<DiagnosticResult> = all_results.into_iter().cloned().collect();
    let score = compute_score(&owned_results);

    let summary = Summary {
        total,
        passed,
        warned,
        failed,
        skipped,
        score,
        score_label: score_label(score).to_string(),
    };

    DiagnosticReport {
        schema_version: SCHEMA_VERSION.to_string(),
        report_id,
        boot_id,
        agent_version: AGENT_VERSION.to_string(),
        generated_at,
        device,
        summary,
        sections: Sections {
            cpu: CollectorSection { info: cpu.0, results: cpu.1 },
            memory: CollectorSection { info: memory.0, results: memory.1 },
            kernel: CollectorSection { info: kernel.0, results: kernel.1 },
            pci: CollectorSection { info: pci.0, results: pci.1 },
            storage: CollectorSection { info: storage.0, results: storage.1 },
            gpu: CollectorSection { info: gpu.0, results: gpu.1 },
            usb: CollectorSection { info: usb.0, results: usb.1 },
            network: CollectorSection { info: network.0, results: network.1 },
            sensors: CollectorSection { info: sensors.0, results: sensors.1 },
        },
    }
}

/// Serializes a report to pretty-printed JSON. The only fallible step
/// in report assembly (barring an OOM-class error, serde_json won't
/// fail on these plain-data types in practice) — kept as a Result
/// rather than unwrapped so main.rs can degrade gracefully instead of
/// panicking a pre-boot process over a formatting bug.
pub fn to_json_pretty(report: &DiagnosticReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Linux exposes a real per-boot random UUID at this path that stays
/// constant for the lifetime of the running kernel and changes on
/// every reboot — exactly the semantics `boot_id` needs, with no extra
/// state file required. Falls back to a fresh report-scoped ID (not
/// stable across an /init retry) if unavailable, e.g. on non-Linux dev
/// hosts running `cargo test`.
pub fn read_boot_id() -> String {
    read_trimmed("/proc/sys/kernel/random/boot_id").unwrap_or_else(fallback_id)
}

/// Each read of `/proc/sys/kernel/random/uuid` returns a fresh random
/// UUID — used here to give each individual report submission its own
/// `report_id`, distinct from the per-boot `boot_id` above.
pub fn generate_report_id() -> String {
    read_trimmed("/proc/sys/kernel/random/uuid").unwrap_or_else(fallback_id)
}

/// Best-effort, dependency-free fallback ID for environments without
/// `/proc/sys/kernel/random/*` (e.g. some containers/dev hosts). NOT
/// cryptographically random — only used when the kernel-backed source
/// above is unavailable, and never for anything security-relevant
/// (that's Phase 7's device-keypair/signing work, not this ID).
fn fallback_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("fallback-{:032x}", nanos)
}

/// RFC3339/ISO8601 UTC timestamp, e.g. `2026-08-08T12:34:56.789Z`.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{DiagnosticResult, Severity, Status};

    fn r(status: Status, severity: Severity) -> DiagnosticResult {
        DiagnosticResult::new("t", "c", status, severity, "m", 0)
    }

    #[test]
    fn score_starts_at_100_with_no_results() {
        assert_eq!(compute_score(&[]), 100);
    }

    #[test]
    fn critical_failure_costs_30() {
        assert_eq!(compute_score(&[r(Status::Fail, Severity::Critical)]), 70);
    }

    #[test]
    fn skipped_never_costs_anything() {
        assert_eq!(compute_score(&[r(Status::Skipped, Severity::Critical)]), 100);
    }

    #[test]
    fn score_never_goes_below_zero() {
        let results: Vec<_> = (0..10).map(|_| r(Status::Fail, Severity::Critical)).collect();
        assert_eq!(compute_score(&results), 0);
    }

    #[test]
    fn score_label_matches_expected_bands() {
        assert_eq!(score_label(100), "GOOD");
        assert_eq!(score_label(75), "WARNING");
        assert_eq!(score_label(50), "POOR");
        assert_eq!(score_label(10), "CRITICAL");
    }

    /// Builds a report from synthetic collector output (no /proc or
    /// /sys reads) and checks the summary counts + JSON shape, so this
    /// test behaves identically on any machine/CI runner.
    #[test]
    fn build_report_aggregates_counts_and_serializes() {
        let cpu = (
            CpuInfo::default(),
            vec![r(Status::Pass, Severity::Info)],
        );
        let memory = (
            MemoryInfo::default(),
            vec![r(Status::Warn, Severity::Warning)],
        );
        let kernel = (
            KernelInfo::default(),
            vec![r(Status::Fail, Severity::Critical)],
        );
        let pci = (Vec::<PciDevice>::new(), vec![r(Status::Skipped, Severity::Info)]);
        let storage = (Vec::<StorageDevice>::new(), vec![r(Status::Pass, Severity::Info)]);
        let gpu = (Vec::<GpuDevice>::new(), vec![r(Status::Skipped, Severity::Info)]);
        let usb = (Vec::<UsbDevice>::new(), vec![r(Status::Skipped, Severity::Info)]);
        let network = (Vec::<NetworkInterface>::new(), vec![r(Status::Skipped, Severity::Info)]);
        let sensors = (Vec::<SensorReading>::new(), vec![r(Status::Skipped, Severity::Info)]);

        let report = build_report(
            DeviceIdentity { device_id: "test-device".to_string(), hostname: None },
            "boot-123".to_string(),
            "report-456".to_string(),
            "2026-01-01T00:00:00.000Z".to_string(),
            cpu,
            memory,
            kernel,
            pci,
            storage,
            gpu,
            usb,
            network,
            sensors,
        );

        assert_eq!(report.schema_version, SCHEMA_VERSION);
        assert_eq!(report.summary.total, 9);
        assert_eq!(report.summary.passed, 2);
        assert_eq!(report.summary.warned, 1);
        assert_eq!(report.summary.failed, 1);
        assert_eq!(report.summary.skipped, 5);
        // one CRITICAL fail (-30) + one WARNING warn (-5) = 65
        assert_eq!(report.summary.score, 65);
        assert_eq!(report.summary.score_label, "POOR");

        let json = to_json_pretty(&report).expect("synthetic report must serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
        // Spot-check the required top-level keys from
        // schemas/diagnostic-report.schema.json rather than the whole
        // tree, so this test doesn't have to be rewritten every time an
        // optional field is added.
        for key in [
            "schema_version",
            "report_id",
            "boot_id",
            "agent_version",
            "generated_at",
            "device",
            "summary",
            "sections",
        ] {
            assert!(value.get(key).is_some(), "missing top-level key: {key}");
        }
        assert_eq!(value["sections"]["cpu"]["results"][0]["status"], "PASS");
    }

    #[test]
    fn fallback_id_is_never_empty() {
        assert!(!fallback_id().is_empty());
    }
}
