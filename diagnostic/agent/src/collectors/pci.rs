//! PCIe device enumeration.
//!
//! Source: /sys/bus/pci/devices/ (per docs/architecture.md "PCIe Diagnostics").
//! Purely read-only enumeration — this module never writes to any PCI
//! config space or device file.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_link_basename, read_trimmed};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/pci_device`.
#[derive(Debug, Clone, Serialize)]
pub struct PciDevice {
    pub address: String,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub class: Option<String>,
    pub driver: Option<String>,
    /// Current negotiated link speed, e.g. "8.0 GT/s PCIe" — only
    /// present on devices where the kernel exposes it.
    pub link_speed: Option<String>,
}

pub fn collect() -> (Vec<PciDevice>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let addresses = list_dir_names("/sys/bus/pci/devices");
    let mut devices = Vec::new();

    for addr in &addresses {
        let base = format!("/sys/bus/pci/devices/{addr}");

        let vendor_id = read_trimmed(format!("{base}/vendor"));
        let device_id = read_trimmed(format!("{base}/device"));
        let class = read_trimmed(format!("{base}/class"));
        let driver = read_link_basename(format!("{base}/driver"));
        let link_speed = read_trimmed(format!("{base}/current_link_speed"));

        devices.push(PciDevice {
            address: addr.clone(),
            vendor_id,
            device_id,
            class,
            driver,
            link_speed,
        });
    }

    let elapsed = timer.elapsed_ms();

    if addresses.is_empty() {
        results.push(DiagnosticResult::new(
            "pci_enumerate",
            "pci",
            Status::Skipped,
            Severity::Info,
            "No PCI bus exposed (/sys/bus/pci/devices unavailable — expected in some VMs/containers)".to_string(),
            elapsed,
        ));
    } else {
        let no_driver = devices.iter().filter(|d| d.driver.is_none()).count();
        if no_driver > 0 {
            results.push(DiagnosticResult::new(
                "pci_enumerate",
                "pci",
                Status::Warn,
                Severity::Warning,
                format!(
                    "Enumerated {} PCI device(s); {} have no bound driver",
                    devices.len(),
                    no_driver
                ),
                elapsed,
            ));
        } else {
            results.push(DiagnosticResult::new(
                "pci_enumerate",
                "pci",
                Status::Pass,
                Severity::Info,
                format!("Enumerated {} PCI device(s), all have a bound driver", devices.len()),
                elapsed,
            ));
        }
    }

    (devices, results)
}
