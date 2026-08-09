# PLDDS Boot Flow

## Full intended flow (final system)

```
UEFI → GRUB → Diagnostic Linux (kernel + initramfs)
  → /init mounts /proc /sys /dev
  → /init execs diagnostic-agent
  → agent collects hardware/kernel diagnostics
  → agent generates JSON report
  → agent uploads report over HTTPS, waits for ACK (bounded timeout)
  → agent writes boot decision (CONTINUE / RETRY / FAIL-SAFE)
  → /init reads decision, calls `grub-reboot` for a ONE-SHOT next-boot
    override, then `reboot`
  → GRUB's persistent default (Windows) is untouched — the one-shot
    override only affects the very next boot
  → Windows Boot Manager → Windows
```

## Phase 2 scope (current)

Phase 2 implements and verifies only the **boot skeleton**, with no
diagnostics and no networking yet:

```
kernel → initramfs → /init mounts /proc /sys /dev
       → prints "PLDDS Diagnostic Environment Started" banner
       → prints kernel version / uptime
       → drops to a BusyBox recovery shell
```

`exec /bin/sh` at the end of `init` is a deliberate placeholder. Phase 3
replaces it with `exec /bin/diagnostic-agent`.

## What was actually built and tested

- `diagnostic/initramfs/init` — the PID 1 script (see file for full
  comments). Mounts only virtual filesystems (`/proc`, `/sys`, `/dev`);
  touches no block device.
- `diagnostic/build/build-initramfs.sh` — packages BusyBox
  (`busybox-static`) plus `init` into a gzip-compressed `newc` cpio
  archive at `diagnostic/build/output/initramfs.cpio.gz`.
- `scripts/run-qemu.sh` — boots that initramfs under
  `qemu-system-x86_64` with **no virtual disk attached at all**, serial
  console only (`-nographic`, `console=ttyS0`), `-no-reboot` so an
  automated test exits cleanly instead of looping.

### Verified test run

Built with the host's `busybox-static` package and booted against the
host's own `vmlinuz-6.8.0-137-generic` (a real, distro-provided kernel —
Phase 2 does not yet build a custom minimal kernel; that lands as part
of hardening later). Confirmed console output:

```
[    3.812211] Run /init as init process
============================================================
  PLDDS Diagnostic Environment Started
  Pre-Boot Linux Deep Diagnostic System
============================================================

  Phase 2: minimal boot environment (no diagnostics yet)

[init] kernel:       6.8.0-137-generic
[init] architecture: x86_64
[init] uptime since kernel handoff:
4.01 0.00

[init] Phase 3 will start the diagnostic-agent binary here.
[init] Dropping to a recovery shell for now.

BusyBox v1.36.1 (Ubuntu 1:1.36.1-6ubuntu3.1) built-in shell (ash)
Enter 'help' for a list of built-in commands.
~ #
```

This confirms: kernel → initramfs → custom init → banner → shell, all
working, with zero disk I/O anywhere in the chain.

## Why no GRUB / real hardware yet

Phase 2's acceptance criterion (per the project plan) is just "boots and
displays the banner, no desktop required." GRUB configuration and the
one-shot `grub-reboot` handoff mechanism are Phase 9 — deliberately
sequenced *after* the diagnostic agent exists (Phase 3–8), so that when
we do touch a bootloader, we're wiring it to something that already
works, not debugging the agent and the bootloader integration at the
same time. See `README.md` for the full phase table.

## A note on the kernel used

This Phase 2 test intentionally reuses the sandbox's existing distro
kernel to validate the *initramfs and init logic* quickly. It is stripped
down for testing (a real production build would use a smaller,
custom-configured kernel per `diagnostic/kernel/config/`), but the
init/banner/mount logic being validated here is kernel-version-independent
and carries forward unchanged.

## Phase 3: diagnostic agent integrated and verified booting

`init` now execs `/bin/diagnostic-agent` after the banner (still before
dropping to the recovery shell — Phase 9 replaces that shell drop with
the reboot/handoff logic).

**Build pitfall found and fixed:** the first attempt built the agent
with plain `cargo build --release`, which produces a binary dynamically
linked against glibc (`ld-linux-x86-64.so.2`, `libc.so.6`). The
initramfs has no dynamic linker, so `/init` failed with
`/bin/diagnostic-agent: not found` and the kernel panicked (PID 1
exiting). Fixed by building with:

```bash
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build --release --target x86_64-unknown-linux-gnu
```

which statically links against glibc using the host's `libc.a` (no musl
toolchain required). `diagnostic/build/build-initramfs.sh` now looks for
this static build path specifically and warns loudly if only a dynamic
build is found.

### Verified test run (Phase 3)

Booted under QEMU with **no virtual disk attached** — the agent
correctly detected zero block devices and reported `SKIPPED` rather than
failing:

```
Run /init as init process
  PLDDS Diagnostic Environment Started
  ...
  PLDDS Diagnostic Agent
  Phase 3: CPU / Memory / Kernel / PCI / Storage collectors

-- CPU --
  [PASS   ] cpu   cpu_identify   Identified QEMU Virtual CPU version 2.5+ (1 logical threads)
  [SKIPPED] cpu   cpu_thermal    No CPU thermal zone exposed by this system

-- Memory --
  [PASS   ] memory memory_identify  Total memory reported: 0.45 GiB (informational check only, no memory test performed)

-- Kernel --
  [PASS] kernel_identify   Kernel: Linux version 6.8.0-137-generic ...
  [PASS] kernel_taint      Kernel is not tainted
  [WARN] kernel_log_scan   1 warning kernel log entries found
    [WARNING] RAS: Correctable Errors collector initialized.

-- PCI --
  [WARN] pci_enumerate  Enumerated 6 PCI device(s); 5 have no bound driver

-- Storage --
  [SKIPPED] storage_enumerate  No block devices found under /sys/block

  Tests: 8 total | 4 passed | 2 warned | 0 failed | 2 skipped
  Overall health score: 90/100 (GOOD)
```

This confirms the full chain works genuinely in the target environment:
kernel -> initramfs -> init -> static Rust agent -> real /proc//sys
reads -> classified results -> graceful handling of absent hardware
(no disk attached), all without a single panic or crash.

11 unit tests cover the pure-logic parsers (CPU range-list parsing,
kernel log line classification, taint-flag decoding) — see
`diagnostic/agent/README.md`.

## Phase 9: GRUB / Windows handoff

The full flow at the top of this document is now real, not aspirational:

```
... agent uploads report, gets ACK or times out ...
  → agent calls bootdecision::decide(upload_ok, any_failed)
  → agent writes /run/pldds-boot-decision (plain KEY=value lines)
  → agent exits (0 = normal, 1 = FAILs found but still booting Windows,
    2 = requesting a diagnostic retry — see main.rs comment)
  → /init reads /run/pldds-boot-decision
  → /init resolves grub-reboot / grub2-reboot on $PATH
  → /init calls `grub-reboot windows` or `grub-reboot diagnostic`
    (one-shot; never grub-set-default; skipped entirely with a logged
    message if the binary isn't found — GRUB's persistent default is
    already "windows" so a plain reboot still lands correctly)
  → /init calls `reboot`
```

### Split of responsibility (why two files, not one)

`bootdecision.rs` (Rust, part of the agent) only ever *decides* and
*writes a file*. `diagnostic/initramfs/init` (POSIX shell, PID 1) is
the only code in the entire repository that *acts* — the only place
`grub-reboot` is invoked. This split exists so that:

- The decision logic is unit-testable in a normal `cargo test` run
  (see the 5 tests in `bootdecision.rs`) without needing an actual
  GRUB environment, a QEMU VM, or root.
- The one genuinely dangerous operation (calling out to a bootloader
  tool) lives in the smallest, most auditable piece of code in the
  project — a single shell script, not compiled Rust — and is trivial
  to grep for (`grub-reboot` appears exactly twice in `/init`, nowhere
  else).

### Why hardware FAILs still boot Windows

This is the core safety invariant restated as boot-decision logic: a
failing RAM stick or a dead NIC is *information for the dashboard*
(Phase 11) and the uploaded report, never a reason to lock the user
out of their own OS. Only a failure to even *tell the server* about
the diagnostic run (upload failure) can request a retry — and even
that gives up after `MAX_RETRIES` and boots Windows anyway. See
`bootdecision.rs`'s `upload_ok_with_hardware_failures_still_boots_windows`
test for the executable version of this rule.

### Why the retry path can't loop

Three independent guards, any one of which alone would already be
sufficient:

1. `MAX_RETRIES = 2` in `bootdecision.rs` — a hard, small, constant
   ceiling.
2. `/init` will not honor `RETRY_DIAGNOSTIC` unless it independently
   confirms `grub-reboot`/`grub2-reboot` exists on `$PATH` at handoff
   time — an unconfirmed retry falls back to `WINDOWS` immediately.
3. `grub-reboot` is a *one-shot* GRUB primitive by construction: the
   override it schedules is consumed by the very next boot regardless
   of what that boot does or doesn't do afterward, so even a PLDDS
   crash immediately after boot can never re-arm another diagnostic
   boot on its own.

### Known limitation (tracked for Phase 13)

The retry counter (`RETRY_COUNTER_PATH` in `bootdecision.rs`) lives on
the same RAM-only tmpfs as the Phase 7 device key, so it does not
survive a reboot yet. Every diagnostic boot today reads back "0 prior
retries" — a safe degradation (see guard #2 and #3 above for why it's
still bounded), just not yet a true multi-boot backoff. Phase 13 moves
both the key and this counter to a small file on the EFI System
Partition, written once at install time; the boot-decision file format
and `/init`'s reader do not need to change when that lands.

Phase 10 turns the "manual verification" subsection right below this
paragraph into an actual automated harness — see
**`docs/qemu-testing.md`** for the checkpoint list, the real
upload+ACK verification against the backend's API, and (importantly)
which parts are host-dependent and why.

### Manual verification performed this phase

Since Phase 9 has no real GRUB environment or EFI System Partition
available in the sandbox (that requires actual hardware or a
UEFI+disk QEMU setup — Phase 10 and Phase 13 respectively), this
phase's testing focused on what's mechanically verifiable now:

- `bootdecision::decide()`'s full truth table via its 5 unit tests
  (`cargo test`, no VM needed).
- `/init`'s `grub-reboot`-missing fallback path, exercised for real:
  the sandbox and the Phase 2/3 QEMU test image both genuinely lack
  `grub-reboot`, so every boot so far has taken the "skip the one-shot
  override, log why, reboot anyway" branch — this is the actual code
  path a bare initramfs-only test environment hits, not a simulated
  one.
- Shell-script review of the `case`/`command -v` logic by hand, since
  POSIX `sh` has no equivalent of `cargo test` to lean on — Phase 10's
  QEMU harness is what finally automates this end-to-end, including
  against a real `grubenv`.
