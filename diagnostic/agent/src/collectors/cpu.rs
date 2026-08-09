//! CPU diagnostics.
//!
//! Sources (per docs/architecture.md section "CPU Diagnostics"):
//!   /proc/cpuinfo
//!   /sys/devices/system/cpu/
//!   /sys/class/thermal/
//!
//! Every field is best-effort: a system that doesn't expose e.g.
//! frequency scaling or thermal zones gets `None` there, not a failure
//! of the whole collector.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_trimmed, read_u64};
use serde::Serialize;
use std::fs;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/cpu_info`.
#[derive(Debug, Default, Serialize)]
pub struct CpuInfo {
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub architecture: String,
    pub physical_cores: Option<u32>,
    pub logical_threads: Option<u32>,
    pub online_cpus: Vec<u32>,
    pub offline_cpus: Vec<u32>,
    pub flags: Vec<String>,
    pub current_freq_mhz: Option<u64>,
    pub max_freq_mhz: Option<u64>,
    pub governor: Option<String>,
    pub temperature_celsius: Option<f64>,
}

/// Parses `/proc/cpuinfo`'s repeated `key : value` blocks. Returns the
/// model name / vendor / flags from the first CPU block (they're
/// homogeneous on virtually all real systems) and a count of blocks
/// (== logical thread count).
fn parse_cpuinfo() -> (Option<String>, Option<String>, Vec<String>, u32) {
    let content = match fs::read_to_string("/proc/cpuinfo") {
        Ok(c) => c,
        Err(_) => return (None, None, Vec::new(), 0),
    };

    let mut model = None;
    let mut vendor = None;
    let mut flags = Vec::new();
    let mut thread_count = 0u32;

    for line in content.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "processor" => thread_count += 1,
            "model name" if model.is_none() => model = Some(value.to_string()),
            "vendor_id" if vendor.is_none() => vendor = Some(value.to_string()),
            "flags" | "Features" if flags.is_empty() => {
                flags = value.split_whitespace().map(|s| s.to_string()).collect();
            }
            _ => {}
        }
    }

    (model, vendor, flags, thread_count)
}

/// Parses a CPU list file like `/sys/devices/system/cpu/online`, which
/// uses ranges: "0-3,6,8-9".
fn parse_cpu_list(raw: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in raw.trim().split(',') {
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.parse::<u32>(), end.parse::<u32>()) {
                out.extend(s..=e);
            }
        } else if let Ok(n) = part.parse::<u32>() {
            out.push(n);
        }
    }
    out
}

/// Best-effort thermal zone read: looks for a zone whose type mentions
/// "cpu" or "x86_pkg_temp"; falls back to the first zone found if none
/// match by name. Values in /sys/class/thermal are millidegrees C.
fn read_cpu_temperature() -> Option<f64> {
    let zones = list_dir_names("/sys/class/thermal");
    let mut fallback: Option<f64> = None;

    for zone in zones {
        if !zone.starts_with("thermal_zone") {
            continue;
        }
        let base = format!("/sys/class/thermal/{zone}");
        let zone_type = read_trimmed(format!("{base}/type")).unwrap_or_default();
        let millidegrees = read_u64(format!("{base}/temp"));

        let Some(m) = millidegrees else { continue };
        let celsius = m as f64 / 1000.0;

        let looks_like_cpu = zone_type.to_lowercase().contains("cpu")
            || zone_type.to_lowercase().contains("x86_pkg");

        if looks_like_cpu {
            return Some(celsius);
        }
        if fallback.is_none() {
            fallback = Some(celsius);
        }
    }
    fallback
}

pub fn collect() -> (CpuInfo, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let (model, vendor, flags, thread_count) = parse_cpuinfo();

    let online = read_trimmed("/sys/devices/system/cpu/online")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let offline = read_trimmed("/sys/devices/system/cpu/offline")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();

    // Physical core count: count unique "core id" values in cpuinfo if
    // present, otherwise fall back to logical thread count (no HT/SMT
    // info available).
    let physical_cores = fs::read_to_string("/proc/cpuinfo").ok().map(|content| {
        let mut core_ids = std::collections::HashSet::new();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim() == "core id" {
                    core_ids.insert(v.trim().to_string());
                }
            }
        }
        if core_ids.is_empty() {
            thread_count
        } else {
            core_ids.len() as u32
        }
    });

    let current_freq_mhz = read_u64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .map(|khz| khz / 1000);
    let max_freq_mhz = read_u64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .map(|khz| khz / 1000);
    let governor = read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    let temperature_celsius = read_cpu_temperature();

    let info = CpuInfo {
        model: model.clone(),
        vendor,
        architecture: std::env::consts::ARCH.to_string(),
        physical_cores,
        logical_threads: if thread_count > 0 {
            Some(thread_count)
        } else {
            None
        },
        online_cpus: online,
        offline_cpus: offline,
        flags,
        current_freq_mhz,
        max_freq_mhz,
        governor,
        temperature_celsius,
    };

    // --- Emit diagnostic results -------------------------------------
    let elapsed = timer.elapsed_ms();

    if thread_count > 0 {
        results.push(DiagnosticResult::new(
            "cpu_identify",
            "cpu",
            Status::Pass,
            Severity::Info,
            format!(
                "Identified {} ({} logical threads)",
                model.unwrap_or_else(|| "unknown model".to_string()),
                thread_count
            ),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "cpu_identify",
            "cpu",
            Status::Fail,
            Severity::Error,
            "Could not read /proc/cpuinfo".to_string(),
            elapsed,
        ));
    }

    match info.temperature_celsius {
        Some(t) if t >= 95.0 => results.push(DiagnosticResult::new(
            "cpu_thermal",
            "cpu",
            Status::Fail,
            Severity::Critical,
            format!("CPU temperature critical: {t:.1}C"),
            elapsed,
        )),
        Some(t) if t >= 85.0 => results.push(DiagnosticResult::new(
            "cpu_thermal",
            "cpu",
            Status::Warn,
            Severity::Warning,
            format!("CPU temperature elevated: {t:.1}C"),
            elapsed,
        )),
        Some(t) => results.push(DiagnosticResult::new(
            "cpu_thermal",
            "cpu",
            Status::Pass,
            Severity::Info,
            format!("CPU temperature normal: {t:.1}C"),
            elapsed,
        )),
        None => results.push(DiagnosticResult::new(
            "cpu_thermal",
            "cpu",
            Status::Skipped,
            Severity::Info,
            "No CPU thermal zone exposed by this system".to_string(),
            elapsed,
        )),
    }

    (info, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_range() {
        assert_eq!(parse_cpu_list("0-3"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn parses_mixed_ranges_and_singles() {
        assert_eq!(parse_cpu_list("0-1,3,5-6"), vec![0, 1, 3, 5, 6]);
    }

    #[test]
    fn parses_single_cpu() {
        assert_eq!(parse_cpu_list("0"), vec![0]);
    }

    #[test]
    fn handles_empty_string() {
        assert_eq!(parse_cpu_list(""), Vec::<u32>::new());
    }

    #[test]
    fn ignores_malformed_range_silently() {
        // Malformed input must never panic — worst case it contributes
        // nothing to the list, per the "never crash on unexpected
        // hardware/kernel data" rule (docs/architecture.md section 38).
        assert_eq!(parse_cpu_list("abc-def"), Vec::<u32>::new());
    }
}
