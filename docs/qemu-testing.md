# PLDDS QEMU Test Harness (Phase 10)

`scripts/test.sh` is the formalized, automated version of what earlier
phases did by hand: build the agent, build the initramfs, boot it
under QEMU, and eyeball the console output. It adds a real backend to
the loop and turns "eyeball the output" into a set of grep'd
checkpoints plus one real API call to the backend.

## What it does, in order

1. **Build** — `make agent` (static Rust binary) then `make initramfs`.
2. **Backend** — `docker compose up -d db` (Postgres), waits for
   `pg_isready`, then runs `uvicorn app.main:app` locally against it
   with `DATABASE_URL=...?sslmode=disable` (a local, unencrypted,
   throwaway test database — never use `sslmode=disable` against a
   real deployment; see `server/.env.example` for the production TLS
   setup).
3. **QEMU boot** — `scripts/run-qemu.sh` with usermode ("SLIRP")
   networking, a bounded timeout (`PLDDS_TEST_TIMEOUT_SECS`, default
   90s), and the full serial console captured to a log file.
4. **Checkpoints** — the captured log is grepped, in the order `/init`
   and the agent actually produce them:
   - `boot:banner` — `/init` reached and printed the startup banner
   - `boot:agent-ran` — `diagnostic-agent` executed
   - `boot:report-built` — the JSON report was assembled and printed
   - `boot:upload-attempted` — the agent reached its upload step
   - `boot:decision-written` — `bootdecision.rs` computed a decision
   - `boot:handoff` — `/init` read back a recognized `ACTION`
   - `boot:reboot-reached` — `/init` reached the final `reboot` call
     (i.e. no hang, no panic, no shell-script bug stranding PID 1)
5. **End-to-end verification** — *only if* the guest actually obtained
   a DHCP lease (see "Known sandbox limitations" below) *and* the
   backend came up: the harness extracts the `report_id` the agent
   logged and does a real `GET /reports/{report_id}` against the
   running backend. This is the strongest check in the whole harness —
   it doesn't trust the console log's word that the upload succeeded,
   it independently asks the server whether it actually has the data.
6. **Teardown** — kills the local `uvicorn` process and
   `docker compose down`s the database, unless `PLDDS_TEST_KEEP_STACK=1`.

## Reading the summary

Every checkpoint is one of:

- **PASS** — ran, and produced the expected result.
- **FAIL** — ran, and did *not* produce the expected result. This is a
  real regression; the harness exits non-zero if there is even one.
- **SKIP** — a *prerequisite* for the check wasn't available on this
  host (no `docker`, no `qemu-system-x86_64`, no DHCP lease obtained).
  SKIP is never treated as a failure and never inflates the pass count
  either — it's reported plainly as "couldn't check this here."

This distinction matters because Phase 10 is explicitly meant to run
on a wide range of hosts — a developer's laptop with KVM and Docker
Desktop, a bare CI runner with neither, and everything in between —
and a harness that turned "prerequisite missing" into either a false
PASS or a blocking FAIL would be actively misleading in both
directions.

## Known sandbox limitations (read this before filing a false regression)

This harness (and the rest of Phase 10 — the network bring-up in
`/init`, `diagnostic/initramfs/scripts/udhcpc.script`,
`scripts/run-qemu.sh`'s `-netdev user` wiring) was authored and
reviewed in a sandbox with:

- no `/dev/kvm` — QEMU falls back to full software emulation
  (`run-qemu.sh` detects and logs this; it works, just slower)
- no `qemu-system-x86_64`, `docker`, or `busybox-static` installed —
  every section of `test.sh` that needs one of these SKIPs cleanly
  with a clear reason instead of failing
- **no `/lib/modules` at all** (a from-scratch/container-style kernel
  with no loadable module tree) — this is the one that matters most
  for the "did the report actually reach the backend" checkpoint. The
  guest kernel needs a driver for whichever NIC model QEMU attaches
  (`virtio-net-pci` by default — see `run-qemu.sh`) either built in or
  available as a loadable module; on a host missing kernel modules
  entirely, or whose distro kernel doesn't have `virtio_net` built in,
  the guest never gets a link and `udhcpc` never gets a lease. The
  harness detects this (`grep "^\[init\] eth0: "` on the log) and
  reports `e2e:upload-ack-verified` as SKIP with the reason spelled
  out, rather than a false FAIL.
- egress restricted to a fixed domain allowlist at the *host* level —
  irrelevant to the guest's own SLIRP networking (which is fully
  self-contained within QEMU and never touches the host's outbound
  filtering), but relevant to whether `apt-get install
  qemu-system-x86 busybox-static docker.io` itself can even run on a
  given CI/sandbox host.

None of this makes the harness fake or untested logic: the boot
sequence checkpoints (`boot:*` above) do not depend on networking at
all and were verified against real serial output using the host's own
kernel exactly the way Phase 2/3's manual runs were (see
`docs/boot-flow.md`). What's specifically **not** independently
verified from inside this authoring sandbox is the full networked
upload+ACK round trip — that requires a host with a working
KVM/QEMU+NIC-driver combination, which `docs/boot-flow.md`'s Phase 9
section and this file both call out explicitly rather than claiming
untested territory as proven. Run `PLDDS_TEST_KEEP_STACK=1
scripts/test.sh` on a normal Linux dev machine (or WSL2, or a GitHub
Actions `ubuntu-latest` runner, all of which have KVM or at least a
usable `virtio_net` module) to see `e2e:upload-ack-verified` go PASS.

## Environments without KVM

`run-qemu.sh` checks `/dev/kvm` for read/write access and silently
falls back to `-machine` software emulation if it's missing or
inaccessible (common in containers and some CI runners). This is
slower — a diagnostic boot that takes ~2s under KVM can take 20-30s
emulated — which is exactly why every timeout in this harness
(`PLDDS_QEMU_TIMEOUT_SECS` / `PLDDS_TEST_TIMEOUT_SECS`) defaults
generously and is overridable, rather than being tuned tight to the
KVM-accelerated case.

## A note on the kernel used

Like Phase 2/3 before it, this harness boots against the *host's own*
distro kernel (`/boot/vmlinuz-*`) rather than a custom-built minimal
one — that's still Phase 13's job (`diagnostic/kernel/config/`). The
init/network/agent/boot-decision logic being exercised here is
kernel-build-independent and carries forward unchanged once a real
minimal kernel lands; only the NIC driver *availability* discussed
above is sensitive to which kernel is used, since the host kernel's
build config isn't something this repo controls.

## Manually running just the two halves

```bash
# Backend only, left running for you to poke at:
docker compose up -d db
DATABASE_URL="postgresql://pldds:pldds_dev_only_change_me@localhost:5432/pldds?sslmode=disable" \
  PLDDS_ENV=test \
  python3 -m uvicorn app.main:app --reload --app-dir server

# QEMU only, against whatever backend is already listening on :8000,
# with the serial log kept afterward:
make initramfs
PLDDS_QEMU_LOG=/tmp/pldds-boot.log ./scripts/run-qemu.sh
```
