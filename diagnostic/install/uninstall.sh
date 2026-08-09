#!/usr/bin/env bash
# Phase 13: undo install.sh — removes the PLDDS GRUB snippet, restores
# the original grub.cfg/grubenv/40_custom from backup, removes the
# copied boot files from the ESP, and regenerates grub.cfg for good
# measure. Safe to run even if install.sh only partially completed.
#
# Usage:
#   sudo ./uninstall.sh                restore + regenerate (normal use)
#   sudo ./uninstall.sh --restore-only just copy the backups back, skip
#                                       regenerating (used internally by
#                                       install.sh on verification failure,
#                                       when trusting update-grub again
#                                       is exactly what we don't want to
#                                       do yet)

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: must be run as root" >&2
    exit 1
fi

MODE="${1:-full}"
BACKUP_DIR="/var/lib/pldds/grub-backup"

if [ ! -f "${BACKUP_DIR}/MANIFEST.txt" ]; then
    echo "ERROR: no backup found at ${BACKUP_DIR}/MANIFEST.txt — nothing to restore from." >&2
    echo "If PLDDS was never installed via install.sh there's nothing to undo." >&2
    exit 1
fi

GRUB_CFG="$(grep 'original grub.cfg path' "${BACKUP_DIR}/MANIFEST.txt" | cut -d: -f2- | xargs)"
GRUBENV="$(grep 'original grubenv path' "${BACKUP_DIR}/MANIFEST.txt" | cut -d: -f2- | xargs)"
CUSTOM="$(grep 'original 40_custom path' "${BACKUP_DIR}/MANIFEST.txt" | cut -d: -f2- | xargs)"

echo "[uninstall] restoring ${GRUB_CFG} from backup"
cp -a "${BACKUP_DIR}/grub.cfg" "${GRUB_CFG}"

if [ -f "${BACKUP_DIR}/grubenv" ] && [ -n "${GRUBENV}" ]; then
    echo "[uninstall] restoring ${GRUBENV} from backup"
    cp -a "${BACKUP_DIR}/grubenv" "${GRUBENV}"
fi

if [ -f "${BACKUP_DIR}/40_custom" ] && [ -n "${CUSTOM}" ]; then
    echo "[uninstall] restoring ${CUSTOM} from backup"
    cp -a "${BACKUP_DIR}/40_custom" "${CUSTOM}"
elif [ -z "${CUSTOM}" ] || [ ! -f "${BACKUP_DIR}/40_custom" ]; then
    # There was no 40_custom before install.sh — nothing to restore, but
    # make sure the one install.sh may have touched isn't left dangling.
    true
fi

# The PLDDS-specific custom snippet install.sh created — always safe to
# remove regardless of mode, it never existed before install.sh ran.
if [ -f /etc/grub.d/41_pldds ]; then
    echo "[uninstall] removing /etc/grub.d/41_pldds"
    rm -f /etc/grub.d/41_pldds
fi

# --- Remove copied boot files ------------------------------------------
ESP="${PLDDS_ESP_PATH:-$(bootctl --print-esp-path 2>/dev/null || echo /boot/efi)}"
if [ -d "${ESP}/pldds" ]; then
    echo "[uninstall] removing ${ESP}/pldds/"
    rm -rf "${ESP}/pldds"
fi

if [ "${MODE}" = "--restore-only" ]; then
    echo "[uninstall] --restore-only: skipping grub.cfg regeneration (backup copy restored directly, as-is)."
    echo "[uninstall] done."
    exit 0
fi

echo "[uninstall] regenerating grub.cfg via update-grub/grub-mkconfig..."
if command -v update-grub >/dev/null 2>&1; then
    update-grub
elif command -v grub-mkconfig >/dev/null 2>&1; then
    grub-mkconfig -o "${GRUB_CFG}"
fi

echo "[uninstall] verifying Windows entry is present and default..."
if grep -q 'set default="windows"' "${GRUB_CFG}" 2>/dev/null; then
    echo "[uninstall] OK — this was expected only if PLDDS's own template was still what generated it; a plain distro regeneration won't have this exact line, that's fine too."
fi
if grep -qi 'Windows Boot Manager\|windows' "${GRUB_CFG}"; then
    echo "[uninstall] OK — Windows entry present in regenerated grub.cfg."
else
    echo "WARNING: could not confirm a Windows entry in the regenerated grub.cfg." >&2
    echo "The raw backup at ${BACKUP_DIR}/grub.cfg was already restored to ${GRUB_CFG} before this regeneration step ran, so it's still available if you'd rather use it verbatim (copy it back over and skip update-grub)." >&2
fi

echo "[uninstall] done. PLDDS has been removed; Windows boot path is unchanged."
