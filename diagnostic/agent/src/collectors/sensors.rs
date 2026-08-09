//! Hardware sensor diagnostics (temperature/fan/voltage).
//!
//! Source: /sys/class/hwmon/ (per docs/architecture.md "Sensor Diagnostics"),
//! the standard kernel hwmon ABI used by lm-sensors and every in-tree
//! hwmon driver (coretemp, k10temp, nct6775, ...). Purely read-only —
//! this module never writes a hwmon `pwm*` fan-control file. Deliberately
//! separate from `cpu.rs`'s single CPU-thermal-zone reading and
//! `gpu.rs`'s single GPU-hwmon reading: this collector reports the full
//! board-level sensor set (case fans, VRM temps, PSU, etc.) rather than
//! one representative value per component.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_trimmed, read_u64};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/sensor_reading`.
#[derive(Debug, Clone, Serialize)]
pub struct SensorReading {
    /// hwmon chip name, e.g. "coretemp", "nct6775" — from `hwmonN/name`.
    pub chip: String,
    /// Sensor label if the driver provides one (e.g. "Package id 0"),
    /// otherwise the raw sysfs input key (e.g. "temp1").
    pub label: String,
    pub kind: SensorKind,
    pub value: f64,
    pub unit: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SensorKind {
    Temperature,
    Fan,
    Voltage,
}

/// Reads every `{prefix}N_input` file under a hwmon directory (e.g. all
/// `temp*_input`), pairing each with its `{prefix}N_label` if present.
/// `scale` converts the raw integer to the unit callers want (hwmon
/// reports millidegrees C for temps and millivolts for voltage; fan
/// RPM is already a plain integer, so scale is 1.0 there).
fn read_channel(hwmon_base: &str, prefix: &str, scale: f64) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let entries = list_dir_names(hwmon_base);
    let mut index = 1;
    loop {
        let input_path = format!("{hwmon_base}/{prefix}{index}_input");
        // list_dir_names already gave us the directory listing; avoid a
        // second syscall per candidate by checking membership instead
        // of re-statting each file.
        let file_name = format!("{prefix}{index}_input");
        if !entries.contains(&file_name) {
            break;
        }
        if let Some(raw) = read_u64(&input_path) {
            let label = read_trimmed(format!("{hwmon_base}/{prefix}{index}_label"))
                .unwrap_or_else(|| format!("{prefix}{index}"));
            out.push((label, raw as f64 * scale));
        }
        index += 1;
    }
    out
}

pub fn collect() -> (Vec<SensorReading>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let hwmons = list_dir_names("/sys/class/hwmon");
    let mut readings = Vec::new();

    for hwmon in &hwmons {
        let base = format!("/sys/class/hwmon/{hwmon}");
        let chip = read_trimmed(format!("{base}/name")).unwrap_or_else(|| hwmon.clone());

        for (label, value) in read_channel(&base, "temp", 0.001) {
            readings.push(SensorReading { chip: chip.clone(), label, kind: SensorKind::Temperature, value, unit: "C" });
        }
        for (label, value) in read_channel(&base, "fan", 1.0) {
            readings.push(SensorReading { chip: chip.clone(), label, kind: SensorKind::Fan, value, unit: "RPM" });
        }
        for (label, value) in read_channel(&base, "in", 0.001) {
            readings.push(SensorReading { chip: chip.clone(), label, kind: SensorKind::Voltage, value, unit: "V" });
        }
    }

    let elapsed = timer.elapsed_ms();

    if hwmons.is_empty() {
        results.push(DiagnosticResult::new(
            "sensors_enumerate",
            "sensors",
            Status::Skipped,
            Severity::Info,
            "No hwmon sensors exposed under /sys/class/hwmon (common in VMs)".to_string(),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "sensors_enumerate",
            "sensors",
            Status::Pass,
            Severity::Info,
            format!("Enumerated {} sensor reading(s) across {} hwmon chip(s)", readings.len(), hwmons.len()),
            elapsed,
        ));
    }

    // Flag any temperature reading that looks dangerously high. This is
    // intentionally a coarse, generic threshold (board/VRM sensors, not
    // just CPU/GPU die) — per-component thresholds already live in
    // cpu.rs/gpu.rs; this is a safety net for everything else.
    for reading in readings.iter().filter(|r| r.kind == SensorKind::Temperature) {
        if reading.value >= 100.0 {
            results.push(DiagnosticResult::new(
                "sensors_thermal",
                "sensors",
                Status::Fail,
                Severity::Critical,
                format!("{} ({}): {:.1}C — critical", reading.chip, reading.label, reading.value),
                elapsed,
            ));
        } else if reading.value >= 85.0 {
            results.push(DiagnosticResult::new(
                "sensors_thermal",
                "sensors",
                Status::Warn,
                Severity::Warning,
                format!("{} ({}): {:.1}C — elevated", reading.chip, reading.label, reading.value),
                elapsed,
            ));
        }
    }

    // Flag a fan reporting 0 RPM as a warning — could be a dead fan or
    // a legitimate zero-RPM idle mode, so Warning rather than Critical.
    for reading in readings.iter().filter(|r| r.kind == SensorKind::Fan) {
        if reading.value == 0.0 {
            results.push(DiagnosticResult::new(
                "sensors_fan",
                "sensors",
                Status::Warn,
                Severity::Warning,
                format!("{} ({}): 0 RPM — stopped or zero-RPM idle mode", reading.chip, reading.label),
                elapsed,
            ));
        }
    }

    (readings, results)
}
