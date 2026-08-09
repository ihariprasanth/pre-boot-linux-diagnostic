import logging
from datetime import datetime, timezone

from fastapi import FastAPI, Depends, HTTPException, Query, Request, status
from fastapi.exceptions import RequestValidationError
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from pydantic import ValidationError
from slowapi import Limiter, _rate_limit_exceeded_handler
from slowapi.errors import RateLimitExceeded
from slowapi.util import get_remote_address
from sqlalchemy import func, text
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from app.config import get_settings
from app.database import Base, engine, get_db
from app.middleware import BodySizeLimitMiddleware, SecurityHeadersMiddleware
from app import models, schemas, security

logger = logging.getLogger("pldds")

settings = get_settings()

# Phase 5: create tables directly. A real alembic migration replaces this
# once the schema needs to evolve non-trivially in place (production data
# already stored) — see server/alembic/ (scaffolded, not yet driving
# startup). Phase 7 added columns/tables are still safe to create-all
# fresh since nothing is deployed yet.
Base.metadata.create_all(bind=engine)

app = FastAPI(
    title="PLDDS Backend",
    version="0.12.0",
    # Phase 12: don't expose interactive docs/schema in production —
    # they're a free map of the API surface for an attacker and aren't
    # needed once the agent/dashboard integrations are stable.
    docs_url=None if settings.is_production else "/docs",
    redoc_url=None if settings.is_production else "/redoc",
    openapi_url=None if settings.is_production else "/openapi.json",
)

# --- Phase 12: rate limiting -------------------------------------------
# Coarse per-IP limiting on the write endpoints so a single misbehaving
# or hostile client can't hammer signature verification (relatively
# cheap, but not free) or fill the used_nonces table. This is a backstop,
# not the primary defense — a reverse proxy/WAF in front of a real
# deployment should do the heavy lifting.
limiter = Limiter(key_func=get_remote_address)
app.state.limiter = limiter
app.add_exception_handler(RateLimitExceeded, _rate_limit_exceeded_handler)

# --- Phase 12: security headers + body size cap -------------------------
# Applied to every request, ahead of CORS/auth. Middleware order matters:
# Starlette runs middleware in reverse-add order around the request, so
# adding these after CORS keeps CORS as the outermost layer (needed for
# proper preflight/error responses to still carry CORS headers).
app.add_middleware(BodySizeLimitMiddleware, max_bytes=settings.max_body_bytes)
app.add_middleware(
    SecurityHeadersMiddleware,
    hsts=settings.is_production and settings.trusted_proxy_hops > 0,
)

# Phase 11: dashboard reads over the browser from a different origin.
# Read-only surface, so only GET (+ the preflight OPTIONS FastAPI/Starlette
# handles for us) needs to be allowed — never widen this to mutating verbs
# without also giving the dashboard its own device-style credential.
# Phase 12: allow_headers narrowed from "*" to what the dashboard's
# fetch client actually sends — a wildcard here costs nothing an
# attacker can exploit directly (this is a read-only, unauthenticated
# GET surface) but a fixed list is the honest description of what's
# supported and easier to review at a glance.
app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.dashboard_origins,
    allow_methods=["GET"],
    allow_headers=["Content-Type", "Accept"],
    max_age=600,
)


@app.exception_handler(RequestValidationError)
async def validation_exception_handler(request: Request, exc: RequestValidationError):
    # Phase 12: FastAPI's default 422 body echoes back the raw input
    # value for every failed field, which can leak submitted secrets or
    # PII into logs/responses. In production, return the error shape
    # (field paths + messages) without the raw `input` payload; keep the
    # full detail in non-production so local debugging isn't hurt.
    if not settings.is_production:
        return JSONResponse(status_code=422, content={"detail": exc.errors()})

    sanitized = [
        {"loc": e.get("loc"), "msg": e.get("msg"), "type": e.get("type")}
        for e in exc.errors()
    ]
    return JSONResponse(status_code=422, content={"detail": sanitized})


@app.exception_handler(Exception)
async def unhandled_exception_handler(request: Request, exc: Exception):
    # Phase 12: never let a raw stack trace / exception message reach the
    # client — that's an information leak (internal paths, query text,
    # library versions). Log the real thing server-side, return a flat
    # 500 to the caller either way.
    logger.exception("unhandled exception on %s %s", request.method, request.url.path)
    return JSONResponse(status_code=500, content={"detail": "internal server error"})


def _device_out(db: Session, device: models.Device) -> schemas.DeviceOut:
    """Shared DeviceOut assembly for get_device and list_devices (Phase 11)."""
    latest = (
        db.query(models.Report)
        .filter_by(device_pk=device.id)
        .order_by(models.Report.received_at.desc())
        .first()
    )
    report_count = db.query(models.Report).filter_by(device_pk=device.id).count()

    return schemas.DeviceOut(
        id=device.id,
        device_id=device.device_id,
        hostname=device.hostname,
        first_seen_at=device.first_seen_at,
        last_seen_at=device.last_seen_at,
        report_count=report_count,
        latest_report=schemas.ReportSummaryOut(
            report_id=latest.report_id,
            boot_id=latest.boot_id,
            generated_at=latest.generated_at,
            received_at=latest.received_at,
            score=latest.score,
            score_label=latest.score_label,
        ) if latest else None,
    )


# ---------------------------------------------------------------------
# 1. Register device
# ---------------------------------------------------------------------
@app.post(
    "/devices/register",
    response_model=schemas.RegisterDeviceResponse,
    status_code=status.HTTP_200_OK,
)
@limiter.limit(settings.register_rate_limit)
async def register_device(request: Request, db: Session = Depends(get_db)):
    """
    Idempotent: registering an already-known device_id just refreshes
    last_seen_at.

    Phase 7 auth (see app/security.py):
      - New device_id: TOFU bootstrap — signature must verify against
        the public_key in THIS request's own body. That public_key is
        then stored permanently.
      - Existing device_id: signature must verify against the
        ALREADY-STORED public_key — the body's public_key field is
        accepted but ignored, so a stolen/forged request can't rotate
        a device's key.
    Every request (new or existing) still needs a fresh nonce + a
    timestamp inside the skew window.
    """
    signed = await security.extract_signed_request(request)
    security.check_timestamp_fresh(signed.timestamp)

    try:
        body = schemas.RegisterDeviceRequest.model_validate_json(signed.raw_body)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())

    if body.device_id != signed.device_id:
        raise HTTPException(status_code=401, detail="X-Device-Id does not match body.device_id")

    device = db.query(models.Device).filter_by(device_id=body.device_id).one_or_none()
    canonical = signed.canonical_payload(request.method, request.url.path)

    if device is None or not device.public_key:
        # TOFU bootstrap: verify against the key being registered.
        security.verify_signature(body.public_key, canonical, signed.signature_b64)
    else:
        # Existing device: verify against the stored key, ignore any
        # different key offered in the body.
        security.verify_signature(device.public_key, canonical, signed.signature_b64)

    security.check_and_record_nonce(db, signed.device_id, signed.nonce)

    now = datetime.now(timezone.utc)
    if device is None:
        device = models.Device(
            device_id=body.device_id,
            hostname=body.hostname,
            public_key=body.public_key,
            first_seen_at=now,
            last_seen_at=now,
        )
        db.add(device)
    else:
        device.last_seen_at = now
        if body.hostname is not None:
            device.hostname = body.hostname

    db.commit()
    db.refresh(device)
    return device


# ---------------------------------------------------------------------
# 2. Submit diagnostics
# ---------------------------------------------------------------------
@app.post(
    "/diagnostics",
    response_model=schemas.SubmitDiagnosticsResponse,
    status_code=status.HTTP_201_CREATED,
)
@limiter.limit(settings.diagnostics_rate_limit)
async def submit_diagnostics(request: Request, db: Session = Depends(get_db)):
    """
    Phase 7: requires a valid signature from an already-registered
    device (app/security.verify_known_device) — no auto-registration of
    unknown devices anymore, since an unknown device has no key on file
    to verify against. Register first via /devices/register.

    Validates the full body against the Phase 4 JSON schema (via the
    Pydantic models in schemas.py) before storing. Two layers of replay
    protection now apply: the nonce check (per-request, from Phase 7)
    and report_id uniqueness (per-report, from Phase 6) — a resubmitted
    report_id is rejected with 409 even if somehow re-signed correctly.
    """
    signed, device = await security.verify_known_device(request, db)

    try:
        body = schemas.DiagnosticReport.model_validate_json(signed.raw_body)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())

    if body.device.device_id != signed.device_id:
        raise HTTPException(status_code=401, detail="X-Device-Id does not match report.device.device_id")
    if body.boot_id != signed.boot_id:
        raise HTTPException(status_code=401, detail="X-Boot-Id does not match report.boot_id")

    device.last_seen_at = datetime.now(timezone.utc)
    if body.device.hostname is not None:
        device.hostname = body.device.hostname

    report = models.Report(
        report_id=body.report_id,
        boot_id=body.boot_id,
        device_pk=device.id,
        schema_version=body.schema_version,
        agent_version=body.agent_version,
        generated_at=body.generated_at,
        score=body.summary.score,
        score_label=body.summary.score_label.value,
        total=body.summary.total,
        passed=body.summary.passed,
        warned=body.summary.warned,
        failed=body.summary.failed,
        skipped=body.summary.skipped,
        raw_report=body.model_dump(mode="json"),
    )
    db.add(report)

    try:
        db.commit()
    except IntegrityError:
        db.rollback()
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"report_id {body.report_id!r} already submitted (replay)",
        )

    return schemas.SubmitDiagnosticsResponse(report_id=body.report_id, stored=True, ack=True)


# ---------------------------------------------------------------------
# 3. Get report
# ---------------------------------------------------------------------
@app.get("/reports/{report_id}", response_model=schemas.ReportOut)
@limiter.limit(settings.read_rate_limit)
def get_report(request: Request, report_id: str, db: Session = Depends(get_db)):
    report = db.query(models.Report).filter_by(report_id=report_id).one_or_none()
    if report is None:
        raise HTTPException(status_code=404, detail="report not found")

    return schemas.ReportOut(
        report_id=report.report_id,
        boot_id=report.boot_id,
        generated_at=report.generated_at,
        received_at=report.received_at,
        score=report.score,
        score_label=report.score_label,
        schema_version=report.schema_version,
        agent_version=report.agent_version,
        device_id=report.device.device_id,
        raw_report=report.raw_report,
    )


# ---------------------------------------------------------------------
# 4. Get device
# ---------------------------------------------------------------------
@app.get("/devices/{device_id}", response_model=schemas.DeviceOut)
@limiter.limit(settings.read_rate_limit)
def get_device(request: Request, device_id: str, db: Session = Depends(get_db)):
    device = db.query(models.Device).filter_by(device_id=device_id).one_or_none()
    if device is None:
        raise HTTPException(status_code=404, detail="device not found")
    return _device_out(db, device)


# ---------------------------------------------------------------------
# 4a. List devices (Phase 11 — dashboard device list / fleet overview)
# ---------------------------------------------------------------------
@app.get("/devices", response_model=schemas.DeviceListOut)
@limiter.limit(settings.read_rate_limit)
def list_devices(
    request: Request,
    db: Session = Depends(get_db),
    limit: int = Query(default=50, ge=1, le=200),
    offset: int = Query(default=0, ge=0),
):
    total = db.query(func.count(models.Device.id)).scalar()
    rows = (
        db.query(models.Device)
        .order_by(models.Device.last_seen_at.desc())
        .offset(offset)
        .limit(limit)
        .all()
    )
    return schemas.DeviceListOut(
        devices=[_device_out(db, d) for d in rows],
        total=total,
        limit=limit,
        offset=offset,
    )


# ---------------------------------------------------------------------
# 4b. List a device's reports (Phase 11 — dashboard boot history)
# ---------------------------------------------------------------------
@app.get("/devices/{device_id}/reports", response_model=schemas.ReportListOut)
@limiter.limit(settings.read_rate_limit)
def list_device_reports(
    request: Request,
    device_id: str,
    db: Session = Depends(get_db),
    limit: int = Query(default=50, ge=1, le=200),
    offset: int = Query(default=0, ge=0),
):
    device = db.query(models.Device).filter_by(device_id=device_id).one_or_none()
    if device is None:
        raise HTTPException(status_code=404, detail="device not found")

    total = db.query(func.count(models.Report.id)).filter_by(device_pk=device.id).scalar()
    rows = (
        db.query(models.Report)
        .filter_by(device_pk=device.id)
        .order_by(models.Report.received_at.desc())
        .offset(offset)
        .limit(limit)
        .all()
    )
    return schemas.ReportListOut(
        reports=[
            schemas.ReportSummaryOut(
                report_id=r.report_id,
                boot_id=r.boot_id,
                generated_at=r.generated_at,
                received_at=r.received_at,
                score=r.score,
                score_label=r.score_label,
            )
            for r in rows
        ],
        total=total,
        limit=limit,
        offset=offset,
    )


# ---------------------------------------------------------------------
# 5. Health
# ---------------------------------------------------------------------
@app.get("/health", response_model=schemas.HealthOut)
@limiter.limit(settings.read_rate_limit)
def health(request: Request, db: Session = Depends(get_db)):
    try:
        db.execute(text("SELECT 1"))
        db_status = "ok"
    except Exception:
        # Phase 12: don't let the raw exception (connection string
        # fragments, host/port, driver error text) leak through an
        # unauthenticated endpoint — log it, report a flat status.
        logger.exception("health check: database unreachable")
        db_status = "unreachable"
    return schemas.HealthOut(status="ok", db=db_status)
