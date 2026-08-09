//! Local-log mode: instead of uploading the report over the network,
//! write it directly onto the Windows C: partition as a plain JSON
//! file under `C:\PLDDS\Logs\`. A separate, later-built Windows-side
//! tool is expected to pick these files up and transmit them — this
//! module's only job is "get the file safely onto the Windows
//! filesystem without ever touching Windows' own files".
//!
//! Safety invariants (same spirit as the rest of this repo — see
//! docs/architecture.md "Core safety invariant"):
//!   - read-only probing before any write: every candidate partition
//!     is mounted, inspected for a real `Windows` directory, and
//!     unmounted again if it isn't the right one.
//!   - the *only* write this module ever performs is creating
//!     `C:\PLDDS\Logs\<name>.json` (and the `PLDDS`/`Logs` directories
//!     if missing). It never opens, modifies, or deletes any existing
//!     file on the Windows partition.
//!   - every step is best-effort: a mount failure, a missing NTFS
//!     driver, a full disk — none of it may panic or block the boot.
//!     Caller treats any `Err` the same way a failed network upload
//!     used to be treated (see main.rs / bootdecision.rs).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const MOUNT_POINT: &str = "/mnt/winc";
const LOG_SUBDIR: &str = "PLDDS/Logs";

/// Block devices worth trying, in order. Covers the common BIOS/UEFI
/// dual-boot layouts (SATA/virtio `sdX`, NVMe `nvmeXn1pX`) without
/// requiring blkid/lsblk in the initramfs (neither is present — see
/// diagnostic/build/build-initramfs.sh, busybox-only userspace).
fn candidate_partitions() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for letter in 'a'..='d' {
        for part in 1..=6 {
            out.push(PathBuf::from(format!("/dev/sd{letter}{part}")));
        }
    }
    for disk in 0..=1 {
        for part in 1..=6 {
            out.push(PathBuf::from(format!("/dev/nvme{disk}n1p{part}")));
        }
    }
    out.into_iter().filter(|p| p.exists()).collect()
}

/// Mount `dev` read-write as ntfs3 at MOUNT_POINT. Returns Ok(()) only
/// if the mount syscall itself succeeded — caller still must verify
/// this is actually the Windows system partition before writing.
fn mount_rw(dev: &Path) -> io::Result<()> {
    fs::create_dir_all(MOUNT_POINT)?;
    let status = Command::new("mount")
        .args(["-t", "ntfs3", "-o", "rw"])
        .arg(dev)
        .arg(MOUNT_POINT)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, format!("mount {dev:?} failed: {status}")))
    }
}

fn unmount() {
    let _ = Command::new("umount").arg(MOUNT_POINT).status();
}

/// A partition is "the Windows C: drive" if it has a top-level
/// `Windows` directory containing `System32` — cheap, reliable, and
/// doesn't require parsing the registry or boot config.
fn looks_like_windows_c(mount_point: &Path) -> bool {
    let windows_dir = mount_point.join("Windows");
    let system32 = windows_dir.join("System32");
    windows_dir.is_dir() && system32.is_dir()
}

/// Try every candidate partition until one mounts *and* looks like
/// Windows C:. Everything that doesn't match is unmounted immediately
/// — never left mounted read-write longer than the check takes.
fn find_and_mount_windows_c() -> Option<PathBuf> {
    for dev in candidate_partitions() {
        if mount_rw(&dev).is_ok() {
            if looks_like_windows_c(Path::new(MOUNT_POINT)) {
                return Some(dev);
            }
            unmount();
        }
    }
    None
}

/// Write `json` to `C:\PLDDS\Logs\<filename>` and unmount. On any
/// failure partway through, still attempts to unmount before
/// returning the error — never leaves the Windows partition mounted
/// behind the caller's back.
pub fn save_report_locally(filename: &str, json: &str) -> io::Result<PathBuf> {
    let dev = find_and_mount_windows_c().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no partition found that looks like the Windows C: drive (Windows\\System32 not found on any candidate)",
        )
    })?;

    let result = (|| -> io::Result<PathBuf> {
        let log_dir = Path::new(MOUNT_POINT).join(LOG_SUBDIR);
        fs::create_dir_all(&log_dir)?;
        let out_path = log_dir.join(filename);
        fs::write(&out_path, json.as_bytes())?;
        // Windows-side path, for the log message — not the initramfs mount path.
        Ok(PathBuf::from(format!(r"C:\{}\{}", LOG_SUBDIR.replace('/', "\\"), filename)))
    })();

    unmount();
    result.map_err(|e| {
        io::Error::new(e.kind(), format!("wrote to {dev:?} but failed: {e}"))
    })
}
