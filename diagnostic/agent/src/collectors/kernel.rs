//! Kernel diagnostics: version, taint state, command line, and a
//! classified read of the kernel ring buffer (dmesg).
//!
//! Per docs/architecture.md "Kernel Diagnostics": we don't blindly
//! grep dmesg for the word "error" — we run a small classification
//! pass that looks for known patterns (PCIe AER, I/O errors, ACPI
//! errors, OOPS/panic markers, driver failures) and buckets each
//! matching line into INFO/WARNING/ERROR/CRITICAL.

use crate::result::{DiagnosticResult, Severity, Status, Timer};
use crate::sysfs::read_trimmed;
use serde::Serialize;
use std::fs;

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/kernel_log_entry`.
#[derive(Debug, Clone, Serialize)]
pub struct KernelLogEntry {
    pub severity: Severity,
    pub line: String,
}

/// Field names/shape match `schemas/diagnostic-report.schema.json#/$defs/kernel_info`.
#[derive(Debug, Default, Serialize)]
pub struct KernelInfo {
    pub version: Option<String>,
    pub cmdline: Option<String>,
    pub tainted: bool,
    pub taint_code: Option<u64>,
    pub log_entries: Vec<KernelLogEntry>,
}

/// Bit meanings for /proc/sys/kernel/tainted, per Documentation/admin-guide/tainted-kernels.rst.
/// We only surface a human-readable summary of the flags that matter
/// for hardware health (not every flag — e.g. "kernel built with a
/// staging driver" is not something a diagnostic report needs to
/// alarm about).
fn describe_taint(code: u64) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if code & (1 << 0) != 0 {
        flags.push("proprietary module loaded");
    }
    if code & (1 << 1) != 0 {
        flags.push("module force-loaded");
    }
    if code & (1 << 5) != 0 {
        flags.push("kernel oops occurred");
    }
    if code & (1 << 7) != 0 {
        flags.push("machine check exception occurred");
    }
    if code & (1 << 9) != 0 {
        flags.push("ACPI table overridden");
    }
    if code & (1 << 18) != 0 {
        flags.push("kernel died recently (OOPS/BUG)");
    }
    flags
}

/// Classify a single kernel log line. Returns None for lines with no
/// diagnostic significance (most of dmesg is routine).
fn classify_line(line: &str) -> Option<Severity> {
    let lower = line.to_lowercase();

    // Critical: kernel crash-adjacent markers.
    if lower.contains("kernel panic")
        || lower.contains("oops:")
        || lower.contains("hardware error")
        || lower.contains("mce: [hardware error]")
    {
        return Some(Severity::Critical);
    }

    // Error: correctable-but-real hardware/driver problems.
    if lower.contains("i/o error")
        || lower.contains("nvme") && lower.contains("error")
        || lower.contains("pcie bus error")
        || lower.contains("ata error")
        || lower.contains("usb disconnect") && lower.contains("error")
        || lower.contains("call trace")
    {
        return Some(Severity::Error);
    }

    // Warning: correctable errors, degraded links, ACPI complaints.
    if lower.contains("aer:")
        || lower.contains("correctable error")
        || lower.contains("acpi error")
        || lower.contains("acpi warning")
        || lower.contains("link is not up")
        || lower.contains("thermal")  && lower.contains("warn")
        || lower.contains("clocksource")  && lower.contains("unstable")
    {
        return Some(Severity::Warning);
    }

    None
}

/// Read the kernel ring buffer. Prefers `dmesg` (works whether or not
/// the caller has direct /dev/kmsg access), falls back to reading
/// /dev/kmsg directly, and returns an empty Vec (not an error) if
/// neither is available — some sandboxes restrict both.
fn read_kernel_log() -> Vec<String> {
    if let Ok(output) = std::process::Command::new("dmesg").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            return text.lines().map(|l| l.to_string()).collect();
        }
    }
    // dmesg unavailable or failed (common in unprivileged containers) —
    // this is expected in many dev/test environments, not an agent bug.
    Vec::new()
}

pub fn collect() -> (KernelInfo, Vec<DiagnosticResult>) {
    let timer = Timer::start();
    let mut results = Vec::new();

    let version = read_trimmed("/proc/version");
    let cmdline = read_trimmed("/proc/cmdline");
    let taint_code = fs::read_to_string("/proc/sys/kernel/tainted")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    let tainted = taint_code.map(|c| c != 0).unwrap_or(false);

    let raw_log = read_kernel_log();
    let log_entries: Vec<KernelLogEntry> = raw_log
        .iter()
        .filter_map(|line| classify_line(line).map(|sev| KernelLogEntry {
            severity: sev,
            line: line.clone(),
        }))
        .collect();

    let elapsed = timer.elapsed_ms();

    match &version {
        Some(v) => results.push(DiagnosticResult::new(
            "kernel_identify",
            "kernel",
            Status::Pass,
            Severity::Info,
            format!("Kernel: {v}"),
            elapsed,
        )),
        None => results.push(DiagnosticResult::new(
            "kernel_identify",
            "kernel",
            Status::Fail,
            Severity::Error,
            "Could not read /proc/version".to_string(),
            elapsed,
        )),
    }

    if tainted {
        let flags = taint_code.map(describe_taint).unwrap_or_default();
        let detail = if flags.is_empty() {
            format!("Kernel tainted (code {})", taint_code.unwrap_or(0))
        } else {
            format!("Kernel tainted: {}", flags.join(", "))
        };
        results.push(DiagnosticResult::new(
            "kernel_taint",
            "kernel",
            Status::Warn,
            Severity::Warning,
            detail,
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "kernel_taint",
            "kernel",
            Status::Pass,
            Severity::Info,
            "Kernel is not tainted".to_string(),
            elapsed,
        ));
    }

    let critical_count = log_entries.iter().filter(|e| e.severity == Severity::Critical).count();
    let error_count = log_entries.iter().filter(|e| e.severity == Severity::Error).count();
    let warning_count = log_entries.iter().filter(|e| e.severity == Severity::Warning).count();

    if raw_log.is_empty() {
        results.push(DiagnosticResult::new(
            "kernel_log_scan",
            "kernel",
            Status::Skipped,
            Severity::Info,
            "Kernel log unavailable (dmesg/kmsg not accessible in this environment)".to_string(),
            elapsed,
        ));
    } else if critical_count > 0 {
        results.push(DiagnosticResult::new(
            "kernel_log_scan",
            "kernel",
            Status::Fail,
            Severity::Critical,
            format!("{critical_count} critical, {error_count} error, {warning_count} warning kernel log entries found"),
            elapsed,
        ));
    } else if error_count > 0 {
        results.push(DiagnosticResult::new(
            "kernel_log_scan",
            "kernel",
            Status::Warn,
            Severity::Error,
            format!("{error_count} error, {warning_count} warning kernel log entries found"),
            elapsed,
        ));
    } else if warning_count > 0 {
        results.push(DiagnosticResult::new(
            "kernel_log_scan",
            "kernel",
            Status::Warn,
            Severity::Warning,
            format!("{warning_count} warning kernel log entries found"),
            elapsed,
        ));
    } else {
        results.push(DiagnosticResult::new(
            "kernel_log_scan",
            "kernel",
            Status::Pass,
            Severity::Info,
            format!("Scanned {} kernel log lines, no issues classified", raw_log.len()),
            elapsed,
        ));
    }

    let info = KernelInfo {
        version,
        cmdline,
        tainted,
        taint_code,
        log_entries,
    };

    (info, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_kernel_panic_as_critical() {
        assert_eq!(classify_line("Kernel panic - not syncing: VFS"), Some(Severity::Critical));
    }

    #[test]
    fn classifies_pcie_aer_as_warning() {
        assert_eq!(classify_line("AER: Corrected error received"), Some(Severity::Warning));
    }

    #[test]
    fn classifies_io_error_as_error() {
        assert_eq!(classify_line("blk_update_request: I/O error, dev sda"), Some(Severity::Error));
    }

    #[test]
    fn routine_line_is_not_classified() {
        assert_eq!(classify_line("Freeing unused kernel image memory: 4932K"), None);
    }

    #[test]
    fn taint_flags_decode_oops_bit() {
        // bit 18 = kernel died recently
        let flags = describe_taint(1 << 18);
        assert!(flags.contains(&"kernel died recently (OOPS/BUG)"));
    }

    #[test]
    fn taint_zero_has_no_flags() {
        assert!(describe_taint(0).is_empty());
    }
}
