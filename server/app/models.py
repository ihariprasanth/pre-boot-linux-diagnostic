import uuid
from datetime import datetime, timezone

from sqlalchemy import (
    Column, String, Integer, Boolean, DateTime, ForeignKey, UniqueConstraint, Index
)
from sqlalchemy.dialects.postgresql import JSONB, UUID
from sqlalchemy.orm import relationship

from app.database import Base


def _now():
    return datetime.now(timezone.utc)


class Device(Base):
    """
    Phase 5: device_id is the unprovisioned placeholder from the agent
    (schema note: device.device_id). Phase 7 replaces this with the real
    hashed/keypair-backed identity — this table's device_id column stays
    the same shape (string), only how it's produced changes.
    """
    __tablename__ = "devices"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    device_id = Column(String, nullable=False, unique=True, index=True)
    hostname = Column(String, nullable=True)

    # Phase 7: base64 Ed25519 public key, set once at first registration
    # (TOFU bootstrap — see app/security.py) and never overwritten after.
    # Every subsequent request from this device_id must verify against
    # this exact key.
    public_key = Column(String, nullable=True)

    first_seen_at = Column(DateTime(timezone=True), nullable=False, default=_now)
    last_seen_at = Column(DateTime(timezone=True), nullable=False, default=_now)

    reports = relationship("Report", back_populates="device", cascade="all, delete-orphan")


class UsedNonce(Base):
    """
    Phase 7 replay protection: one row per (device_id, nonce) ever
    accepted. A unique constraint on the pair is what actually blocks
    replays — see app/security.py check_and_record_nonce.
    """
    __tablename__ = "used_nonces"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    device_id = Column(String, nullable=False)
    nonce = Column(String, nullable=False)
    used_at = Column(DateTime(timezone=True), nullable=False, default=_now)

    __table_args__ = (
        UniqueConstraint("device_id", "nonce", name="uq_device_nonce"),
        # Phase 12: scripts/cleanup_nonces.py deletes by used_at < cutoff
        # on a schedule — without this index that's a full table scan
        # every run as the table grows.
        Index("ix_used_nonces_used_at", "used_at"),
    )


class Report(Base):
    __tablename__ = "reports"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)

    # From the report body — used for idempotency/replay protection.
    report_id = Column(String, nullable=False, unique=True, index=True)
    boot_id = Column(String, nullable=False, index=True)

    device_pk = Column(UUID(as_uuid=True), ForeignKey("devices.id"), nullable=False)
    device = relationship("Device", back_populates="reports")

    schema_version = Column(String, nullable=False)
    agent_version = Column(String, nullable=False)
    generated_at = Column(DateTime(timezone=True), nullable=False)
    received_at = Column(DateTime(timezone=True), nullable=False, default=_now)

    # Summary pulled out as real columns for querying/filtering.
    score = Column(Integer, nullable=False)
    score_label = Column(String, nullable=False)
    total = Column(Integer, nullable=False)
    passed = Column(Integer, nullable=False)
    warned = Column(Integer, nullable=False)
    failed = Column(Integer, nullable=False)
    skipped = Column(Integer, nullable=False)

    # Full validated report body, retained as-is (schema §"raw report retention").
    raw_report = Column(JSONB, nullable=False)

    __table_args__ = (
        Index("ix_reports_device_pk_received_at", "device_pk", "received_at"),
    )
