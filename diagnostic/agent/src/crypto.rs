//! Phase 7: device keypair provisioning + request signing.
//!
//! Replaces the Phase 4 `placeholder_device_identity()` with the real
//! privacy-conscious hashed identity described in
//! docs/architecture.md "Security model":
//!   - `device_id` = SHA-256(public_key), hex — stable across boots
//!     (as long as the key persists — see the KNOWN LIMITATION below),
//!     reveals nothing about the physical hardware itself.
//!   - Every request to the backend is signed with the device's Ed25519
//!     private key; the server verifies against the public key it
//!     received at first registration (docs/architecture.md "Device
//!     identity").
//!
//! KNOWN LIMITATION (tracked for Phase 9/13): the diagnostic
//! environment's initramfs is RAM-only with no persistent filesystem
//! mounted yet, so `PLDDS_KEY_PATH` defaults to a tmpfs path — the key
//! (and therefore `device_id`) is regenerated every boot until Phase 9
//! wires a real persistent location (e.g. a small file on the EFI
//! System Partition, written once at install time in Phase 13). The
//! signing protocol itself does not change when that lands — only
//! where the key is read from.

use std::fs;
use std::path::PathBuf;

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub struct DeviceIdentity {
    pub device_id: String,
    pub public_key_b64: String,
    signing_key: SigningKey,
}

fn key_path() -> PathBuf {
    std::env::var("PLDDS_KEY_PATH")
        .unwrap_or_else(|_| "/run/pldds-device.key".to_string())
        .into()
}

/// Loads the persisted signing key if present, otherwise generates a
/// fresh one and writes it out. `perm 600`-equivalent: initramfs has no
/// multi-user concept, but we still avoid world-readable modes where
/// the underlying fs supports them.
pub fn load_or_provision() -> DeviceIdentity {
    let path = key_path();

    let signing_key = match fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => {
            let arr: [u8; 32] = bytes.try_into().expect("checked len == 32 above");
            SigningKey::from_bytes(&arr)
        }
        _ => {
            let mut csprng = OsRng;
            let fresh = SigningKey::generate(&mut csprng);
            if let Err(e) = fs::write(&path, fresh.to_bytes()) {
                println!(
                    "[agent] warning: could not persist device key to {} ({e}) — \
                     a fresh identity will be provisioned next boot too",
                    path.display()
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
            fresh
        }
    };

    let public_key: VerifyingKey = signing_key.verifying_key();
    let device_id = hex::encode(Sha256::digest(public_key.as_bytes()));
    let public_key_b64 = B64.encode(public_key.as_bytes());

    DeviceIdentity { device_id, public_key_b64, signing_key }
}

/// Signs `canonical_payload` (see `upload.rs` for how it's built) and
/// returns the base64-encoded Ed25519 signature to send in the
/// `X-Signature` header.
pub fn sign(identity: &DeviceIdentity, canonical_payload: &[u8]) -> String {
    let sig: Signature = identity.signing_key.sign(canonical_payload);
    B64.encode(sig.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_stable_for_same_key_bytes() {
        let mut csprng = OsRng;
        let key = SigningKey::generate(&mut csprng);
        let pk = key.verifying_key();
        let id1 = hex::encode(Sha256::digest(pk.as_bytes()));
        let id2 = hex::encode(Sha256::digest(pk.as_bytes()));
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64); // sha256 hex
    }

    #[test]
    fn signature_verifies_against_the_public_key() {
        let mut csprng = OsRng;
        let key = SigningKey::generate(&mut csprng);
        let identity = DeviceIdentity {
            device_id: "test".to_string(),
            public_key_b64: B64.encode(key.verifying_key().as_bytes()),
            signing_key: key,
        };
        let payload = b"POST\n/diagnostics\n1234567890\nnonce-abc\nboot-xyz\nbodyhash";
        let sig_b64 = sign(&identity, payload);
        let sig_bytes = B64.decode(sig_b64).unwrap();
        let sig = Signature::from_slice(&sig_bytes).unwrap();
        let pk = identity.signing_key.verifying_key();
        assert!(pk.verify_strict(payload, &sig).is_ok());
    }
}
