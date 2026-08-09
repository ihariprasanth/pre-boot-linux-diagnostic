#!/bin/bash
# Build the PLDDS diagnostic initramfs.
#
# Produces: diagnostic/build/output/initramfs.cpio.gz
#
# Requirements on the build host:
#   - busybox-static (Ubuntu/Debian: `apt-get install busybox-static`)
#   - cpio, gzip (present on virtually any Linux host)
#
# This script only ever writes inside diagnostic/build/output/ and a
# temporary staging directory. It never touches any block device.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIAG_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUT_DIR="${SCRIPT_DIR}/output"
STAGE_DIR="$(mktemp -d)"

BUSYBOX_BIN="${BUSYBOX_BIN:-$(command -v busybox || true)}"

cleanup() {
    rm -rf "${STAGE_DIR}"
}
trap cleanup EXIT

if [ -z "${BUSYBOX_BIN}" ] || [ ! -x "${BUSYBOX_BIN}" ]; then
    echo "ERROR: busybox binary not found. Install busybox-static or set BUSYBOX_BIN=/path/to/busybox" >&2
    exit 1
fi

echo "[build-initramfs] staging dir: ${STAGE_DIR}"
echo "[build-initramfs] busybox:     ${BUSYBOX_BIN}"

# --- Directory skeleton ----------------------------------------------------
mkdir -p "${STAGE_DIR}"/{bin,sbin,proc,sys,dev,etc,tmp,root,usr/bin,usr/sbin}

# --- Install busybox and generate applet symlinks --------------------------
cp "${BUSYBOX_BIN}" "${STAGE_DIR}/bin/busybox"
chmod +x "${STAGE_DIR}/bin/busybox"

# Ask busybox itself for the list of applets it was built with, then
# symlink each one to the busybox binary (the standard multi-call pattern).
APPLETS="$("${STAGE_DIR}/bin/busybox" --list)"
for applet in ${APPLETS}; do
    # Skip "busybox" itself — symlinking it over the real binary would
    # replace the multi-call binary with a symlink pointing to nothing.
    if [ "${applet}" = "busybox" ]; then
        continue
    fi
    ln -sf busybox "${STAGE_DIR}/bin/${applet}"
done

# --- Install our init script -----------------------------------------------
install -m 0755 "${DIAG_DIR}/initramfs/init" "${STAGE_DIR}/init"

# Phase 10: init's network bring-up needs these two applets. Most
# busybox-static builds include both, but warn loudly rather than fail
# silently at boot if this particular build doesn't — the network
# bring-up in /init already degrades gracefully either way.
for needed in ip udhcpc; do
    if ! echo "${APPLETS}" | grep -qw "${needed}"; then
        echo "[build-initramfs] WARNING: busybox build has no '${needed}' applet." >&2
        echo "[build-initramfs] Network bring-up in /init will silently skip; agent upload will always fail." >&2
    fi
done

# --- Install the diagnostic agent, if it has been built --------------------
# MUST be statically linked: the initramfs has no dynamic linker or libc.
# Build with:
#   RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu
AGENT_BIN_STATIC="${DIAG_DIR}/agent/target/x86_64-unknown-linux-gnu/release/diagnostic-agent"
AGENT_BIN_DYNAMIC="${DIAG_DIR}/agent/target/release/diagnostic-agent"

if [ -x "${AGENT_BIN_STATIC}" ]; then
    install -m 0755 "${AGENT_BIN_STATIC}" "${STAGE_DIR}/bin/diagnostic-agent"
    echo "[build-initramfs] included statically-linked diagnostic-agent ($(du -h "${AGENT_BIN_STATIC}" | cut -f1))"
elif [ -x "${AGENT_BIN_DYNAMIC}" ]; then
    echo "[build-initramfs] WARNING: only a dynamically-linked agent binary was found." >&2
    echo "[build-initramfs] It will NOT run in the initramfs (no dynamic linker present)." >&2
    echo "[build-initramfs] Rebuild with: RUSTFLAGS=\"-C target-feature=+crt-static\" cargo build --release --target x86_64-unknown-linux-gnu" >&2
    install -m 0755 "${AGENT_BIN_DYNAMIC}" "${STAGE_DIR}/bin/diagnostic-agent"
else
    echo "[build-initramfs] diagnostic-agent not built yet — see diagnostic/agent/README.md"
fi

# --- Minimal /etc so busybox tools that expect it don't choke --------------
echo "root:x:0:0:root:/root:/bin/sh" > "${STAGE_DIR}/etc/passwd"
echo "root:x:0:" > "${STAGE_DIR}/etc/group"
echo "pldds-diagnostic" > "${STAGE_DIR}/etc/hostname"

# --- udhcpc handler script (Phase 10) ---------------------------------
# init calls `udhcpc -s /etc/udhcpc.script`; without a handler script
# udhcpc prints lease info but never actually configures the interface.
install -m 0755 "${DIAG_DIR}/initramfs/scripts/udhcpc.script" "${STAGE_DIR}/etc/udhcpc.script"

# --- Package as gzip-compressed cpio (newc format, what the kernel expects) -
mkdir -p "${OUT_DIR}"
( cd "${STAGE_DIR}" && find . -print0 | cpio --null -ov --format=newc 2>/dev/null ) \
    | gzip -9 > "${OUT_DIR}/initramfs.cpio.gz"

SIZE="$(du -h "${OUT_DIR}/initramfs.cpio.gz" | cut -f1)"
echo "[build-initramfs] wrote ${OUT_DIR}/initramfs.cpio.gz (${SIZE})"
