#!/usr/bin/env bash
# Phase 13: snapshot the machine's GRUB state before install.sh touches
# anything. Called automatically by install.sh and by make-recovery-usb.sh
# if no backup exists yet — safe to run manually any time too.
#
# Writes to /var/lib/pldds/grub-backup/ :
#   grub.cfg        — verbatim copy of the generated config
#   grubenv         — verbatim copy of the GRUB environment block
#   40_custom       — verbatim copy (if present) — install.sh appends here
#   MANIFEST.txt    — where each file came from + when, so a restore
#                     doesn't depend on remembering paths correctly

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: must be run as root" >&2
    exit 1
fi

BACKUP_DIR="/var/lib/pldds/grub-backup"
mkdir -p "${BACKUP_DIR}"

# Locate the real grub.cfg — path differs by distro (Debian/Ubuntu:
# /boot/grub/grub.cfg, Fedora/RHEL: /boot/grub2/grub.cfg or the ESP
# itself on some UEFI setups).
CANDIDATES=(
    /boot/grub/grub.cfg
    /boot/grub2/grub.cfg
    /boot/efi/EFI/*/grub.cfg
)
GRUB_CFG=""
for c in "${CANDIDATES[@]}"; do
    for f in $c; do
        if [ -f "$f" ]; then
            GRUB_CFG="$f"
            break 2
        fi
    done
done

if [ -z "${GRUB_CFG}" ]; then
    echo "ERROR: could not locate an existing grub.cfg (checked: ${CANDIDATES[*]})" >&2
    echo "Set GRUB_CFG_PATH=/path/to/grub.cfg and re-run if yours is somewhere else." >&2
    exit 1
fi
GRUB_CFG="${GRUB_CFG_PATH:-${GRUB_CFG}}"

TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

cp -a "${GRUB_CFG}" "${BACKUP_DIR}/grub.cfg"
echo "[backup-grub-config] backed up ${GRUB_CFG} -> ${BACKUP_DIR}/grub.cfg"

GRUBENV="$(dirname "${GRUB_CFG}")/grubenv"
if [ -f "${GRUBENV}" ]; then
    cp -a "${GRUBENV}" "${BACKUP_DIR}/grubenv"
    echo "[backup-grub-config] backed up ${GRUBENV} -> ${BACKUP_DIR}/grubenv"
fi

CUSTOM=/etc/grub.d/40_custom
if [ -f "${CUSTOM}" ]; then
    cp -a "${CUSTOM}" "${BACKUP_DIR}/40_custom"
    echo "[backup-grub-config] backed up ${CUSTOM} -> ${BACKUP_DIR}/40_custom"
fi

cat > "${BACKUP_DIR}/MANIFEST.txt" <<EOF
PLDDS GRUB backup
captured: ${TIMESTAMP}
original grub.cfg path: ${GRUB_CFG}
original grubenv path:  ${GRUBENV}
original 40_custom path: ${CUSTOM}

To restore by hand:
  sudo cp ${BACKUP_DIR}/grub.cfg ${GRUB_CFG}
  sudo cp ${BACKUP_DIR}/grubenv ${GRUBENV}      # if it exists above
  sudo cp ${BACKUP_DIR}/40_custom ${CUSTOM}     # if it exists above

Or just run: sudo diagnostic/install/uninstall.sh
EOF

echo "[backup-grub-config] manifest written to ${BACKUP_DIR}/MANIFEST.txt"
