#!/bin/bash
# Boot the PLDDS diagnostic environment under QEMU for testing.
#
# Phase 2: boots the initramfs directly against a host-provided kernel
# (BIOS mode, serial console, no disk attached at all — this test can
# never touch a real or virtual Windows installation).
#
# Phase 10: adds usermode ("SLIRP") networking so the agent's upload
# step has something to talk to, a bounded wall-clock timeout so an
# automated run can never hang forever, and optional serial-log
# capture for scripts/test.sh to grep afterward. Still zero virtual
# disks, still -no-reboot, still nothing that can touch a real
# Windows install — see docs/qemu-testing.md for the full contract
# this script and scripts/test.sh together provide.
#
# Usage:
#   ./run-qemu.sh [path-to-kernel]
#
# Env vars (all optional):
#   PLDDS_QEMU_TIMEOUT_SECS   wall-clock cap on the whole boot (default 60)
#   PLDDS_QEMU_LOG            if set, serial console is also teed to this
#                              file (scripts/test.sh sets this to parse
#                              the run afterward)
#   PLDDS_QEMU_NET            "user" (default, SLIRP — reaches the host's
#                              10.0.2.2) or "none" (no NIC attached at all,
#                              e.g. to specifically test the "no eth0"
#                              fallback path in /init)
#
# If no kernel path is given, it looks for a running host's own
# vmlinuz (useful for quick smoke tests) — see docs/qemu-testing.md
# "A note on the kernel used" for why this is fine pre-Phase-13 but
# gets replaced by our own minimal kernel build in a later phase.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
INITRAMFS="${ROOT_DIR}/diagnostic/build/output/initramfs.cpio.gz"

KERNEL="${1:-}"
if [ -z "${KERNEL}" ]; then
    KERNEL="$(ls /boot/vmlinuz-* 2>/dev/null | head -1 || true)"
fi

if [ -z "${KERNEL}" ] || [ ! -r "${KERNEL}" ]; then
    echo "ERROR: no readable kernel image found. Pass one explicitly:" >&2
    echo "  ./run-qemu.sh /path/to/vmlinuz" >&2
    exit 1
fi

if [ ! -f "${INITRAMFS}" ]; then
    echo "ERROR: initramfs not found at ${INITRAMFS}" >&2
    echo "Run diagnostic/build/build-initramfs.sh first." >&2
    exit 1
fi

TIMEOUT_SECS="${PLDDS_QEMU_TIMEOUT_SECS:-60}"
NET_MODE="${PLDDS_QEMU_NET:-user}"
LOG_FILE="${PLDDS_QEMU_LOG:-}"

echo "[run-qemu] kernel:    ${KERNEL}"
echo "[run-qemu] initramfs: ${INITRAMFS}"
echo "[run-qemu] net mode:  ${NET_MODE}"
echo "[run-qemu] timeout:   ${TIMEOUT_SECS}s"
echo "[run-qemu] Booting — no disk attached, nothing on this or any"
echo "[run-qemu] host disk can be modified by this test."
echo ""

NET_ARGS=()
case "${NET_MODE}" in
    user)
        # SLIRP usermode networking: the guest's default route (see
        # diagnostic/initramfs/scripts/udhcpc.script) points at
        # 10.0.2.2, which QEMU transparently NATs to the host — this
        # is why upload.rs's PLDDS_SERVER_URL default
        # (http://10.0.2.2:8000) needs no override to reach a backend
        # bound to the host's own :8000. No -netdev hostfwd is needed
        # for this guest-initiates-the-connection direction.
        NET_ARGS=(-netdev user,id=net0 -device virtio-net-pci,netdev=net0)
        ;;
    none)
        # Deliberately no NIC at all — exercises /init's "no eth0
        # present" branch and confirms the agent's upload failure is
        # handled gracefully end to end (see bootdecision.rs).
        NET_ARGS=(-net none)
        ;;
    *)
        echo "ERROR: PLDDS_QEMU_NET must be 'user' or 'none', got '${NET_MODE}'" >&2
        exit 1
        ;;
esac

# -nographic + console=ttyS0: serial-only, works headless / in CI.
# -no-reboot: if the diagnostic env calls reboot, QEMU exits instead of
#   looping, which is what we want for an automated smoke test.
# No -drive / -hda / -hdb at all: there is no virtual disk to damage.
QEMU_ARGS=(
    -kernel "${KERNEL}"
    -initrd "${INITRAMFS}"
    -append "console=ttyS0 panic=1"
    -nographic
    -no-reboot
    -m 512M
    "${NET_ARGS[@]}"
)

run_qemu() {
    local kvm_flag=("$@")
    if [ -n "${LOG_FILE}" ]; then
        # `script`-free tee: QEMU writes the serial console straight to
        # stdout in -nographic mode, so a plain tee captures it.
        timeout --preserve-status "${TIMEOUT_SECS}" \
            qemu-system-x86_64 "${QEMU_ARGS[@]}" "${kvm_flag[@]}" | tee "${LOG_FILE}"
    else
        timeout --preserve-status "${TIMEOUT_SECS}" \
            qemu-system-x86_64 "${QEMU_ARGS[@]}" "${kvm_flag[@]}"
    fi
}

# Prefer KVM (fast); fall back to pure emulation (slow but works
# anywhere, including CI runners and sandboxes with no /dev/kvm — see
# docs/qemu-testing.md "Environments without KVM").
if [ -e /dev/kvm ] && [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
    run_qemu -enable-kvm
else
    echo "[run-qemu] /dev/kvm not available — running without hardware acceleration (slower)"
    run_qemu
fi
