//! Memory diagnostics.
//!
//! IMPORTANT (per docs/architecture.md "Memory Diagnostics"): this
//! module performs an INFORMATIONAL CHECK only — it reads what the
//! kernel reports about memory. It does NOT run an actual memory test
//! (e.g. pattern writes/reads to detect bad cells). A future opt-in
//! `memtest` module may add that; it must remain explicit and optional
//! because it is slow and, unlike everything here, actually exercises
//! the RAM rather than just reading kernel-reported metadata.
//!
//! We never claim RAM is physically healthy merely because
//! /proc/meminfo parsed successfully.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::list_dir_names;
use serde::Serialize;
use std::fs;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/memory_info`.
#[derive(Debug, Default, Serialize)]
pub struct MemoryInfo {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_free_bytes: Option<u64>,
    pub memory_online_blocks: usize,
    pub memory_offline_blocks: usize,
    /// ECC support cannot be determined from generic /proc or /sys
    /// interfaces without vendor-specific EDAC drivers; None means
    /// "not determined", never "no ECC".
    pub ecc: Option<bool>,
}

/// Parses `/proc/meminfo` lines of the form `Key:    12345 kB`.
fn parse_meminfo() -> std::collections::HashMap<String, u64> {
    let mut map = std::collections::HashMap::new();
    let Ok(content) = fs::read_to_string("/proc/meminfo") else {
        return map;
    };
    for line in content.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value_str = rest.trim().trim_end_matches("kB").trim();
        if let Ok(kb) = value_str.parse::<u64>() {
            map.insert(key.trim().to_string(), kb * 1024); // normalize to bytes
        }
    }
    map
}

/// EDAC (Error Detection And Correction) sysfs presence is the closest
/// generic signal for ECC memory without vendor tools. Presence of
/// /sys/devices/system/edac/mc/mc* entries strongly suggests ECC is
/// active; absence is inconclusive (could be non-ECC, or ECC without
/// EDAC driver support), so we return None rather than false in that case.
fn detect_ecc_hint() -> Option<bool> {
    let controllers = list_dir_names("/sys/devices/system/edac/mc");
    if controllers.iter().any(|c| c.starts_with("mc")) {
        Some(true)
    } else {
        None
    }
}

pub fn collect() -> (MemoryInfo, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let meminfo = parse_meminfo();

    let total_bytes = meminfo.get("MemTotal").copied();
    let available_bytes = meminfo.get("MemAvailable").copied();
    let free_bytes = meminfo.get("MemFree").copied();
    let swap_total_bytes = meminfo.get("SwapTotal").copied();
    let swap_free_bytes = meminfo.get("SwapFree").copied();

    let online_blocks = list_dir_names("/sys/devices/system/memory")
        .into_iter()
        .filter(|n| n.starts_with("memory"))
        .count();

    let info = MemoryInfo {
        total_bytes,
        available_bytes,
        free_bytes,
        swap_total_bytes,
        swap_free_bytes,
        memory_online_blocks: online_blocks,
        memory_offline_blocks: 0, // refined once we cross-check per-block "state" files
        ecc: detect_ecc_hint(),
    };

    let elapsed = timer.elapsed_ms();

    match total_bytes {
        Some(total) => {
            let total_gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
            results.push(DiagnosticResult::new(
                "memory_identify",
                "memory",
                Status::Pass,
                Severity::Info,
                format!("Total memory reported: {total_gb:.2} GiB (informational check only, no memory test performed)"),
                elapsed,
            ));
        }
        None => {
            results.push(DiagnosticResult::new(
                "memory_identify",
                "memory",
                Status::Fail,
                Severity::Error,
                "Could not read /proc/meminfo".to_string(),
                elapsed,
            ));
        }
    }

    // Low-available-memory is only meaningful at runtime, not really a
    // "health" signal this early in boot, but flag it if it's very low
    // relative to total (could indicate a system already under memory
    // pressure from something odd happening pre-boot, e.g. a huge
    // initramfs).
    if let (Some(total), Some(avail)) = (total_bytes, available_bytes) {
        let ratio = avail as f64 / total as f64;
        if ratio < 0.05 {
            results.push(DiagnosticResult::new(
                "memory_pressure",
                "memory",
                Status::Warn,
                Severity::Warning,
                format!("Available memory is only {:.1}% of total", ratio * 100.0),
                elapsed,
            ));
        }
    }

    (info, results)
}
