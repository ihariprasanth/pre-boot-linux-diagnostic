#!/usr/bin/env bash
# Build the PLDDS diagnostic kernel (bzImage) from a pinned upstream
# kernel.org tarball, using tinyconfig + our fragment
# (diagnostic/kernel/config/diagnostic.config) merged on top.
#
# Produces: diagnostic/build/output/vmlinuz
#
# Requires on the build host:
#   build-essential flex bison libssl-dev libelf-dev bc rsync curl
#
# This script only writes inside diagnostic/build/ (a cached source
# checkout under diagnostic/build/.kernel-src/) and diagnostic/build/output/.
# It never touches any block device.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIAG_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUT_DIR="${SCRIPT_DIR}/output"
SRC_CACHE="${SCRIPT_DIR}/.kernel-src"
CONFIG_FRAGMENT="${DIAG_DIR}/kernel/config/diagnostic.config"

# Pin an exact, reproducible kernel version. Bump deliberately, not
# by chasing "latest" — this is the version every ISO build is tested
# against.
KERNEL_VERSION="${PLDDS_KERNEL_VERSION:-6.6.32}"
KERNEL_MAJOR="$(echo "${KERNEL_VERSION}" | cut -d. -f1)"
KERNEL_URL="https://cdn.kernel.org/pub/linux/kernel/v${KERNEL_MAJOR}.x/linux-${KERNEL_VERSION}.tar.xz"
KERNEL_DIR="${SRC_CACHE}/linux-${KERNEL_VERSION}"

JOBS="${JOBS:-$(nproc 2>/dev/null || echo 2)}"

echo "[build-kernel] target version: ${KERNEL_VERSION}"

if ! command -v make >/dev/null 2>&1; then
    echo "ERROR: 'make' not found. Install build-essential flex bison libssl-dev libelf-dev bc" >&2
    exit 1
fi

if [ ! -f "${CONFIG_FRAGMENT}" ]; then
    echo "ERROR: missing config fragment: ${CONFIG_FRAGMENT}" >&2
    exit 1
fi

mkdir -p "${SRC_CACHE}" "${OUT_DIR}"

if [ ! -d "${KERNEL_DIR}" ]; then
    echo "[build-kernel] fetching linux-${KERNEL_VERSION} source..."
    curl -fL --retry 3 -o "${SRC_CACHE}/linux-${KERNEL_VERSION}.tar.xz" "${KERNEL_URL}"
    tar -xf "${SRC_CACHE}/linux-${KERNEL_VERSION}.tar.xz" -C "${SRC_CACHE}"
else
    echo "[build-kernel] using cached source at ${KERNEL_DIR}"
fi

cd "${KERNEL_DIR}"

echo "[build-kernel] starting from tinyconfig (minimal baseline)..."
make tinyconfig >/dev/null

echo "[build-kernel] merging diagnostic config fragment..."
scripts/kconfig/merge_config.sh -m .config "${CONFIG_FRAGMENT}" >/dev/null

# Resolve any new dependent symbols non-interactively (take defaults).
make olddefconfig >/dev/null

echo "[build-kernel] building bzImage with ${JOBS} jobs (this takes a while)..."
make -j"${JOBS}" bzImage

BZIMAGE_PATH="arch/x86/boot/bzImage"
if [ ! -f "${BZIMAGE_PATH}" ]; then
    echo "ERROR: build finished but ${BZIMAGE_PATH} not found." >&2
    exit 1
fi

cp "${BZIMAGE_PATH}" "${OUT_DIR}/vmlinuz"
echo "[build-kernel] wrote ${OUT_DIR}/vmlinuz ($(du -h "${OUT_DIR}/vmlinuz" | cut -f1))"
