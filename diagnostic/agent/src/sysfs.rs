//! Small helpers for reading `/proc` and `/sys` files that may or may not
//! exist depending on the machine. Every collector in this project uses
//! these instead of raw `std::fs::read_to_string` + `.unwrap()`, because
//! "the file wasn't there" must never crash the agent — see docs/architecture.md
//! section 38 (Error Handling): one missing sensor should never take down
//! the rest of the diagnostic run.

use std::fs;
use std::path::Path;

/// Read a file to a trimmed String. Returns None (not an error) if the
/// file doesn't exist, isn't readable, or isn't valid UTF-8 — all of
/// which are normal and expected on some hardware/kernel combinations.
pub fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read a file and parse it as a u64. None if missing, unreadable, or
/// not a valid integer.
pub fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path).and_then(|s| s.parse::<u64>().ok())
}

/// List directory entries (just the file names, not full paths) sorted
/// alphabetically. Empty Vec — not an error — if the directory is
/// missing or unreadable.
pub fn list_dir_names(path: impl AsRef<Path>) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// Resolve a symlink (e.g. a PCI device's `driver` link) to just the
/// final path component, e.g. `/sys/bus/pci/drivers/nvme` -> `"nvme"`.
/// None if the symlink doesn't exist or can't be read.
pub fn read_link_basename(path: impl AsRef<Path>) -> Option<String> {
    fs::read_link(path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}
