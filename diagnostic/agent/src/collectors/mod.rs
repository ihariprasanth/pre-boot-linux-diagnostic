//! Individual hardware/kernel collectors.
//!
//! Phase 3 implements: cpu, memory, kernel, pci, storage.
//! Phase 8 implements: gpu, usb, network, sensors.
//!
//! Every collector follows the same contract: it never panics, it
//! returns partial/None data for anything unavailable rather than
//! erroring out, and it returns a `Vec<DiagnosticResult>` describing
//! what it found so main.rs/report.rs can aggregate a health summary
//! and a JSON report without knowing the internals of any one
//! collector. As of Phase 4, every `*Info`/device struct here also
//! derives `serde::Serialize` and its shape is mirrored 1:1 in
//! `schemas/diagnostic-report.schema.json` — keep the two in sync.

pub mod cpu;
pub mod gpu;
pub mod kernel;
pub mod memory;
pub mod network;
pub mod pci;
pub mod sensors;
pub mod storage;
pub mod usb;
