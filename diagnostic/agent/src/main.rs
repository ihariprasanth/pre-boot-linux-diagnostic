//! PLDDS diagnostic agent — Phase 9.
//!
//! Runs the CPU, memory, kernel, PCI, storage, GPU, USB, network, and
//! sensor collectors, prints a human-readable progress summary to the
//! console (serial/framebuffer in the real boot environment), then
//! assembles and prints the structured JSON diagnostic report (see
//! `report.rs` and `schemas/diagnostic-report.schema.json`), uploads
//! it, and finally decides + records what the next boot should be
//! (`bootdecision.rs`) for `/init` to act on.
//!
//! What Phase 9 deliberately does NOT do yet:
//!   - the agent itself never calls `grub-reboot` or `reboot` — that
//!     stays entirely in `/init` (POSIX shell, not this binary), by
//!     design (see `bootdecision.rs` module docs)
//!   - no formalized QEMU boot+diagnose+upload+ACK+reboot test harness
//!     (Phase 10)
//!   - no persistent retry counter or device key (Phase 13 — EFI
//!     System Partition storage)
//!
//! Every collector is independent: if one fails or a section of
//! hardware is unavailable, the others still run and report normally
//! (see docs/architecture.md "Error Handling" — one bad collector must
//! never abort the whole diagnostic run).

mod bootdecision;
mod collectors;
mod crypto;
mod localsave;
mod report;
mod result;
mod sysfs;
mod upload;

use report::DiagnosticReport;
use result::DiagnosticResult;
use std::fs;
use std::io::Write;

/// Where the report is best-effort written on disk in addition to
/// being printed to the console. `/run` is tmpfs on virtually every
/// real Linux system (including the initramfs), so this never touches
/// persistent storage. Phase 6 will read this same file to upload it
/// instead of regenerating it.
const REPORT_OUTPUT_PATH: &str = "/run/pldds-report.json";

fn print_banner() {
    println!();
    println!("============================================================");
    println!("  PLDDS Diagnostic Agent v{}", report::AGENT_VERSION);
    println!(
        "  Phase 9: full hardware sweep + boot handoff (schema v{})",
        report::SCHEMA_VERSION
    );
    println!("============================================================");
    println!();
}

fn print_result_line(r: &DiagnosticResult) {
    println!(
        "  [{:<7}] {:<10} {:<24} {}  ({} ms)",
        r.status.to_string(),
        r.component,
        r.test,
        r.message,
        r.duration_ms
    );
}

fn main() {
    // Must happen before any TLS client is built (see upload::client /
    // Cargo.toml comment) — reqwest's rustls-tls backend errors out at
    // Client::builder().build() ("builder error", no connection ever
    // attempted) unless a process-level crypto provider is installed
    // first. Safe to call once, ignore the result if something else
    // already installed one first.
    let _ = rustls::crypto::ring::default_provider().install_default();

    print_banner();

    println!("-- CPU --");
    let cpu = collectors::cpu::collect();
    for r in &cpu.1 {
        print_result_line(r);
    }
    if let Some(cores) = cpu.0.physical_cores {
        println!(
            "  detail: {} physical core(s), {} online CPU(s)",
            cores,
            cpu.0.online_cpus.len()
        );
    }
    println!();

    println!("-- Memory --");
    let memory = collectors::memory::collect();
    for r in &memory.1 {
        print_result_line(r);
    }
    if let Some(total) = memory.0.total_bytes {
        println!(
            "  detail: {:.2} GiB total, ECC: {}",
            total as f64 / (1024.0 * 1024.0 * 1024.0),
            match memory.0.ecc {
                Some(true) => "detected (EDAC)",
                Some(false) => "not detected",
                None => "undetermined",
            }
        );
    }
    println!();

    println!("-- Kernel --");
    let kernel = collectors::kernel::collect();
    for r in &kernel.1 {
        print_result_line(r);
    }
    if !kernel.0.log_entries.is_empty() {
        println!("  detail: sample classified log entries:");
        for entry in kernel.0.log_entries.iter().take(3) {
            println!("    [{}] {}", entry.severity, entry.line);
        }
    }
    println!();

    println!("-- PCI --");
    let pci = collectors::pci::collect();
    for r in &pci.1 {
        print_result_line(r);
    }
    println!("  detail: {} device(s) enumerated", pci.0.len());
    println!();

    println!("-- Storage --");
    let storage = collectors::storage::collect();
    for r in &storage.1 {
        print_result_line(r);
    }
    for dev in &storage.0 {
        let size_gb = dev
            .size_bytes
            .map(|b| format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0)))
            .unwrap_or_else(|| "unknown size".to_string());
        println!(
            "  detail: {} — {} — {}",
            dev.name,
            dev.model.clone().unwrap_or_else(|| "unknown model".to_string()),
            size_gb
        );
    }
    println!();

    println!("-- GPU --");
    let gpu = collectors::gpu::collect();
    for r in &gpu.1 {
        print_result_line(r);
    }
    println!("  detail: {} GPU(s) enumerated", gpu.0.len());
    println!();

    println!("-- USB --");
    let usb = collectors::usb::collect();
    for r in &usb.1 {
        print_result_line(r);
    }
    for dev in &usb.0 {
        println!(
            "  detail: {} — {} {} ({})",
            dev.bus_path,
            dev.manufacturer.clone().unwrap_or_else(|| "unknown vendor".to_string()),
            dev.product.clone().unwrap_or_else(|| "unknown product".to_string()),
            dev.speed_mbps.clone().map(|s| format!("{s} Mbps")).unwrap_or_else(|| "unknown speed".to_string())
        );
    }
    println!();

    println!("-- Network --");
    let network = collectors::network::collect();
    for r in &network.1 {
        print_result_line(r);
    }
    for iface in &network.0 {
        println!(
            "  detail: {} — {}{} — {}",
            iface.name,
            iface.oper_state.clone().unwrap_or_else(|| "unknown".to_string()),
            if iface.is_wireless { " (wireless)" } else { "" },
            iface.mac_address.clone().unwrap_or_else(|| "no MAC exposed".to_string())
        );
    }
    println!();

    println!("-- Sensors --");
    let sensors = collectors::sensors::collect();
    for r in &sensors.1 {
        print_result_line(r);
    }
    println!("  detail: {} sensor reading(s) across all hwmon chips", sensors.0.len());
    println!();

    // --- Device identity (Phase 7) -------------------------------------
    // Loads (or provisions on first boot) the Ed25519 device keypair and
    // derives the privacy-conscious hashed device_id from it — see
    // crypto.rs for the KNOWN LIMITATION on key persistence pre-Phase-9.
    let identity = crypto::load_or_provision();
    let device = report::DeviceIdentity {
        device_id: identity.device_id.clone(),
        hostname: sysfs::read_trimmed("/proc/sys/kernel/hostname"),
    };

    // --- Assemble the JSON report (Phase 4) ---------------------------
    let full_report: DiagnosticReport = report::build_report(
        device,
        report::read_boot_id(),
        report::generate_report_id(),
        report::now_rfc3339(),
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

    println!("============================================================");
    println!(
        "  Tests: {} total | {} passed | {} warned | {} failed | {} skipped",
        full_report.summary.total,
        full_report.summary.passed,
        full_report.summary.warned,
        full_report.summary.failed,
        full_report.summary.skipped
    );
    println!(
        "  Overall health score: {}/100 ({})",
        full_report.summary.score, full_report.summary.score_label
    );
    println!("============================================================");
    println!();

    let json_result = report::to_json_pretty(&full_report);
    match &json_result {
        Ok(json) => {
            println!("-- JSON report ({}) --", REPORT_OUTPUT_PATH);
            println!("{json}");

            match fs::File::create(REPORT_OUTPUT_PATH).and_then(|mut f| f.write_all(json.as_bytes())) {
                Ok(()) => println!("\n[agent] Report written to {REPORT_OUTPUT_PATH}"),
                Err(e) => println!(
                    "\n[agent] Could not write report to {REPORT_OUTPUT_PATH} ({e}) — \
                     printed above instead. Non-fatal: a pre-boot diagnostic must never \
                     abort over a filesystem write failure."
                ),
            }
        }
        Err(e) => {
            // Should not happen for these plain-data types, but a
            // pre-boot process must degrade instead of panicking.
            eprintln!("[agent] Failed to serialize report: {e}");
        }
    }

    // --- Local-save or Upload (local-log mode vs. network mode) --------
    // PLDDS_LOCAL_LOG_ONLY=1 (set in diagnostic/initramfs/init) skips the
    // network entirely: the report is written straight onto the Windows
    // C: partition (see localsave.rs) instead of being POSTed to a
    // server. This is the default — nothing here calls out to the
    // network unless that variable is explicitly unset. A separate
    // Windows-side tool (built later, not part of this boot) is meant to
    // pick these files up from C:\PLDDS\Logs\ and transmit them on its
    // own schedule.
    let local_log_only = std::env::var("PLDDS_LOCAL_LOG_ONLY").map(|v| v != "0").unwrap_or(true);

    let upload_ok = if local_log_only {
        println!("-- Local save (PLDDS_LOCAL_LOG_ONLY) --");
        match &json_result {
            Ok(json) => {
                let filename = format!("pldds-report-{}.json", full_report.report_id);
                match localsave::save_report_locally(&filename, json) {
                    Ok(windows_path) => {
                        println!("[agent] report saved to Windows: {}", windows_path.display());
                        true
                    }
                    Err(e) => {
                        println!(
                            "[agent] local save failed: {e} — continuing boot anyway \
                             (GRACEFUL mode, see docs/architecture.md \"Failure safety\")"
                        );
                        false
                    }
                }
            }
            Err(_) => false, // already logged above when serialization failed
        }
    } else {
        println!("-- Upload --");
        match upload::submit_report(&identity, &full_report) {
            Ok(()) => {
                println!("[agent] upload OK, server ACKed report {}", full_report.report_id);
                true
            }
            Err(e) => {
                println!(
                    "[agent] upload failed: {e} — continuing boot anyway (GRACEFUL mode, \
                     see docs/architecture.md \"Failure safety\")"
                );
                false
            }
        }
    };
    println!();
    println!(
        "[agent] device_id: {} (Ed25519-signed requests, replay nonce per boot_id)",
        identity.device_id
    );

    // --- Boot decision + handoff (Phase 9) -----------------------------
    // The agent decides WHAT the next boot should be; it never acts on
    // it. /init reads this file and is the only thing that ever calls
    // `grub-reboot`. See bootdecision.rs for the full safety reasoning.
    let decision = bootdecision::decide(upload_ok, full_report.summary.failed > 0);
    println!("-- Boot decision --");
    println!("  action:      {:?}", decision.action);
    println!("  reason:      {}", decision.reason);
    println!("  retry_count: {}", decision.retry_count);
    match bootdecision::write_decision(&decision) {
        Ok(()) => println!("[agent] boot decision written to /run/pldds-boot-decision"),
        Err(e) => eprintln!(
            "[agent] could not write boot decision ({e}) — /init falls back to its \
             own fail-safe (boot Windows) when this file is missing"
        ),
    }

    // Exit code mirrors the boot decision so /init has a cheap
    // double-check even if it somehow can't read the decision file:
    // 0 = normal (boot Windows), 1 = diagnostic found real problems
    // (still boots Windows — see bootdecision.rs — but flagged for
    // anyone tailing the console log), 2 = requesting a retry.
    if decision.action == bootdecision::BootAction::RetryDiagnostic {
        std::process::exit(2);
    }
    if full_report.summary.failed > 0 {
        std::process::exit(1);
    }
}
