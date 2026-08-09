# PLDDS diagnostic agent

Current (Phase 9) scope: CPU, memory, kernel, PCI, storage, GPU, USB,
network, and sensor collectors (Phases 3 & 8) assembled into a
structured `DiagnosticReport` (`src/report.rs`), serialized to JSON
matching `../../schemas/diagnostic-report.schema.json`, signed and
uploaded over HTTPS with replay protection (Phases 5-7,
`src/upload.rs` / `src/crypto.rs`), and finally turned into a boot
decision (`src/bootdecision.rs`) that `/init` acts on to hand off back
to Windows or retry the diagnostic boot. The agent still prints a
human-readable progress summary as it runs, then prints the JSON
report and best-effort writes it to `/run/pldds-report.json`. See the
module docs in `src/main.rs` and `src/bootdecision.rs` for what's
deliberately deferred to later phases (persistent storage, Phase 13).

## Build

**For local testing on your dev machine** (dynamically linked, fine for
`cargo run`/`cargo test`, but will NOT run inside the initramfs):

```bash
cargo build --release
./target/release/diagnostic-agent
```

**For the actual initramfs** — must be statically linked, since the
initramfs has no dynamic linker or libc:

```bash
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build --release --target x86_64-unknown-linux-gnu
```

This produces `target/x86_64-unknown-linux-gnu/release/diagnostic-agent`,
a fully static binary (`ldd` reports "statically linked"). Requires
`libc6-dev` (specifically `libc.a`) on the build host — no musl toolchain
needed.

`diagnostic/build/build-initramfs.sh` looks for the static build first
and automatically includes it; if only the dynamic build exists it warns
loudly and includes it anyway (so you find out from a clear message, not
a kernel panic on boot).

## Test

```bash
cargo test
```

Covers the pure-logic parsers (CPU range list parsing, kernel log line
classification, taint flag decoding), report assembly and scoring
(`src/report.rs`), and, as of Phase 9, the full boot-decision truth
table (`src/bootdecision.rs` — upload success/failure × hardware
FAIL/no-FAIL, plus the retry-cap and decision-file-format checks) —
all of these run against synthetic data, not real `/proc`/`/sys` reads
or an actual GRUB environment, so they behave identically on any dev
machine or CI runner.

## Design notes

- **No panics.** Every collector uses the helpers in `src/sysfs.rs`
  (`read_trimmed`, `read_u64`, `list_dir_names`, `read_link_basename`)
  which return `None`/empty on any missing or unreadable file rather
  than erroring. A missing sensor is data, not a bug.
- **No destructive operations, ever.** Storage and PCI collectors are
  pure enumeration/read-only. The one opportunistic external command
  (`smartctl -H`) is a read-only health query, and its absence is
  handled the same as any other missing optional tool.
- **Report shape lives in one place.** `src/report.rs` mirrors
  `../../schemas/diagnostic-report.schema.json` field-for-field. Every
  `*Info`/device struct's doc comment points at the `$defs` entry it
  corresponds to — if you rename or add a field on either side, update
  the other in the same change.
- **The agent decides, `/init` acts.** `bootdecision.rs` only computes
  a `BootDecision` and writes it to a tmpfs file — it never calls
  `grub-reboot` or `reboot` itself. That stays in
  `../initramfs/init` (POSIX shell) on purpose; see that file's header
  comment and `../../docs/boot-flow.md` "Phase 9" for why.
- **Non-goals for this phase:** persistent device key / retry counter
  across reboots (Phase 13 — needs a real EFI System Partition, not
  RAM-only tmpfs), the formalized QEMU boot+diagnose+upload+ACK+reboot
  test harness (Phase 10), and the dashboard that will actually
  display these reports to a human (Phase 11).
