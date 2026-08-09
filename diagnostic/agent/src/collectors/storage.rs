//! Storage device diagnostics.
//!
//! Source: /sys/block/ (per docs/architecture.md "Storage Diagnostics").
//! This is a purely read-only enumeration of block devices already
//! known to the kernel — it never issues any write, TRIM, secure-erase,
//! or destructive SMART self-test command to any disk. SMART *health*
//! (read-only) via `smartctl -H` is attempted opportunistically and
//! skipped cleanly if the tool isn't present.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_trimmed, read_u64};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/storage_device`.
#[derive(Debug, Clone, Serialize)]
pub struct StorageDevice {
    pub name: String,
    pub model: Option<String>,
    pub size_bytes: Option<u64>,
    pub removable: bool,
    pub is_nvme: bool,
    /// Coarse SMART overall-health verdict from `smartctl -H`, when the
    /// tool is available. None means "not determined", never "healthy".
    pub smart_healthy: Option<bool>,
}

/// Block devices we don't want to report as "storage" for diagnostic
/// purposes: loop devices, ram disks, device-mapper internals.
fn is_diagnostic_relevant(name: &str) -> bool {
    !(name.starts_with("loop")
        || name.starts_with("ram")
        || name.starts_with("dm-")
        || name.starts_with("sr")) // optical drives: listed separately if needed later
}

/// Best-effort, read-only SMART overall-health check. Returns None if
/// smartctl isn't installed or the check can't be run — this is
/// expected in minimal initramfs environments until smartmontools is
/// vendored into the build (tracked for a later phase).
fn smart_health(device_name: &str) -> Option<bool> {
    let path = format!("/dev/{device_name}");
    let output = std::process::Command::new("smartctl")
        .args(["-H", "-j", &path])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Avoid pulling in a JSON parser dependency for one boolean in Phase 3;
    // smartctl's -j output reliably contains this literal substring.
    if text.contains("\"passed\": true") {
        Some(true)
    } else if text.contains("\"passed\": false") {
        Some(false)
    } else {
        None
    }
}

pub fn collect() -> (Vec<StorageDevice>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let all_names = list_dir_names("/sys/block");
    let relevant: Vec<String> = all_names.into_iter().filter(|n| is_diagnostic_relevant(n)).collect();

    let mut devices = Vec::new();
    for name in &relevant {
        let base = format!("/sys/block/{name}");

        let model = read_trimmed(format!("{base}/device/model"));
        // /sys/block/<dev>/size is in 512-byte sectors.
        let size_bytes = read_u64(format!("{base}/size")).map(|sectors| sectors * 512);
        let removable = read_trimmed(format!("{base}/removable")).as_deref() == Some("1");
        let is_nvme = name.starts_with("nvme");

        let smart_healthy = smart_health(name);

        devices.push(StorageDevice {
            name: name.clone(),
            model,
            size_bytes,
            removable,
            is_nvme,
            smart_healthy,
        });
    }

    let elapsed = timer.elapsed_ms();

    if devices.is_empty() {
        results.push(DiagnosticResult::new(
            "storage_enumerate",
            "storage",
            Status::Skipped,
            Severity::Info,
            "No block devices found under /sys/block".to_string(),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "storage_enumerate",
            "storage",
            Status::Pass,
            Severity::Info,
            format!("Enumerated {} storage device(s)", devices.len()),
            elapsed,
        ));
    }

    for dev in &devices {
        match dev.smart_healthy {
            Some(true) => results.push(DiagnosticResult::new(
                "storage_smart_health",
                "storage",
                Status::Pass,
                Severity::Info,
                format!("{}: SMART overall health PASSED", dev.name),
                elapsed,
            )),
            Some(false) => results.push(DiagnosticResult::new(
                "storage_smart_health",
                "storage",
                Status::Fail,
                Severity::Critical,
                format!("{}: SMART overall health FAILED", dev.name),
                elapsed,
            )),
            None => results.push(DiagnosticResult::new(
                "storage_smart_health",
                "storage",
                Status::Skipped,
                Severity::Info,
                format!("{}: SMART health not determined (smartctl unavailable or unsupported device)", dev.name),
                elapsed,
            )),
        }
    }

    (devices, results)
}
