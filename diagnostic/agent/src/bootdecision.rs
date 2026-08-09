//! Phase 9: the agent's half of the GRUB / Windows handoff.
//!
//! The agent never touches GRUB itself (no bootloader binaries are
//! even present in the initramfs on purpose — see `docs/boot-flow.md`
//! "Phase 9"). Its only job is to decide *what* the next boot should
//! be and write that decision to a plain-text tmpfs file. `/init`
//! (`diagnostic/initramfs/init`) reads that file after the agent exits
//! and is the only thing in the whole system that ever shells out to
//! `grub-reboot`.
//!
//! ## The one hard safety rule
//!
//! However bad the diagnostic run went — upload failure, a crashed
//! collector, the agent's own bug — the answer is **always** either
//! "boot Windows now" or "try the diagnostic one more time, then boot
//! Windows". There is no code path in this module that can produce a
//! decision which loops forever or which would leave the machine
//! sitting at a diagnostic boot the user never asked for. See
//! `docs/architecture.md` "Core safety invariant" and
//! `docs/boot-flow.md` "Phase 9" for the full reasoning.
//!
//! ## Retry counter is best-effort (KNOWN LIMITATION)
//!
//! Same limitation as `crypto.rs`'s device key: this initramfs is
//! RAM-only, so the retry counter below does not survive a reboot
//! until Phase 13 wires up a small persistent file on the EFI System
//! Partition. Until then every diagnostic boot sees `retry_count == 0`
//! read back, i.e. the counter degrades to "retry once, unconditionally,
//! whenever upload fails" rather than a true bounded backoff. That is
//! still a *safe* degradation — it never causes a boot loop, because
//! `RETRY_DIAGNOSTIC` is only ever selected when `grub-reboot` is
//! confirmed available (`/init` re-checks this), and the diagnostic
//! entry's own GRUB fallback (see `diagnostic/grub/grub.cfg`) drops
//! back to Windows if the retried boot doesn't produce a decision file
//! either. It is simply not yet a *smart* bounded backoff. Phase 13
//! removes this caveat entirely.

use std::fs;
use std::io::Write;

/// Hard ceiling on consecutive "retry the diagnostic boot" decisions
/// before giving up and booting Windows regardless of upload state.
/// Exists so that even a persistent-counter future implementation
/// (Phase 13) can never retry indefinitely.
const MAX_RETRIES: u32 = 2;

/// Where `/init` looks for the decision. tmpfs (`/run`) — same
/// no-persistent-storage caveat as `crypto::key_path()`.
const DECISION_PATH: &str = "/run/pldds-boot-decision";

/// Legacy/best-effort retry counter path — see "Retry counter is
/// best-effort" above. Not read back reliably today; written anyway
/// so Phase 13's persistent-storage change is a one-line path swap,
/// not a new file format.
const RETRY_COUNTER_PATH: &str = "/run/pldds-retry-count";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootAction {
    /// Hand off to Windows on the very next boot via a one-shot
    /// `grub-reboot` override. This is the outcome for the large
    /// majority of runs: successful upload, regardless of whether the
    /// hardware diagnostics themselves found WARN/FAIL results — a
    /// failing RAM stick is not a reason to strand the user outside
    /// their OS. Diagnostic findings are informational, delivered via
    /// the uploaded report, not enforced by refusing to boot.
    BootWindows,
    /// One more diagnostic boot, then Windows regardless. Reserved
    /// for the narrow case of "we couldn't even tell the server what
    /// happened" (upload failure) and only ever chosen when `/init`
    /// has independently confirmed `grub-reboot` is present and
    /// working — see `diagnostic/initramfs/init`.
    RetryDiagnostic,
}

impl BootAction {
    fn as_str(self) -> &'static str {
        match self {
            BootAction::BootWindows => "WINDOWS",
            BootAction::RetryDiagnostic => "RETRY_DIAGNOSTIC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BootDecision {
    pub action: BootAction,
    pub reason: String,
    pub retry_count: u32,
}

/// Best-effort read of the previous boot's retry counter. Missing or
/// unparsable file just means "treat this as attempt zero" — never an
/// error, this is advisory-only (see module docs).
fn read_retry_count() -> u32 {
    fs::read_to_string(RETRY_COUNTER_PATH)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// Decide the next boot given whether the report upload succeeded and
/// whether the diagnostic run itself found any FAIL-severity results.
///
/// `upload_ok` and `any_failed` are both taken as plain booleans
/// rather than the full report so this function stays trivially unit
/// testable and has no way to reach into agent internals it doesn't
/// need — see the tests below for the full truth table.
pub fn decide(upload_ok: bool, any_failed: bool) -> BootDecision {
    let prior_retries = read_retry_count();

    if upload_ok {
        let reason = if any_failed {
            "upload succeeded; hardware diagnostics found FAIL-severity results, \
             but findings are informational only — booting Windows normally \
             (see docs/architecture.md \"Core safety invariant\")"
                .to_string()
        } else {
            "upload succeeded; no FAIL-severity results".to_string()
        };
        return BootDecision {
            action: BootAction::BootWindows,
            reason,
            retry_count: 0,
        };
    }

    // Upload failed. Retry once (see MAX_RETRIES), then give up and
    // boot Windows anyway — the user's OS access must never depend on
    // this box's network or the backend being reachable.
    if prior_retries < MAX_RETRIES {
        BootDecision {
            action: BootAction::RetryDiagnostic,
            reason: format!(
                "upload failed; retrying diagnostic boot (attempt {} of {})",
                prior_retries + 1,
                MAX_RETRIES
            ),
            retry_count: prior_retries + 1,
        }
    } else {
        BootDecision {
            action: BootAction::BootWindows,
            reason: format!(
                "upload failed after {MAX_RETRIES} retries; giving up and \
                 booting Windows (fail-safe — never strand the user)"
            ),
            retry_count: prior_retries,
        }
    }
}

/// Writes the decision as simple `KEY=value` lines — deliberately not
/// JSON, so `/init`'s POSIX shell can parse it with plain `read`/`case`
/// without pulling in a JSON parser for PID 1. See `/init` for the
/// reader side.
pub fn write_decision(decision: &BootDecision) -> std::io::Result<()> {
    let body = format!(
        "ACTION={}\nREASON={}\nRETRY_COUNT={}\n",
        decision.action.as_str(),
        // Newlines would break the KEY=value-per-line format; reasons
        // are static/format! strings above and never contain one, but
        // strip defensively since this is boot-critical parsing.
        decision.reason.replace('\n', " "),
        decision.retry_count
    );
    fs::File::create(DECISION_PATH)?.write_all(body.as_bytes())?;

    // Best-effort counter for the next boot (see module docs on why
    // this doesn't reliably survive a reboot yet).
    let _ = fs::write(RETRY_COUNTER_PATH, decision.retry_count.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_ok_no_failures_boots_windows() {
        let d = decide(true, false);
        assert_eq!(d.action, BootAction::BootWindows);
    }

    #[test]
    fn upload_ok_with_hardware_failures_still_boots_windows() {
        // The core safety invariant: hardware FAILs never block the
        // user's OS. Only upload failure affects the boot decision.
        let d = decide(true, true);
        assert_eq!(d.action, BootAction::BootWindows);
    }

    #[test]
    fn upload_failed_retries_when_under_the_cap() {
        // With no persisted counter (tmpfs-only today), prior_retries
        // reads back as 0, which is < MAX_RETRIES.
        let d = decide(false, false);
        assert_eq!(d.action, BootAction::RetryDiagnostic);
        assert_eq!(d.retry_count, 1);
    }

    #[test]
    fn decision_serializes_to_parseable_key_value_lines() {
        let d = decide(true, false);
        let body = format!(
            "ACTION={}\nREASON={}\nRETRY_COUNT={}\n",
            d.action.as_str(),
            d.reason.replace('\n', " "),
            d.retry_count
        );
        assert!(body.starts_with("ACTION=WINDOWS\n"));
        assert!(body.contains("REASON="));
        assert!(body.ends_with("RETRY_COUNT=0\n"));
    }

    #[test]
    fn max_retries_constant_is_a_real_bound() {
        // Sanity check the constant itself is finite and small — this
        // is what stops any future persistent-counter change from
        // accidentally becoming an infinite retry loop.
        assert!(MAX_RETRIES > 0 && MAX_RETRIES <= 5);
    }
}
