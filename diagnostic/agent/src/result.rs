//! Shared result/status/severity types produced by every collector.
//!
//! This mirrors the "Diagnostic Result Model" from the project spec
//! (docs/architecture.md section on diagnostics). As of Phase 4 these
//! derive `serde::Serialize` so they flow straight into the JSON report
//! (see `report.rs` and `schemas/diagnostic-report.schema.json`) with
//! no separate DTO/mapping layer to keep in sync.

use serde::Serialize;
use std::fmt;
use std::time::Instant;

/// Serializes as `"PASS"` / `"WARN"` / ... — kept identical to
/// `Display` on purpose so the JSON report and the console text
/// summary never disagree on wording. See
/// `schemas/diagnostic-report.schema.json#/$defs/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skipped,
    Unknown,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Skipped => "SKIPPED",
            Status::Unknown => "UNKNOWN",
        };
        write!(f, "{s}")
    }
}

/// Serializes as `"INFO"` / `"WARNING"` / ... — see
/// `schemas/diagnostic-report.schema.json#/$defs/severity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Error => "ERROR",
            Severity::Critical => "CRITICAL",
        };
        write!(f, "{s}")
    }
}

/// The outcome of a single diagnostic test, independent of which
/// component it belongs to. Every collector produces one or more of
/// these; `report.rs` aggregates them into the final JSON report.
/// Field names match `schemas/diagnostic-report.schema.json#/$defs/diagnostic_result`
/// exactly — do not rename a field here without updating the schema
/// (and bumping `report::SCHEMA_VERSION`) in the same change.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticResult {
    pub test: String,
    pub component: String,
    pub status: Status,
    pub severity: Severity,
    pub message: String,
    pub duration_ms: u64,
}

impl DiagnosticResult {
    pub fn new(
        test: impl Into<String>,
        component: impl Into<String>,
        status: Status,
        severity: Severity,
        message: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            test: test.into(),
            component: component.into(),
            status,
            severity,
            message: message.into(),
            duration_ms,
        }
    }
}

/// Small helper so every collector times itself the same way without
/// repeating boilerplate. Usage:
///
/// ```ignore
/// let timer = Timer::start();
/// // ... do work ...
/// let ms = timer.elapsed_ms();
/// ```
pub struct Timer(Instant);

impl Timer {
    pub fn start() -> Self {
        Self(Instant::now())
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}
