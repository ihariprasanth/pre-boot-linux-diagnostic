"""
Phase 5 config. All secrets come from environment variables — never hardcode
keys here. See server/.env.example for the variables you need to set.

Phase 12: adds the knobs the security-hardening pass needs — production
mode detection, request-size limits, rate limits, and nonce retention —
so hardening behaviour is configurable per-deployment instead of baked in.
"""
import os
from functools import lru_cache


def _split_csv(value: str) -> list[str]:
    return [v.strip() for v in value.split(",") if v.strip()]


class Settings:
    database_url: str = os.environ["DATABASE_URL"]  # fail fast if unset
    schema_major_version: int = int(os.environ.get("PLDDS_SCHEMA_MAJOR", "1"))
    env: str = os.environ.get("PLDDS_ENV", "development")

    # Phase 11: the dashboard is a separate origin (default Vite dev
    # server on :3000) making read-only GET requests from the browser,
    # so it needs CORS — comma-separated list, no trailing slashes.
    dashboard_origins: list[str] = _split_csv(
        os.environ.get("PLDDS_DASHBOARD_ORIGINS", "http://localhost:3000")
    )

    # --- Phase 12: security hardening -----------------------------------

    @property
    def is_production(self) -> bool:
        return self.env.lower() in ("production", "prod")

    # Hard cap on request body size (bytes) before the body is even
    # parsed — blocks trivial memory-exhaustion DoS via oversized
    # /diagnostics or /devices/register payloads. Diagnostic reports are
    # JSON and bounded in practice; 2 MiB is generous headroom.
    max_body_bytes: int = int(os.environ.get("PLDDS_MAX_BODY_BYTES", str(2 * 1024 * 1024)))

    # Simple in-process rate limits (requests / window) for the two
    # unauthenticated-until-verified write endpoints. Deliberately
    # coarse — real fleets behind a shared IP (NAT) need this generous,
    # a reverse proxy / WAF is expected to do the serious rate limiting.
    register_rate_limit: str = os.environ.get("PLDDS_REGISTER_RATE_LIMIT", "20/minute")
    diagnostics_rate_limit: str = os.environ.get("PLDDS_DIAGNOSTICS_RATE_LIMIT", "60/minute")
    read_rate_limit: str = os.environ.get("PLDDS_READ_RATE_LIMIT", "120/minute")

    # How long a used nonce needs to be kept to still catch a replay.
    # Must be >= security.CLOCK_SKEW_SECONDS with headroom for clock
    # drift/clock jitter; scripts/cleanup_nonces.py purges anything older
    # so the used_nonces table doesn't grow unbounded forever.
    nonce_retention_seconds: int = int(os.environ.get("PLDDS_NONCE_RETENTION_SECONDS", "600"))

    # Trust X-Forwarded-Proto from exactly this many reverse-proxy hops
    # (0 = don't trust any, i.e. this process itself terminates TLS or is
    # only ever reached over plain HTTP in dev). Used to decide whether to
    # send HSTS and to detect a downgraded (http) request behind a proxy.
    trusted_proxy_hops: int = int(os.environ.get("PLDDS_TRUSTED_PROXY_HOPS", "1"))


@lru_cache
def get_settings() -> Settings:
    return Settings()
