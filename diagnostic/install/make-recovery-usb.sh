#!/usr/bin/env bash
# Phase 13: build recovery media. Run this and VERIFY it boots on a spare
# machine (or in QEMU with -drive if=none) BEFORE running install.sh on
# real hardware. The whole point of Phase 13's safety story is: if
# install.sh or GRUB itself goes wrong, you can boot this USB and get
# Windows working again without needing the broken machine's own GRUB.
#
# What it puts on the USB:
#   1. A standalone GRUB EFI binary (grub-mkstandalone) that can chainload
#      Windows directly, independent of whatever's on the internal disk's
#      ESP — this is the actual "recovery" part.
#   2. A copy of the machine's ORIGINAL grub.cfg (captured by
#      backup-grub-config.sh, run automatically here if not already done)
#      so you have a byte-for-byte restore point, not just a description.
#   3. This repo's docs (architecture.md, boot-flow.md,
#      real-hardware-testing.md) so the recovery steps are on the USB
#      itself, not only on a machine you may not be able to boot.
#
# Usage:
#   sudo ./make-recovery-usb.sh /dev/sdX
#
# /dev/sdX must be an otherwise-empty USB drive — THIS SCRIPT WILL
# REPARTITION AND FORMAT IT. It refuses to run against anything that
# looks like an internal disk (see the safety check below) but you are
# still responsible for pointing it at the right device.

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: must be run as root" >&2
    exit 1
fi

DEVICE="${1:-}"
if [ -z "${DEVICE}" ]; then
    echo "Usage: sudo $0 /dev/sdX   (a USB drive — will be ERASED)" >&2
    echo "Available removable block devices:" >&2
    lsblk -d -o NAME,SIZE,TYPE,RM,TRAN 2>/dev/null | awk '$4==1' >&2 || true
    exit 1
fi

# --- Safety: refuse anything that isn't clearly removable -----------------
DEV_NAME="$(basename "${DEVICE}")"
RM_FLAG="$(cat "/sys/block/${DEV_NAME}/removable" 2>/dev/null || echo 0)"
if [ "${RM_FLAG}" != "1" ]; then
    echo "ERROR: ${DEVICE} is not reported as removable. Refusing to touch it." >&2
    echo "This almost certainly means you pointed this script at an internal disk." >&2
    exit 1
fi

read -r -p "This will ERASE ALL DATA on ${DEVICE}. Type the device path again to confirm: " CONFIRM
if [ "${CONFIRM}" != "${DEVICE}" ]; then
    echo "Confirmation did not match — aborting, nothing was touched." >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIAG_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${DIAG_DIR}/.." && pwd)"
BACKUP_DIR="/var/lib/pldds/grub-backup"
STAGE="$(mktemp -d)"
trap 'rm -rf "${STAGE}"' EXIT

# --- Make sure we have a grub.cfg backup to put on the USB ------------
if [ ! -f "${BACKUP_DIR}/grub.cfg" ]; then
    echo "[make-recovery-usb] no existing grub.cfg backup found — capturing one now"
    "${SCRIPT_DIR}/backup-grub-config.sh"
fi

# --- Partition + format the USB (single FAT32 ESP-style partition) --------
echo "[make-recovery-usb] partitioning ${DEVICE}"
parted -s "${DEVICE}" mklabel gpt
parted -s "${DEVICE}" mkpart ESP fat32 1MiB 100%
parted -s "${DEVICE}" set 1 esp on
sleep 1
PART="${DEVICE}1"
[ -b "${PART}" ] || PART="${DEVICE}p1"
mkfs.fat -F32 -n PLDDS_RESCUE "${PART}"

MNT="$(mktemp -d)"
mount "${PART}" "${MNT}"
trap 'umount "${MNT}" 2>/dev/null || true; rmdir "${MNT}" 2>/dev/null || true; rm -rf "${STAGE}"' EXIT

# --- Build a standalone GRUB EFI binary that can find + chainload Windows -
mkdir -p "${STAGE}/grub-embed"
cat > "${STAGE}/grub-embed/rescue.cfg" <<'EOF'
# Embedded into the standalone rescue GRUB binary. No dependency on any
# on-disk grub.cfg — this is the whole point of recovery media.
set timeout=-1
insmod part_gpt
insmod fat
insmod chain
insmod search_fs_uuid

echo "PLDDS recovery media — searching for Windows Boot Manager..."
for uuid_hint in 1 2 3 4 5 6 7 8; do
    true
done

# Try every FAT partition on every disk looking for bootmgfw.efi, in case
# the internal disk's own ESP UUID changed or its GRUB is broken.
insmod part_msdos
search --no-floppy --file --set=root /EFI/Microsoft/Boot/bootmgfw.efi
if [ -n "$root" ]; then
    chainloader /EFI/Microsoft/Boot/bootmgfw.efi
    boot
fi

echo "Could not auto-locate bootmgfw.efi. Dropping to GRUB rescue shell —"
echo "try: ls, then 'set root=(hdX,gptY)' + chainloader manually."
EOF

if command -v grub-mkstandalone >/dev/null 2>&1; then
    mkdir -p "${MNT}/EFI/Boot"
    grub-mkstandalone \
        --format=x86_64-efi \
        --output="${MNT}/EFI/Boot/bootx64.efi" \
        --locales="" \
        --fonts="" \
        "boot/grub/grub.cfg=${STAGE}/grub-embed/rescue.cfg"
    echo "[make-recovery-usb] wrote standalone rescue bootloader to EFI/Boot/bootx64.efi (default UEFI removable-media path — boots without needing an NVRAM entry)"
else
    echo "WARNING: grub-mkstandalone not found (package grub-efi-amd64-bin on Debian/Ubuntu)." >&2
    echo "Falling back to copying rescue.cfg only — you'll need to load it manually from a live GRUB shell." >&2
    mkdir -p "${MNT}/pldds-rescue"
    cp "${STAGE}/grub-embed/rescue.cfg" "${MNT}/pldds-rescue/rescue.cfg"
fi

# --- Copy the real grub.cfg backup + repo docs -----------------------------
mkdir -p "${MNT}/pldds-rescue/grub-backup" "${MNT}/pldds-rescue/docs"
cp -a "${BACKUP_DIR}/." "${MNT}/pldds-rescue/grub-backup/"
cp "${REPO_ROOT}/docs/architecture.md" "${MNT}/pldds-rescue/docs/" 2>/dev/null || true
cp "${REPO_ROOT}/docs/boot-flow.md" "${MNT}/pldds-rescue/docs/" 2>/dev/null || true
cp "${REPO_ROOT}/docs/real-hardware-testing.md" "${MNT}/pldds-rescue/docs/" 2>/dev/null || true
cp "${SCRIPT_DIR}/uninstall.sh" "${MNT}/pldds-rescue/" 2>/dev/null || true

cat > "${MNT}/pldds-rescue/README.txt" <<'EOF'
PLDDS recovery media
=====================

If you're reading this because the machine won't boot Windows:

1. Boot from this USB (UEFI boot menu, usually F12/F10/Esc at power-on;
   pick the USB drive — it should auto-chainload Windows Boot Manager).

2. If it doesn't auto-boot Windows, you're at a GRUB rescue prompt. Try:
     ls
   to list disks/partitions, find the one with EFI/Microsoft/Boot/, then:
     set root=(hdX,gptY)
     chainloader /EFI/Microsoft/Boot/bootmgfw.efi
     boot

3. To fully restore the internal disk's own GRUB to how it was before
   PLDDS was installed: boot a normal Linux environment (this USB is
   boot-only, not a full install), mount the internal disk, and either
   run pldds-rescue/uninstall.sh against it, or manually copy
   pldds-rescue/grub-backup/grub.cfg back over /boot/grub/grub.cfg (or
   wherever it lives — see grub-backup/MANIFEST.txt for the original
   path it was captured from).

See pldds-rescue/docs/real-hardware-testing.md for the full Phase 13
runbook this media was built as part of.
EOF

sync
umount "${MNT}"
rmdir "${MNT}"
trap - EXIT
rm -rf "${STAGE}"

echo "[make-recovery-usb] done. VERIFY THIS BOOTS before relying on it:"
echo "  - Test in QEMU: qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd -drive if=none,id=usb,file=${DEVICE},format=raw -device usb-storage,drive=usb"
echo "  - Or boot it on a spare machine's UEFI boot menu and confirm it reaches Windows."
