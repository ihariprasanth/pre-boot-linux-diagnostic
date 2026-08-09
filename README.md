# PLDDS — Pre-Boot Linux Deep Diagnostic System

A minimal Linux environment that boots before Windows, runs a deep hardware/kernel
diagnostic, uploads a structured report to a backend server, waits for
acknowledgement, and then hands control back to Windows.

```
POWER ON → UEFI → GRUB → Diagnostic Linux → Hardware Diagnosis → JSON Report
   → HTTPS Upload → Server ACK → Reboot → Windows Boot Manager → WINDOWS
```

## Core safety invariant

The diagnostic environment **never modifies the Windows partition or Windows
boot files**. The only bootloader operation performed is a one-shot
`grub-reboot` next-boot override. If the diagnostic boot fails to run for any
reason, GRUB automatically falls back to its normal default on the following
boot. Windows can always be selected manually from the GRUB menu.

## Project structure

```
pldds/
├── diagnostic/        Rust agent + initramfs + kernel config + GRUB config + install/
├── server/             FastAPI backend + Postgres migrations
├── dashboard/          React/TypeScript read-only dashboard
├── schemas/            JSON schema for diagnostic reports
├── docs/                Architecture, boot flow, security, recovery docs
└── scripts/             dev/test/QEMU helper scripts
```

## Development phases

| Phase | Goal |
|---|---|
| 1 | Repo scaffold ✅ |
| 2 | Minimal Linux diagnostic environment boots and prints a banner ✅ |
| 3 | Diagnostic agent: CPU / RAM / kernel / PCI / storage collectors ✅ |
| 4 | Report JSON schema ✅ |
| 5 | Backend API ✅ |
| 6 | Agent ↔ API integration ✅ |
| 7 | Authentication + TLS ✅ |
| 8 | Remaining hardware diagnostics (GPU, USB, network, sensors) ✅ |
| 9 | GRUB / Windows handoff ✅ |
| 10 | QEMU test environment ✅ |
| 11 | Dashboard ✅ |
| 12 | Security hardening ✅ |
| 13 | Real hardware testing ✅ *(current)* |

## Status

Currently: **Phase 13 — Real hardware testing**. Adds
`diagnostic/install/` — the actual install/uninstall/recovery tooling
for a real dual-boot machine, plus the runbook
(`docs/real-hardware-testing.md`) walking through it step by step:

- `preflight-check.sh` — read-only checks (UEFI mode, Secure Boot
  state, Windows Boot Manager present, build artifacts exist, free
  space) before anything is written.
- `backup-grub-config.sh` — snapshots the current `grub.cfg`/`grubenv`/
  `40_custom` before any change, with a manifest recording exactly
  where each came from.
- `make-recovery-usb.sh` — builds a standalone bootable USB (via
  `grub-mkstandalone`) that can chainload Windows independently of the
  internal disk's own GRUB, carrying the grub.cfg backup and repo docs
  — meant to be built and verified to boot *before* `install.sh` ever
  runs.
- `install.sh` — copies `vmlinuz`/`initramfs.cpio.gz` onto the ESP,
  appends the PLDDS GRUB entries via a new `/etc/grub.d/41_pldds` (never
  edits Windows' own entry or hand-edits `grub.cfg`), regenerates via
  `update-grub`/`grub-mkconfig`, then verifies the result still
  defaults to Windows and still has a Windows entry — auto-rolling back
  via `uninstall.sh --restore-only` if not.
- `uninstall.sh` — full reverse: restores the pre-install GRUB files
  from backup, removes the PLDDS snippet and copied boot files,
  regenerates, and verifies.

Previously, **Phase 12 — Security hardening** shipped — see
`docs/security-hardening.md`. Before that, **Phase 11 — Dashboard**
React + TypeScript app (Vite) over the backend: a fleet overview
(devices, last-seen, latest score), a device detail view with full
boot history, and a report detail view rendering all nine diagnostic
sections (cpu/memory/kernel/storage/pci/gpu/network/usb/sensors) with
their per-check results. Run it with `make dashboard` (or
`scripts/dev.sh` for the whole stack) and open http://localhost:3000;
point `VITE_API_BASE_URL` (see `dashboard/.env.example`) at the backend
if it's not on the default `http://localhost:8000`.

To support it, the backend gained two read endpoints and CORS:
- `GET /devices` — paginated device list, ordered by last seen.
- `GET /devices/{device_id}/reports` — paginated report history for one
  device (this is what "boot history" renders).
- CORS (GET-only) for the dashboard's origin, via the new
  `PLDDS_DASHBOARD_ORIGINS` env var (default `http://localhost:3000`).

**Bugfix found while wiring this up:** `server/app/schemas.py`'s
`Sections` model was never updated for the Phase 8 collectors —
it only defined `cpu`/`memory`/`kernel`/`pci`/`storage`, while
`schemas/diagnostic-report.schema.json` and the agent have required
`gpu`/`usb`/`network`/`sensors` too since Phase 8. Because `Sections`
forbids extra fields, every report submitted by a Phase 8+ agent was
being rejected with a 422 before it ever reached storage. Added the
missing `GpuSection`/`UsbSection`/`NetworkSection`/`SensorsSection`
(and their `info` models) to match the JSON schema exactly — reports
now validate and store correctly, which is also what makes boot
history worth looking at in the dashboard.

Phase 10 recap: `scripts/test.sh` is
now the single command that exercises the full loop end to end:
builds the agent + initramfs, brings up a real Postgres + FastAPI
backend, boots it all under QEMU with a bounded timeout, and checks
both the console log *and* the backend's own API to confirm a report
genuinely made the round trip. See **`docs/qemu-testing.md`** for the
full checkpoint list and, importantly, its "Known sandbox limitations"
section — the harness is written to SKIP (not fake-pass, not
hard-fail) checks whose prerequisites aren't available on a given
host, which matters a lot for anything KVM/NIC-driver-dependent.

Key additions this phase beyond the harness itself:
- `diagnostic/initramfs/init` now brings up networking (best-effort
  DHCP via `udhcpc` on `eth0`) before running the agent, so the
  upload step (Phase 6/7) has something to talk to.
- `diagnostic/initramfs/scripts/udhcpc.script` — the handler script
  that actually configures the interface from a lease; without it
  `udhcpc` only prints the lease, it doesn't apply it.
- `scripts/run-qemu.sh` gained usermode networking
  (`-netdev user` / `virtio-net-pci`), a wall-clock timeout, serial
  log capture, and automatic KVM detection with a software-emulation
  fallback.

Phase 9 recap: the agent no longer
just uploads a report and stops — it now decides what the *next* boot
should be and records that decision for `/init` to act on:

- **`diagnostic/agent/src/bootdecision.rs`** — pure decision logic,
  `decide(upload_ok, any_failed) -> BootDecision`. The one hard rule:
  hardware FAIL results are informational only and never block the
  user's OS — only a failed *upload* can trigger a `RETRY_DIAGNOSTIC`
  decision, capped at `MAX_RETRIES = 2` before giving up and booting
  Windows regardless. Decision is written to `/run/pldds-boot-decision`
  as plain `KEY=value` lines (deliberately not JSON — see the file's
  doc comment on why).
- **`diagnostic/initramfs/init`** — now the *only* place in the entire
  system that shells out to `grub-reboot`. Reads the decision file,
  defaults to `WINDOWS` on anything it doesn't recognize (missing
  file, unknown action), and — critically — refuses to honor
  `RETRY_DIAGNOSTIC` unless it can confirm `grub-reboot`/`grub2-reboot`
  is actually present on `$PATH`, falling back to `WINDOWS` otherwise.
  That refusal is what makes the retry path provably bounded: it can
  never spin forever even if a future change to the decision logic
  tried to make it.
- **`diagnostic/grub/grub.cfg`** — the real (non-stub) GRUB entries:
  `windows` (fixed `--id`, persistent default, never touched by
  `set default=` at runtime by anything in this repo), `diagnostic`
  (the one-shot-selectable PLDDS boot), and `diagnostic-safe` (manual
  recovery entry with a `pldds.safe=1` cmdline flag, not scheduled by
  any automatic logic). Entry IDs are load-bearing strings shared with
  `/init` — see the comments in both files if renaming either.

**Known limitation carried into Phase 13** (same root cause as the
Phase 7 device-key limitation): the retry counter lives on the
RAM-only tmpfs alongside everything else in this initramfs, so it
doesn't survive a reboot yet — every diagnostic boot currently reads
back "0 prior retries." This is a safe degradation, not an unsafe one
(see `bootdecision.rs` module docs for why it still can't loop), just
not yet a *smart* bounded backoff. Phase 13 moves both the device key
and this counter to a small file on the EFI System Partition, written
once at install time.

Phase 8 recap: four new
read-only collectors round out the hardware sweep alongside the Phase 3
cpu/memory/kernel/pci/storage set:

- **GPU** (`collectors/gpu.rs`) — `/sys/class/drm/cardN` enumeration:
  vendor/device ID, bound driver, best-effort hwmon temperature.
- **USB** (`collectors/usb.rs`) — `/sys/bus/usb/devices/` enumeration
  filtered to real devices (root hubs/interface nodes excluded):
  vendor/product ID, manufacturer/product strings, link speed.
- **Network** (`collectors/network.rs`) — `/sys/class/net/` enumeration
  (loopback excluded): operstate, carrier, MAC, speed, wireless flag.
- **Sensors** (`collectors/sensors.rs`) — full `/sys/class/hwmon/` sweep
  across every chip's temp/fan/voltage channels, not just the one
  representative CPU/GPU reading the other two collectors already take.

All four follow the same never-panic, `Vec<DiagnosticResult>`-returning
contract as the Phase 3 collectors (see `docs/architecture.md` §8.1-8.4),
are wired into `report::Sections` and `main.rs`, and are mirrored in
`schemas/diagnostic-report.schema.json` (`gpu`/`usb`/`network`/`sensors`
are now required section keys — a breaking change from schema v1.0 for
any consumer still validating against the Phase 4-7 shape).

Phase 7 recap: every device provisions an Ed25519 keypair on first boot
(`diagnostic/agent/src/crypto.rs`) and derives its `device_id` as
SHA-256(public_key) — the privacy-conscious hashed identity
`docs/architecture.md` describes, replacing the Phase 4 placeholder.
Every request to the backend is signed (`X-Device-Id` / `X-Timestamp` /
`X-Nonce` / `X-Boot-Id` / `X-Signature` headers, see `upload.rs`'s
canonical-payload doc comment) and the server verifies it
(`server/app/security.py`) against a trust-on-first-use bootstrapped
public key, rejecting stale timestamps and replayed nonces
(`used_nonces` table) before ever validating the report body. TLS:
`PLDDS_SERVER_URL` should be `https://` for any real deployment
(`reqwest`'s `rustls-tls` makes the agent side work out-of-the-box) and
`PLDDS_REQUIRE_TLS=1` makes the agent refuse to even try a plain-`http://`
server — see `server/.env.example` for the production TLS setup
(reverse-proxy termination or uvicorn's own `--ssl-certfile`).

**Known limitation, tracked for Phase 9/13**: the device's private key
currently lives at `PLDDS_KEY_PATH` (default a tmpfs path under `/run`),
which doesn't survive a reboot in the current RAM-only initramfs — so
`device_id` changes every boot until a real persistent storage location
(e.g. a file on the EFI System Partition, written once at install time)
is wired up. The signing protocol itself won't need to change when
that lands.

Try it:
```bash
make agent-test  # runs the agent's unit tests
make initramfs   # builds the agent (static) + packages initramfs
make qemu        # boots it under QEMU; watch the JSON report print
```

Or natively, to see the JSON report on its own:
```bash
cd diagnostic/agent && cargo run --release
```

See `docs/architecture.md` for the full design and `docs/boot-flow.md`
for the verified boot transcript.
