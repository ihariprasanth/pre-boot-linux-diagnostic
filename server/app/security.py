"""
Phase 7: Ed25519 request signature verification + replay protection.

Must match diagnostic/agent/src/upload.rs's signing scheme exactly:

    canonical_payload = "{METHOD}\\n{PATH}\\n{TIMESTAMP}\\n{NONCE}\\n{BOOT_ID}\\n{SHA256(BODY)_HEX}"
    signature         = base64(Ed25519_sign(canonical_payload))

sent as headers: X-Device-Id, X-Timestamp, X-Nonce, X-Boot-Id, X-Signature.

Two trust cases:
  - Known device (public_key already stored) — signature MUST verify
    against the stored key. This is the normal path for /diagnostics
    and re-registration.
  - Brand-new device hitting /devices/register — there's no stored key
    yet, so the request is self-signed: the caller supplies its public
    key in the body and the signature must verify against THAT key.
    This proves possession of the matching private key (you can't forge
    a registration for a public key you don't hold), which is the
    standard TOFU (trust-on-first-use) bootstrap for device identity.
    docs/architecture.md "Security model" describes this as
    "device-credential authentication, no shared tokens".
"""
import base64
import hashlib
import time

from fastapi import HTTPException, Request, status
from nacl.exceptions import BadSignatureError
from nacl.signing import VerifyKey
from sqlalchemy.orm import Session

from app import models

# Requests older/newer than this many seconds (either direction) are
# rejected — bounds the window an intercepted request could be replayed
# in even before the nonce check, and catches badly-skewed device clocks.
CLOCK_SKEW_SECONDS = 120

REQUIRED_HEADERS = ("x-device-id", "x-timestamp", "x-nonce", "x-boot-id", "x-signature")

# Phase 12: hard bounds on the signing headers, checked before any of
# them touch the DB or the signature verifier. device_id is a hex
# SHA-256 digest (64 chars) and nonce is 16 random bytes hex-encoded (32
# chars) per diagnostic/agent/src/crypto.rs and upload.rs — real values
# are always short and hex. Rejecting anything outside generous bounds
# up front stops oversized/garbage header values from being hashed,
# base64-decoded, or written into a nonce-uniqueness query unnecessarily,
# and stops device_id/boot_id from being used as a vector for log
# injection or oversized-row abuse.
_MAX_DEVICE_ID_LEN = 128
_MAX_NONCE_LEN = 128
_MAX_BOOT_ID_LEN = 128
_MAX_SIGNATURE_LEN = 256
_MAX_TIMESTAMP_LEN = 20
_HEX_OR_UUID = frozenset("0123456789abcdefABCDEF-")


def _check_header_shape(name: str, value: str, max_len: int) -> None:
    if not value or len(value) > max_len:
        raise HTTPException(status_code=401, detail=f"{name} header missing or too long")
    if any(c.isspace() for c in value):
        raise HTTPException(status_code=401, detail=f"{name} header contains whitespace")


def _check_hexish_header(name: str, value: str, max_len: int) -> None:
    _check_header_shape(name, value, max_len)
    if not set(value) <= _HEX_OR_UUID:
        raise HTTPException(status_code=401, detail=f"{name} header has unexpected characters")


class SignedRequest:
    def __init__(self, device_id: str, timestamp: str, nonce: str, boot_id: str,
                 signature_b64: str, raw_body: bytes):
        self.device_id = device_id
        self.timestamp = timestamp
        self.nonce = nonce
        self.boot_id = boot_id
        self.signature_b64 = signature_b64
        self.raw_body = raw_body

    def canonical_payload(self, method: str, path: str) -> bytes:
        body_hash = hashlib.sha256(self.raw_body).hexdigest()
        canonical = f"{method}\n{path}\n{self.timestamp}\n{self.nonce}\n{self.boot_id}\n{body_hash}"
        return canonical.encode("utf-8")


async def extract_signed_request(request: Request) -> SignedRequest:
    headers = request.headers
    missing = [h for h in REQUIRED_HEADERS if h not in headers]
    if missing:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=f"missing required signing headers: {missing}",
        )

    device_id = headers["x-device-id"]
    timestamp = headers["x-timestamp"]
    nonce = headers["x-nonce"]
    boot_id = headers["x-boot-id"]
    signature_b64 = headers["x-signature"]

    # Phase 12: shape-check every signing header before doing anything
    # else with them (DB query, hashing, base64 decode). device_id is
    # a hex digest and nonce/boot_id are hex/UUID-shaped in the real
    # agent — enforce that here rather than only implicitly via
    # signature failure, so malformed input is rejected cheaply and
    # can't reach the nonce-uniqueness DB write with junk data.
    _check_hexish_header("X-Device-Id", device_id, _MAX_DEVICE_ID_LEN)
    _check_hexish_header("X-Nonce", nonce, _MAX_NONCE_LEN)
    _check_hexish_header("X-Boot-Id", boot_id, _MAX_BOOT_ID_LEN)
    _check_header_shape("X-Timestamp", timestamp, _MAX_TIMESTAMP_LEN)
    _check_header_shape("X-Signature", signature_b64, _MAX_SIGNATURE_LEN)

    raw_body = await request.body()

    return SignedRequest(
        device_id=device_id,
        timestamp=timestamp,
        nonce=nonce,
        boot_id=boot_id,
        signature_b64=signature_b64,
        raw_body=raw_body,
    )


def check_timestamp_fresh(timestamp_str: str) -> None:
    try:
        ts = int(timestamp_str)
    except ValueError:
        raise HTTPException(status_code=401, detail="X-Timestamp must be a unix epoch integer")

    skew = abs(time.time() - ts)
    if skew > CLOCK_SKEW_SECONDS:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=f"request timestamp outside allowed {CLOCK_SKEW_SECONDS}s skew window",
        )


def verify_signature(public_key_b64: str, canonical_payload: bytes, signature_b64: str) -> None:
    try:
        verify_key = VerifyKey(base64.b64decode(public_key_b64))
        verify_key.verify(canonical_payload, base64.b64decode(signature_b64))
    except (BadSignatureError, ValueError, TypeError):
        raise HTTPException(status_code=401, detail="signature verification failed")


def check_and_record_nonce(db: Session, device_id: str, nonce: str) -> None:
    """
    Rejects a (device_id, nonce) pair that's been seen before — the core
    replay-protection check (docs/architecture.md "Security model").
    Relies on the DB unique constraint as the source of truth (race-safe
    under concurrent requests), not just a pre-check-then-insert.
    """
    from sqlalchemy.exc import IntegrityError

    record = models.UsedNonce(device_id=device_id, nonce=nonce)
    db.add(record)
    try:
        db.flush()
    except IntegrityError:
        db.rollback()
        raise HTTPException(status_code=401, detail="replayed request (nonce already used)")


async def verify_known_device(request: Request, db: Session) -> tuple[SignedRequest, models.Device]:
    """
    Full verification path for endpoints that require an already-
    registered device (e.g. /diagnostics). 401s on any failure —
    unknown device, bad timestamp, replayed nonce, or bad signature.
    """
    signed = await extract_signed_request(request)
    check_timestamp_fresh(signed.timestamp)

    device = db.query(models.Device).filter_by(device_id=signed.device_id).one_or_none()
    if device is None or not device.public_key:
        raise HTTPException(status_code=401, detail="device not registered")

    canonical = signed.canonical_payload(request.method, request.url.path)
    verify_signature(device.public_key, canonical, signed.signature_b64)
    check_and_record_nonce(db, signed.device_id, signed.nonce)

    return signed, device
