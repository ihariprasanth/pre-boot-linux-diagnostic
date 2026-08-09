#!/usr/bin/env python3
"""
Phase 12: purge used_nonces rows older than they need to be.

app/security.check_and_record_nonce relies on the (device_id, nonce)
unique constraint to block replays, and app/security.check_timestamp_fresh
already rejects any request more than CLOCK_SKEW_SECONDS (120s) old — so a
nonce older than that window can never again be presented on a request
that would pass the timestamp check anyway. Keeping every nonce forever
just grows the table (and its unique index) without buying any more
protection.

Run this on a schedule (cron / systemd timer / hosting platform's
scheduled-job feature), e.g. every 15 minutes:

    */15 * * * * cd /path/to/pldds/server && python -m scripts.cleanup_nonces

Retention window defaults to Settings.nonce_retention_seconds (600s —
5x the clock-skew window, generous headroom for a delayed cron run).
Safe to run concurrently with the live API: it only deletes rows, never
reads them for auth decisions.
"""
import sys
from datetime import datetime, timedelta, timezone

sys.path.insert(0, __file__.rsplit("/scripts/", 1)[0])  # run as a script, not just -m

from app.config import get_settings  # noqa: E402
from app.database import SessionLocal  # noqa: E402
from app.models import UsedNonce  # noqa: E402


def cleanup_nonces() -> int:
    settings = get_settings()
    cutoff = datetime.now(timezone.utc) - timedelta(seconds=settings.nonce_retention_seconds)

    db = SessionLocal()
    try:
        deleted = db.query(UsedNonce).filter(UsedNonce.used_at < cutoff).delete()
        db.commit()
        return deleted
    finally:
        db.close()


if __name__ == "__main__":
    count = cleanup_nonces()
    print(f"[cleanup_nonces] deleted {count} expired nonce row(s)")
