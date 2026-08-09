"""
Phase 12: unit tests for the header-shape validation added to
app/security.py's extract_signed_request path.

These deliberately test only the pure validation helpers
(_check_header_shape / _check_hexish_header) rather than the full
extract_signed_request flow, since that needs a live Request object and
a DB session (see server/tests for the fuller integration harness once
Phase 5's Postgres-only models get a test-friendly setup — JSONB/UUID
columns don't run on sqlite, so these stay dependency-light for now).
"""
import pytest
from fastapi import HTTPException

from app.security import (
    _MAX_BOOT_ID_LEN,
    _MAX_DEVICE_ID_LEN,
    _MAX_NONCE_LEN,
    _check_header_shape,
    _check_hexish_header,
)


def test_check_header_shape_accepts_normal_value():
    _check_header_shape("X-Timestamp", "1733760000", 20)  # should not raise


def test_check_header_shape_rejects_empty():
    with pytest.raises(HTTPException) as exc:
        _check_header_shape("X-Timestamp", "", 20)
    assert exc.value.status_code == 401


def test_check_header_shape_rejects_too_long():
    with pytest.raises(HTTPException) as exc:
        _check_header_shape("X-Nonce", "a" * 1000, _MAX_NONCE_LEN)
    assert exc.value.status_code == 401


def test_check_header_shape_rejects_whitespace():
    with pytest.raises(HTTPException):
        _check_header_shape("X-Device-Id", "abc def", _MAX_DEVICE_ID_LEN)


def test_check_hexish_header_accepts_hex_digest():
    device_id = "a" * 64  # SHA-256 hex digest shape
    _check_hexish_header("X-Device-Id", device_id, _MAX_DEVICE_ID_LEN)  # should not raise


def test_check_hexish_header_accepts_uuid_shape():
    boot_id = "550e8400-e29b-41d4-a716-446655440000"
    _check_hexish_header("X-Boot-Id", boot_id, _MAX_BOOT_ID_LEN)  # should not raise


def test_check_hexish_header_rejects_non_hex_characters():
    with pytest.raises(HTTPException) as exc:
        _check_hexish_header("X-Nonce", "not-hex-at-all-zzz!$", _MAX_NONCE_LEN)
    assert exc.value.status_code == 401


def test_check_hexish_header_rejects_sql_injection_attempt():
    with pytest.raises(HTTPException):
        _check_hexish_header(
            "X-Device-Id", "'; DROP TABLE devices; --", _MAX_DEVICE_ID_LEN
        )
