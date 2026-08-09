//! Phase 6 transport + Phase 7 auth: uploads the assembled
//! `DiagnosticReport` to the backend, signed with the device's Ed25519
//! key, and waits for the server ACK.
//!
//! Signing scheme (must match `server/app/security.py` exactly):
//!   canonical_payload = "{METHOD}\n{PATH}\n{TIMESTAMP}\n{NONCE}\n{BOOT_ID}\n{SHA256(BODY)_HEX}"
//!   signature         = base64(Ed25519_sign(canonical_payload))
//! sent as headers:
//!   X-Device-Id, X-Timestamp, X-Nonce, X-Boot-Id, X-Signature
//! `NONCE` is fresh per request; the server rejects any (device_id, nonce)
//! pair it has already seen, and any timestamp outside its allowed skew
//! window — see docs/architecture.md "Security model" (replay protection).
//!
//! Boot-safety (docs/architecture.md "Failure safety" — default mode
//! `GRACEFUL`): every network interaction here is bounded by
//! `PLDDS_UPLOAD_TIMEOUT_SECS`. If the server is unreachable, slow, or
//! returns an error, this module returns `Err` and the caller (`main.rs`)
//! continues the boot regardless.
//!
//! TLS: `PLDDS_SERVER_URL` should be `https://` in any real deployment —
//! `reqwest`'s `rustls-tls` feature makes that work out of the box.
//! Plain `http://` is still accepted (for the QEMU/dev loop against a
//! local backend) unless `PLDDS_REQUIRE_TLS=1` is set, which real
//! installs (Phase 13) should always set.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::crypto::{self, DeviceIdentity};
use crate::report::DiagnosticReport;

fn server_base_url() -> String {
    std::env::var("PLDDS_SERVER_URL").unwrap_or_else(|_| "http://10.0.2.2:8000".to_string())
}

fn require_tls() -> bool {
    std::env::var("PLDDS_REQUIRE_TLS").map(|v| v == "1").unwrap_or(false)
}

fn upload_timeout() -> Duration {
    let secs = std::env::var("PLDDS_UPLOAD_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10);
    Duration::from_secs(secs)
}

#[derive(Debug)]
pub enum UploadError {
    InsecureUrlRejected(String),
    Network(reqwest::Error),
    ServerRejected { status: u16, body: String },
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::InsecureUrlRejected(url) => {
                write!(f, "PLDDS_REQUIRE_TLS=1 but server URL is not https: {url}")
            }
            UploadError::Network(e) => write!(f, "network error: {e}"),
            UploadError::ServerRejected { status, body } => {
                write!(f, "server rejected request (HTTP {status}): {body}")
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct SubmitAck {
    #[allow(dead_code)]
    report_id: String,
    #[allow(dead_code)]
    stored: bool,
    ack: bool,
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, UploadError> {
    reqwest::blocking::Client::builder().timeout(timeout).build().map_err(UploadError::Network)
}

fn fresh_nonce() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn unix_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Builds the canonical payload, signs it, and returns the header set
/// to attach to the request. `path` must be exactly what the server
/// sees (e.g. `/diagnostics`), and `body` the exact bytes being sent —
/// signature verification fails if either doesn't match byte-for-byte.
fn signed_headers(
    identity: &DeviceIdentity,
    method: &str,
    path: &str,
    body: &[u8],
    boot_id: &str,
) -> Vec<(&'static str, String)> {
    let timestamp = unix_timestamp().to_string();
    let nonce = fresh_nonce();
    let body_hash = hex::encode(Sha256::digest(body));

    let canonical = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{boot_id}\n{body_hash}");
    let signature = crypto::sign(identity, canonical.as_bytes());

    vec![
        ("X-Device-Id", identity.device_id.clone()),
        ("X-Timestamp", timestamp),
        ("X-Nonce", nonce),
        ("X-Boot-Id", boot_id.to_string()),
        ("X-Signature", signature),
    ]
}

fn check_url_allowed(base_url: &str) -> Result<(), UploadError> {
    if require_tls() && !base_url.starts_with("https://") {
        return Err(UploadError::InsecureUrlRejected(base_url.to_string()));
    }
    Ok(())
}

/// Calls `POST /devices/register`, sending the device's public key.
/// Signed like every other request so the server can bind the very
/// first registration to a real Ed25519 keypair — there's no
/// unauthenticated bootstrap step. Best-effort: `/diagnostics`
/// auto-registers unknown-but-valid devices too, so a transient
/// failure here doesn't block the upload attempt.
fn register_device(
    base_url: &str,
    http: &reqwest::blocking::Client,
    identity: &DeviceIdentity,
    report: &DiagnosticReport,
) {
    let body = serde_json::json!({
        "device_id": identity.device_id,
        "hostname": report.device.hostname,
        "public_key": identity.public_key_b64,
    });
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    let headers = signed_headers(identity, "POST", "/devices/register", &body_bytes, &report.boot_id);

    let mut req = http.post(format!("{base_url}/devices/register")).body(body_bytes);
    req = req.header("Content-Type", "application/json");
    for (k, v) in &headers {
        req = req.header(*k, v);
    }

    match req.send() {
        Ok(resp) if resp.status().is_success() => {
            println!("[agent] device registered ({})", identity.device_id);
        }
        Ok(resp) => println!(
            "[agent] device registration returned HTTP {} — continuing",
            resp.status()
        ),
        Err(e) => println!("[agent] device registration failed ({e}) — continuing"),
    }
}

/// Uploads the report and waits for the server's ACK.
pub fn submit_report(identity: &DeviceIdentity, report: &DiagnosticReport) -> Result<(), UploadError> {
    let base_url = server_base_url();
    check_url_allowed(&base_url)?;
    let timeout = upload_timeout();
    let http = client(timeout)?;

    register_device(&base_url, &http, identity, report);

    let body_bytes = serde_json::to_vec(report).map_err(|e| UploadError::ServerRejected {
        status: 0,
        body: format!("failed to serialize report: {e}"),
    })?;
    let headers = signed_headers(identity, "POST", "/diagnostics", &body_bytes, &report.boot_id);

    let mut req = http.post(format!("{base_url}/diagnostics")).body(body_bytes);
    req = req.header("Content-Type", "application/json");
    for (k, v) in &headers {
        req = req.header(*k, v);
    }

    let resp = req.send().map_err(UploadError::Network)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(UploadError::ServerRejected { status: status.as_u16(), body });
    }

    let ack: SubmitAck = resp.json().map_err(UploadError::Network)?;
    if !ack.ack {
        return Err(UploadError::ServerRejected {
            status: status.as_u16(),
            body: "server accepted report but did not set ack=true".to_string(),
        });
    }

    println!("[agent] report {} uploaded and ACKed by server", report.report_id);
    Ok(())
}
