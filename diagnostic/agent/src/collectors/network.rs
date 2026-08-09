//! Network interface diagnostics.
//!
//! Source: /sys/class/net/ (per docs/architecture.md "Network Diagnostics").
//! Purely read-only enumeration — this module never brings an interface
//! up/down and never opens a socket. The loopback interface is skipped
//! since it isn't hardware and its "link" state is meaningless.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::{list_dir_names, read_trimmed};
use serde::Serialize;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/network_interface`.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkInterface {
    pub name: String,
    /// "up" / "down" / "unknown" — from /sys/class/net/<if>/operstate.
    pub oper_state: Option<String>,
    /// Physical link detected, independent of the interface being
    /// administratively up — None on interfaces that don't expose it
    /// (common on virtual interfaces).
    pub carrier: Option<bool>,
    pub mac_address: Option<String>,
    pub speed_mbps: Option<i64>,
    pub is_wireless: bool,
}

pub fn collect() -> (Vec<NetworkInterface>, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let all_names = list_dir_names("/sys/class/net");
    let relevant: Vec<String> = all_names.into_iter().filter(|n| n != "lo").collect();

    let mut interfaces = Vec::new();
    for name in &relevant {
        let base = format!("/sys/class/net/{name}");

        let oper_state = read_trimmed(format!("{base}/operstate"));
        let carrier = read_trimmed(format!("{base}/carrier")).map(|s| s == "1");
        let mac_address = read_trimmed(format!("{base}/address"));
        // /sys/class/net/<if>/speed is only readable (and only makes
        // sense) while the link is up; reading it on a down interface
        // can return -1 or fail entirely — both are just "unknown".
        let speed_mbps = read_trimmed(format!("{base}/speed")).and_then(|s| s.parse::<i64>().ok()).filter(|&v| v > 0);
        // A `wireless` subdirectory (legacy WEXT) or a `phy80211` link
        // (modern cfg80211/nl80211) both reliably indicate a Wi-Fi
        // interface; checking both covers older and newer drivers.
        let is_wireless = std::path::Path::new(&format!("{base}/wireless")).exists()
            || std::path::Path::new(&format!("{base}/phy80211")).exists();

        interfaces.push(NetworkInterface {
            name: name.clone(),
            oper_state,
            carrier,
            mac_address,
            speed_mbps,
            is_wireless,
        });
    }

    let elapsed = timer.elapsed_ms();

    if interfaces.is_empty() {
        results.push(DiagnosticResult::new(
            "network_enumerate",
            "network",
            Status::Skipped,
            Severity::Info,
            "No non-loopback network interfaces found under /sys/class/net".to_string(),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "network_enumerate",
            "network",
            Status::Pass,
            Severity::Info,
            format!("Enumerated {} network interface(s)", interfaces.len()),
            elapsed,
        ));
    }

    for iface in &interfaces {
        match iface.oper_state.as_deref() {
            Some("up") => results.push(DiagnosticResult::new(
                "network_link",
                "network",
                Status::Pass,
                Severity::Info,
                format!("{}: link up{}", iface.name, iface.speed_mbps.map(|s| format!(" ({s} Mbps)")).unwrap_or_default()),
                elapsed,
            )),
            Some("down") => results.push(DiagnosticResult::new(
                "network_link",
                "network",
                Status::Warn,
                Severity::Warning,
                format!("{}: link down", iface.name),
                elapsed,
            )),
            Some(other) => results.push(DiagnosticResult::new(
                "network_link",
                "network",
                Status::Skipped,
                Severity::Info,
                format!("{}: operstate '{}' (not up/down)", iface.name, other),
                elapsed,
            )),
            None => results.push(DiagnosticResult::new(
                "network_link",
                "network",
                Status::Skipped,
                Severity::Info,
                format!("{}: operstate not exposed", iface.name),
                elapsed,
            )),
        }
    }

    (interfaces, results)
}
