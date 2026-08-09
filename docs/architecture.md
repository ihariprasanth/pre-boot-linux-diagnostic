# PLDDS Architecture

## 1. Overview

PLDDS boots a minimal, ephemeral Linux environment before Windows, runs a
hardware/kernel diagnostic, uploads a structured report to a backend, waits
for acknowledgement, then hands control back to Windows Boot Manager.

```
UEFI → GRUB → Diagnostic Linux → Hardware Diagnosis → JSON Report
   → HTTPS Upload → Server ACK → Reboot → Windows Boot Manager → Windows
```

## 2. Components

### Diagnostic Linux
- Linux kernel (minimal config, no GUI subsystems)
- BusyBox-based initramfs
- Custom `/init` (PID 1) — orchestrates mounts, networking, agent execution
- Diagnostic agent (Rust binary) — the actual diagnostic logic

### Backend
- FastAPI (Python) REST API
- PostgreSQL for devices, reports, results, boot sessions, events
- Device-credential authentication, no shared tokens

### Dashboard
- React/TypeScript, read-only view over backend data

## 3. Technology choices and rationale

| Component | Choice | Rationale |
|---|---|---|
| Diagnostic agent | Rust | Static binary, no runtime dependency, memory safety while running as effectively-root pre-boot, mature TLS via rustls |
| Init | BusyBox + custom shell init | Single-purpose ephemeral environment; systemd is unnecessary overhead |
| Bootloader | GRUB2 | Standard on most Windows dual-boot UEFI systems; supports one-shot `grub-reboot` |
| Backend | FastAPI | Fast iteration, Pydantic schema validation |
| DB | PostgreSQL | Relational integrity for devices/reports, JSONB for raw report retention |

## 4. Boot flow detail

1. UEFI firmware loads GRUB from the EFI System Partition.
2. GRUB's **normal default entry is Windows**. A diagnostic boot only happens
   when explicitly selected, or scheduled via a prior `grub-reboot` one-shot
   override (never `grub-set-default`).
3. GRUB loads the diagnostic kernel + initramfs.
4. `/init` mounts `/proc`, `/sys`, `/dev`, brings up networking, execs the
   agent.
5. Agent collects hardware/kernel data, builds a JSON report, uploads over
   TLS with device authentication, waits for ACK within a bounded timeout.
6. Agent writes a boot decision to a local state file.
7. `/init` reads the decision and reboots.
8. Because the diagnostic boot was one-shot, the *next* boot reverts to
   GRUB's normal default (Windows) automatically — this is the core
   anti-brick mechanism. No permanent bootloader state changes.

## 5. Security model

- **Transport**: TLS everywhere; no plaintext report submission in production.
- **Device identity**: per-device keypair provisioned at install; requests
  signed (HMAC or Ed25519), verified server-side against stored public key.
- **Replay protection**: `boot_id` (random per boot) + timestamp + nonce;
  server rejects duplicate/replayed reports.
- **Device ID**: privacy-conscious hash of stable, non-sensitive fields.
  Raw serial numbers are opt-in only (`privacy.collect_serial_numbers`).
- **Minimal attack surface**: no SSH, no unnecessary BusyBox applets, no
  persistent shell access in the initramfs.

## 6. Failure safety

- Diagnostic environment failure must never brick the machine.
- All network/server interactions are bounded by configurable timeouts.
- Default boot mode is `GRACEFUL`: attempt upload for N seconds, then
  continue to Windows regardless of outcome.
- GRUB always retains a manually-selectable Windows entry.
- Every module (CPU, GPU, storage, etc.) fails independently — one failed
  collector does not abort the rest of the diagnostic run.

## 7. Data flow / report lifecycle

```
Diagnostic Agent → JSON Report → HTTPS POST /api/v1/diagnostics
   → Backend validates schema, device identity, timestamp, size
   → Backend stores report + normalized results
   → Backend responds with ACK + boot_action
   → Agent honors boot_action, proceeds to reboot
```

## 8. Report schema

As of Phase 4, the JSON report the agent produces (step 5 above) is
formally specified in `schemas/diagnostic-report.schema.json` (JSON
Schema draft 2020-12). It mirrors the agent's Rust types 1:1:

- `result::DiagnosticResult` → `$defs/diagnostic_result`
- `collectors::*::{CpuInfo, MemoryInfo, KernelInfo, PciDevice, StorageDevice}`
  → `$defs/{cpu_info, memory_info, kernel_info, pci_device, storage_device}`
- `report::{DeviceIdentity, Summary, DiagnosticReport}` → the top-level
  document plus `$defs/device_identity` and `$defs/summary`
- Phase 8: `collectors::*::{GpuDevice, UsbDevice, NetworkInterface, SensorReading}`
  → `$defs/{gpu_device, usb_device, network_interface, sensor_reading}`,
  added as new required `sections` keys (see 8.1-8.4 below). Additive to
  the report shape, but `sections` now requires all nine keys, so this
  is a schema-breaking change for any consumer still validating against
  the Phase 4-7 shape — bump anything pinned to schema v1.0.

### 8.1 GPU diagnostics

Source: `/sys/class/drm/cardN` (connector nodes like `card0-HDMI-A-1`
are filtered out — they're display outputs, not devices). Read-only:
vendor/device ID, bound driver, and — where the driver exposes one — a
junction/die temperature via `device/hwmon/hwmonN/temp1_input`. A
missing GPU (headless boxes, most VMs) is `SKIPPED`/`INFO`, never a
failure. Temperature thresholds: PASS < 90C, WARN 90-99C, FAIL >= 100C.

### 8.2 USB diagnostics

Source: `/sys/bus/usb/devices/`. Root hubs (`usbN`) and interface nodes
(`N-N:1.0`) are filtered so the count matches `lsusb`. Read-only:
vendor/product ID, manufacturer/product strings, negotiated link speed.

### 8.3 Network diagnostics

Source: `/sys/class/net/` (loopback excluded — not hardware). Read-only:
operational state, carrier, MAC address, negotiated speed, and whether
the interface is wireless (`wireless/` or `phy80211` present). Link
state feeds a per-interface result: `up` → PASS, `down` → WARN.

### 8.4 Sensor diagnostics

Source: `/sys/class/hwmon/` — the standard lm-sensors-compatible kernel
ABI. Read-only enumeration of every `temp*_input`, `fan*_input`, and
`in*_input` channel across every hwmon chip (board/VRM/PSU sensors, not
just the one representative CPU/GPU reading `cpu.rs`/`gpu.rs` already
report). Thresholds mirror the CPU/GPU ones (WARN >= 85C, FAIL >= 100C
for temperature) plus a WARN on any fan reporting 0 RPM.

## 9. Development phases

See `README.md` for the full phase table. This document will be extended
with `boot-flow.md`, `security.md`, `recovery.md`, and `development.md` as
those phases are implemented.
