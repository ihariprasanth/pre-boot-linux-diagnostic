#!/usr/bin/env bash
# Build a bootable PLDDS diagnostic ISO — a hybrid BIOS+UEFI ISO
# (via grub-mkrescue) that boots straight into the diagnostic
# environment. This is NOT the same artifact diagnostic/install/install.sh
# writes to a real machine's ESP (that's the "add PLDDS alongside your
# already-installed Windows" path, driven by the host's own GRUB) — this
# ISO is a standalone bootable image, useful for:
#
#   - burning to a USB stick to test the diagnostic environment on real
#     hardware WITHOUT installing anything (boot from USB, run once,
#     reboot — nothing on the internal disk is touched)
#   - booting directly in QEMU with `-cdrom` instead of the raw
#     `-kernel`/`-initrd` flags scripts/run-qemu.sh uses (closer to how
#     a real firmware boots removable media, catches GRUB-menu-level
#     issues the direct-kernel-boot QEMU path can't)
#   - handing someone a single file to try PLDDS without walking them
#     through diagnostic/install/ at all
#
# Produces: diagnostic/build/output/pldds.iso
#
# Requires on the build host: grub-mkrescue + xorriso (Debian/Ubuntu:
# `apt-get install grub-pc-bin grub-efi-amd64-bin xorriso mtools`).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIAG_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUT_DIR="${SCRIPT_DIR}/output"
VMLINUZ="${OUT_DIR}/vmlinuz"
INITRD="${OUT_DIR}/initramfs.cpio.gz"
ISO_OUT="${OUT_DIR}/pldds.iso"

STAGE="$(mktemp -d)"
trap 'rm -rf "${STAGE}"' EXIT

echo "[build-iso] checking dependencies..."
if ! command -v grub-mkrescue >/dev/null 2>&1; then
    echo "ERROR: grub-mkrescue not found." >&2
    echo "Debian/Ubuntu: apt-get install grub-pc-bin grub-efi-amd64-bin xorriso mtools" >&2
    exit 1
fi
if ! command -v xorriso >/dev/null 2>&1; then
    echo "ERROR: xorriso not found (grub-mkrescue needs it)." >&2
    echo "Debian/Ubuntu: apt-get install xorriso" >&2
    exit 1
fi

echo "[build-iso] checking build artifacts..."
if [ ! -f "${VMLINUZ}" ]; then
    echo "ERROR: ${VMLINUZ} not found." >&2
    echo "Run diagnostic/build/build-kernel.sh first, OR for a quick" >&2
    echo "smoke-test ISO, copy a host kernel in: cp /boot/vmlinuz-\$(uname -r) ${VMLINUZ}" >&2
    echo "(see docs/qemu-testing.md \"A note on the kernel used\" — same" >&2
    echo "pre-Phase-13-kernel caveat applies here until build-kernel.sh" >&2
    echo "produces our own minimal kernel image)." >&2
    exit 1
fi
if [ ! -f "${INITRD}" ]; then
    echo "ERROR: ${INITRD} not found. Run diagnostic/build/build-initramfs.sh first." >&2
    exit 1
fi

# --- Stage the ISO's GRUB tree ------------------------------------------
mkdir -p "${STAGE}/boot/grub"
cp "${VMLINUZ}" "${STAGE}/boot/vmlinuz"
cp "${INITRD}" "${STAGE}/boot/initramfs.cpio.gz"

# A self-contained grub.cfg — deliberately NOT diagnostic/grub/grub.cfg
# (that one targets install.sh's alongside-Windows layout and references
# ${PLDDS_BOOT_PARTITION_UUID}/${EFI_SYSTEM_PARTITION_UUID} placeholders
# that only make sense on a real installed disk). This ISO boots itself
# directly — no UUID search needed, GRUB already knows it's reading from
# the disc/USB it was loaded from.
cat > "${STAGE}/boot/grub/grub.cfg" <<'EOF'
# PLDDS standalone diagnostic ISO — grub-mkrescue-generated.
# No Windows entry here: this ISO is a standalone boot-and-test image,
# not the alongside-Windows GRUB config (see diagnostic/grub/grub.cfg
# and diagnostic/install/ for that path).
set timeout=5
set timeout_style=menu
set default="diagnostic"

menuentry "PLDDS Diagnostic Linux" --id diagnostic {
    linux  /boot/vmlinuz console=ttyS0 console=tty0 quiet
    initrd /boot/initramfs.cpio.gz
}

menuentry "PLDDS Diagnostic Linux (safe mode)" --id diagnostic-safe {
    linux  /boot/vmlinuz console=ttyS0 console=tty0 pldds.safe=1
    initrd /boot/initramfs.cpio.gz
}
EOF

echo "[build-iso] building hybrid BIOS+UEFI ISO..."
mkdir -p "${OUT_DIR}"
grub-mkrescue -o "${ISO_OUT}" "${STAGE}" \
    -- -volid PLDDS_DIAG 2>&1 | grep -v '^xorriso :' || true

if [ ! -f "${ISO_OUT}" ]; then
    echo "ERROR: grub-mkrescue did not produce ${ISO_OUT}" >&2
    exit 1
fi

SIZE="$(du -h "${ISO_OUT}" | cut -f1)"
echo "[build-iso] wrote ${ISO_OUT} (${SIZE})"
echo
echo "Test it:"
echo "  QEMU (BIOS):  qemu-system-x86_64 -cdrom ${ISO_OUT} -m 512M -nographic"
echo "  QEMU (UEFI):  qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd -cdrom ${ISO_OUT} -m 512M -nographic"
echo "  Real USB:     sudo dd if=${ISO_OUT} of=/dev/sdX bs=4M status=progress conv=fsync   (erases /dev/sdX)"
echo
echo "This ISO is standalone (boots directly into PLDDS, no Windows entry)."
echo "For the alongside-Windows install on a real dual-boot machine, use"
echo "diagnostic/install/install.sh instead - see docs/real-hardware-testing.md."
