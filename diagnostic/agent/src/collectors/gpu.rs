//! GPU device diagnostics.
//!
//! Source: /sys/class/drm/ (per docs/architecture.md "GPU Diagnostics").
//! Only `cardN` entries are treated as GPUs — `cardN-<connector>` nodes
//! (e.g. `card0-HDMI-A-1`) are display outputs, not devices, and are
//! skipped. Purely read-only enumeration: this module never touches
//! `/dev/dri/*` and never issues any mode-set, reset, or vendor ioctl.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_link_basename, read_trimmed, read_u64};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/gpu_device`.
#[derive(Debug, Clone, Serialize)]
pub struct GpuDevice {
    pub name: String,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub driver: Option<String>,
    /// Best-effort junction/die temperature in Celsius, read from the
    /// first hwmon sensor hung off this DRM card's device, when the
    /// driver exposes one (amdgpu/i915/nouveau do; many don't).
    pub temperature_celsius: Option<f64>,
}

/// True for `card0`, `card1`, ... — false for connector nodes like
/// `card0-HDMI-A-1` and for `renderD128`-style render nodes, which are
/// the same underlying device and would double-count it.
fn is_card_node(name: &str) -> bool {
    name.starts_with("card") && !name.contains('-')
}

/// A card's hwmon temperature, if its device exposes one under
/// `device/hwmon/hwmonN/temp1_input` (millidegrees C, per the kernel
/// hwmon ABI — same convention as `cpu.rs`'s thermal-zone reader).
fn read_gpu_temperature(card_base: &str) -> Option<f64> {
    let hwmon_dir = format!("{card_base}/device/hwmon");
    let hwmons = list_dir_names(&hwmon_dir);
    for hwmon in hwmons {
        if let Some(m) = read_u64(format!("{hwmon_dir}/{hwmon}/temp1_input")) {
            return Some(m as f64 / 1000.0);
        }
    }
    None
}

pub fn collect() -> (Vec<GpuDevice>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let entries = list_dir_names("/sys/class/drm");
    let card_names: Vec<String> = entries.into_iter().filter(|n| is_card_node(n)).collect();

    let mut devices = Vec::new();
    for name in &card_names {
        let base = format!("/sys/class/drm/{name}");
        let vendor_id = read_trimmed(format!("{base}/device/vendor"));
        let device_id = read_trimmed(format!("{base}/device/device"));
        let driver = read_link_basename(format!("{base}/device/driver"));
        let temperature_celsius = read_gpu_temperature(&base);

        devices.push(GpuDevice {
            name: name.clone(),
            vendor_id,
            device_id,
            driver,
            temperature_celsius,
        });
    }

    let elapsed = timer.elapsed_ms();

    if devices.is_empty() {
        // Headless/server boxes and most VMs legitimately have no DRM
        // card — this is informational, never a failure.
        results.push(DiagnosticResult::new(
            "gpu_enumerate",
            "gpu",
            Status::Skipped,
            Severity::Info,
            "No GPU exposed under /sys/class/drm".to_string(),
            elapsed,
        ));
    } else {
        let no_driver = devices.iter().filter(|d| d.driver.is_none()).count();
        if no_driver > 0 {
            results.push(DiagnosticResult::new(
                "gpu_enumerate",
                "gpu",
                Status::Warn,
                Severity::Warning,
                format!(
                    "Enumerated {} GPU(s); {} have no bound driver",
                    devices.len(),
                    no_driver
                ),
                elapsed,
            ));
        } else {
            results.push(DiagnosticResult::new(
                "gpu_enumerate",
                "gpu",
                Status::Pass,
                Severity::Info,
                format!("Enumerated {} GPU(s), all have a bound driver", devices.len()),
                elapsed,
            ));
        }
    }

    for dev in &devices {
        match dev.temperature_celsius {
            Some(t) if t >= 100.0 => results.push(DiagnosticResult::new(
                "gpu_thermal",
                "gpu",
                Status::Fail,
                Severity::Critical,
                format!("{}: GPU temperature critical: {t:.1}C", dev.name),
                elapsed,
            )),
            Some(t) if t >= 90.0 => results.push(DiagnosticResult::new(
                "gpu_thermal",
                "gpu",
                Status::Warn,
                Severity::Warning,
                format!("{}: GPU temperature elevated: {t:.1}C", dev.name),
                elapsed,
            )),
            Some(t) => results.push(DiagnosticResult::new(
                "gpu_thermal",
                "gpu",
                Status::Pass,
                Severity::Info,
                format!("{}: GPU temperature normal: {t:.1}C", dev.name),
                elapsed,
            )),
            None => results.push(DiagnosticResult::new(
                "gpu_thermal",
                "gpu",
                Status::Skipped,
                Severity::Info,
                format!("{}: no hwmon temperature sensor exposed", dev.name),
                elapsed,
            )),
        }
    }

    (devices, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_node_accepted() {
        assert!(is_card_node("card0"));
        assert!(is_card_node("card1"));
    }

    #[test]
    fn connector_node_rejected() {
        assert!(!is_card_node("card0-HDMI-A-1"));
        assert!(!is_card_node("card0-eDP-1"));
    }
}
