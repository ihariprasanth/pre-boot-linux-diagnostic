"""initial schema

Revision ID: 0001_initial
Revises:
Create Date: 2026-08-09

Creates the three tables in app/models.py: devices, used_nonces, reports.
"""
from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision = "0001_initial"
down_revision = None
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        "devices",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("device_id", sa.String(), nullable=False),
        sa.Column("hostname", sa.String(), nullable=True),
        sa.Column("public_key", sa.String(), nullable=True),
        sa.Column("first_seen_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("last_seen_at", sa.DateTime(timezone=True), nullable=False),
        sa.UniqueConstraint("device_id", name="uq_devices_device_id"),
    )
    op.create_index("ix_devices_device_id", "devices", ["device_id"])

    op.create_table(
        "used_nonces",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("device_id", sa.String(), nullable=False),
        sa.Column("nonce", sa.String(), nullable=False),
        sa.Column("used_at", sa.DateTime(timezone=True), nullable=False),
        sa.UniqueConstraint("device_id", "nonce", name="uq_device_nonce"),
    )
    op.create_index("ix_used_nonces_used_at", "used_nonces", ["used_at"])

    op.create_table(
        "reports",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("report_id", sa.String(), nullable=False),
        sa.Column("boot_id", sa.String(), nullable=False),
        sa.Column("device_pk", postgresql.UUID(as_uuid=True), sa.ForeignKey("devices.id"), nullable=False),
        sa.Column("schema_version", sa.String(), nullable=False),
        sa.Column("agent_version", sa.String(), nullable=False),
        sa.Column("generated_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("received_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("score", sa.Integer(), nullable=False),
        sa.Column("score_label", sa.String(), nullable=False),
        sa.Column("total", sa.Integer(), nullable=False),
        sa.Column("passed", sa.Integer(), nullable=False),
        sa.Column("warned", sa.Integer(), nullable=False),
        sa.Column("failed", sa.Integer(), nullable=False),
        sa.Column("skipped", sa.Integer(), nullable=False),
        sa.Column("raw_report", postgresql.JSONB(), nullable=False),
        sa.UniqueConstraint("report_id", name="uq_reports_report_id"),
    )
    op.create_index("ix_reports_report_id", "reports", ["report_id"])
    op.create_index("ix_reports_boot_id", "reports", ["boot_id"])
    op.create_index("ix_reports_device_pk_received_at", "reports", ["device_pk", "received_at"])


def downgrade() -> None:
    op.drop_table("reports")
    op.drop_table("used_nonces")
    op.drop_table("devices")
