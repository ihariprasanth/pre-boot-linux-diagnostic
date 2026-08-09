//! USB device diagnostics.
//!
//! Source: /sys/bus/usb/devices/ (per docs/architecture.md "USB Diagnostics").
//! Purely read-only enumeration. Root hubs (`usbN`) and interface nodes
//! (`N-N:1.0`) are filtered out — only actual attached devices are kept,
//! so the count reported matches what a user would see in `lsusb`.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_trimmed};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/usb_device`.
#[derive(Debug, Clone, Serialize)]
pub struct UsbDevice {
    pub bus_path: String,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// Negotiated link speed in Mbps as reported by the kernel, e.g.
    /// "480" (USB 2.0 High Speed) or "5000" (USB 3.0 SuperSpeed).
    pub speed_mbps: Option<String>,
}

/// A real attached USB device's directory name is bus-relative and
/// numeric, e.g. `1-2` or `2-1.4` — root hubs are `usbN` and interface
/// nodes contain a `:` (e.g. `1-2:1.0`), both excluded.
fn is_device_node(name: &str) -> bool {
    !name.starts_with("usb") && !name.contains(':')
}

pub fn collect() -> (Vec<UsbDevice>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let entries = list_dir_names("/sys/bus/usb/devices");
    let device_names: Vec<String> = entries.into_iter().filter(|n| is_device_node(n)).collect();

    let mut devices = Vec::new();
    for name in &device_names {
        let base = format!("/sys/bus/usb/devices/{name}");

        // idVendor is present on real devices; absent (or unreadable)
        // means this node isn't a full device — skip it rather than
        // reporting a mostly-empty entry.
        let vendor_id = read_trimmed(format!("{base}/idVendor"));
        if vendor_id.is_none() {
            continue;
        }

        devices.push(UsbDevice {
            bus_path: name.clone(),
            vendor_id,
            product_id: read_trimmed(format!("{base}/idProduct")),
            manufacturer: read_trimmed(format!("{base}/manufacturer")),
            product: read_trimmed(format!("{base}/product")),
            speed_mbps: read_trimmed(format!("{base}/speed")),
        });
    }

    let elapsed = timer.elapsed_ms();

    if device_names.is_empty() {
        results.push(DiagnosticResult::new(
            "usb_enumerate",
            "usb",
            Status::Skipped,
            Severity::Info,
            "No USB bus exposed (/sys/bus/usb/devices unavailable — expected in some VMs/containers)".to_string(),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "usb_enumerate",
            "usb",
            Status::Pass,
            Severity::Info,
            format!("Enumerated {} USB device(s)", devices.len()),
            elapsed,
        ));
    }

    (devices, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_device_path_accepted() {
        assert!(is_device_node("1-2"));
        assert!(is_device_node("2-1.4"));
    }

    #[test]
    fn root_hub_rejected() {
        assert!(!is_device_node("usb1"));
        assert!(!is_device_node("usb2"));
    }

    #[test]
    fn interface_node_rejected() {
        assert!(!is_device_node("1-2:1.0"));
    }
}
