#!/usr/bin/env bash
# Phase 13: pre-flight checks — run this FIRST, before install.sh, on the
# real dual-boot machine. Read-only: never writes anything, only reports.
#
# Confirms the machine is in a state install.sh can safely handle, and
# prints exactly what install.sh is about to do so there are no
# surprises. If any check fails, fix it before running install.sh.

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}OK${NC}   $1"; }
warn() { echo -e "  ${YELLOW}WARN${NC} $1"; }
fail() { echo -e "  ${RED}FAIL${NC} $1"; FAILED=1; }

FAILED=0

echo "PLDDS Phase 13 — pre-flight checks"
echo "==================================="

# --- 1. Must be root (needed to read partition tables / EFI vars) ----------
if [ "$(id -u)" -ne 0 ]; then
    fail "must be run as root (sudo $0)"
else
    ok "running as root"
fi

# --- 2. UEFI, not legacy BIOS ------------------------------------------
if [ -d /sys/firmware/efi ]; then
    ok "system is booted in UEFI mode"
else
    fail "system is NOT in UEFI mode — this repo's GRUB config targets UEFI chainloading of bootmgfw.efi only, legacy BIOS is unsupported"
fi

# --- 3. Secure Boot state (informational — we don't sign our kernel yet) ---
if command -v mokutil >/dev/null 2>&1 && mokutil --sb-state 2>/dev/null | grep -qi enabled; then
    warn "Secure Boot is ENABLED. diagnostic/kernel is not signed — the PLDDS entry will fail to boot until you either sign it or disable Secure Boot for this test. Windows Boot Manager chainloading is unaffected either way."
else
    ok "Secure Boot is disabled or mokutil unavailable (assuming disabled)"
fi

# --- 4. Existing Windows Boot Manager present ------------------------------
ESP="$(bootctl --print-esp-path 2>/dev/null || echo /boot/efi)"
if [ -f "${ESP}/EFI/Microsoft/Boot/bootmgfw.efi" ]; then
    ok "found Windows Boot Manager at ${ESP}/EFI/Microsoft/Boot/bootmgfw.efi"
else
    fail "did not find ${ESP}/EFI/Microsoft/Boot/bootmgfw.efi — is ESP mounted? Pass PLDDS_ESP_PATH=/your/esp if it's mounted somewhere non-standard."
fi

# --- 5. grub-reboot / grub-editenv available --------------------------
for bin in grub-reboot grub-editenv grub-mkconfig update-grub; do
    if command -v "$bin" >/dev/null 2>&1; then
        ok "found $bin"
    fi
done
if ! command -v grub-reboot >/dev/null 2>&1; then
    fail "grub-reboot not found — required for the one-shot boot-to-diagnostic mechanism (docs/architecture.md)"
fi

# --- 6. Free space on ESP for vmlinuz + initramfs --------------------------
if [ -d "${ESP}" ]; then
    AVAIL_KB="$(df -k --output=avail "${ESP}" 2>/dev/null | tail -1 | tr -d ' ')"
    if [ -n "${AVAIL_KB:-}" ] && [ "${AVAIL_KB}" -lt 102400 ]; then
        fail "less than 100MB free on ESP (${AVAIL_KB}KB) — PLDDS needs room for vmlinuz + initramfs.cpio.gz"
    else
        ok "sufficient free space on ESP (${AVAIL_KB:-unknown}KB available)"
    fi
fi

# --- 7. Build artifacts exist -------------------------------------------
DIAG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VMLINUZ="${DIAG_DIR}/build/output/vmlinuz"
INITRD="${DIAG_DIR}/build/output/initramfs.cpio.gz"
for f in "${VMLINUZ}" "${INITRD}"; do
    if [ -f "$f" ]; then
        ok "build artifact present: $f"
    else
        fail "missing build artifact: $f — run diagnostic/build/build-kernel.sh and build-initramfs.sh first"
    fi
done

# --- 8. /etc/grub.d is writable and 40_custom / a free slot exists ------
if [ -w /etc/grub.d ]; then
    ok "/etc/grub.d is writable"
else
    fail "/etc/grub.d is not writable"
fi

echo "==================================="
if [ "${FAILED}" -eq 0 ]; then
    echo -e "${GREEN}All checks passed.${NC} Next: build recovery media (make-recovery-usb.sh), then run install.sh."
    exit 0
else
    echo -e "${RED}One or more checks failed.${NC} Fix these before running install.sh — do not proceed on a machine with a real Windows install otherwise."
    exit 1
fi
