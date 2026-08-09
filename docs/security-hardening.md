# Phase 12 — Security Hardening

Full review pass across auth, TLS, replay protection, and general attack
surface, building on the Phase 7 signing/replay design. This is a
checklist + rationale document, not a new subsystem — most of it is
tightening what Phases 5–11 already built.

## 1. Auth / signature path (`server/app/security.py`)

- **Header shape validation (new).** Every signing header
  (`X-Device-Id`, `X-Nonce`, `X-Boot-Id`, `X-Timestamp`, `X-Signature`)
  is now length- and charset-checked *before* it's used in a DB query,
  hashed, or base64-decoded. Real values from `diagnostic/agent` are
  short hex/UUID strings — anything else is rejected immediately with a
  generic 401, which also means a malformed/oversized header can't reach
  the `used_nonces` unique-constraint write with garbage data.
- **TOFU bootstrap stays TOFU.** No change to the trust model: first
  registration proves possession of the private key for the public key
  being registered; every later request for that `device_id` must
  verify against the *stored* key. Confirmed the `/devices/register`
  path ignores a different key offered in the body for an existing
  device — a stolen/forged request still can't rotate a device's
  identity.
- **Known trade-off, left as-is:** `/diagnostics` on an unknown
  `device_id` returns `"device not registered"` vs. a bad signature
  returning `"signature verification failed"` — technically lets an
  attacker distinguish "not registered" from "registered, wrong key".
  Not fixed this phase: the fix (uniform error + constant-time dummy
  verify) adds real complexity for low payoff here, since `device_id`
  is a public SHA-256 digest anyway (not a secret) and enumerating it
  buys an attacker nothing without also holding a matching private key.
- **Nonce retention.** `used_nonces` grew forever with no cleanup. Since
  `check_timestamp_fresh` already rejects anything outside the
  ±120s clock-skew window, a nonce older than that can never be
  replayed successfully regardless of whether the row still exists.
  `server/scripts/cleanup_nonces.py` purges rows past
  `PLDDS_NONCE_RETENTION_SECONDS` (default 600s, 5x the skew window)
  — run it on a schedule (cron/systemd timer/host scheduler). Added an
  index on `used_at` so that delete doesn't become a full table scan as
  the table grows.

## 2. Transport / TLS

- `diagnostic/agent/src/upload.rs` already refuses non-`https://` URLs
  when `PLDDS_REQUIRE_TLS=1`. Confirmed this env var should be set for
  every non-dev deployment — documented explicitly in
  `server/.env.example` (Phase 7) and repeated here: **Phase 13 (real
  hardware install) must set `PLDDS_REQUIRE_TLS=1` and an `https://`
  `PLDDS_SERVER_URL`, no exceptions.**
- Server side: this app expects TLS termination at a reverse proxy /
  hosting platform (Render, Fly, etc.) per `server/.env.example`. Added
  conditional `Strict-Transport-Security` (HSTS) response headers,
  sent only when `PLDDS_ENV=production` **and** `PLDDS_TRUSTED_PROXY_HOPS
  > 0` — sending HSTS from a plain-HTTP dev server would force-upgrade
  future `http://localhost` requests in a browser and break the local
  dev loop.

## 3. Replay protection

Two independent layers, unchanged in design, hardening applied around
the edges this phase:

1. **Per-request nonce** (`used_nonces` unique constraint) — now with
   bounded input (see §1) and a retention/cleanup story (see §1).
2. **Per-report idempotency** (`reports.report_id` unique constraint) —
   already race-safe via the DB constraint + `IntegrityError` → 409,
   no change needed.

## 4. General attack surface (new this phase)

- **Response headers** (`server/app/middleware.py:SecurityHeadersMiddleware`):
  `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`,
  `Referrer-Policy: no-referrer`, a locked-down `Content-Security-Policy`
  (this API serves JSON only, never HTML), `Permissions-Policy`
  disabling sensor/payment APIs, and the `Server` header stripped so the
  stack isn't advertised. Conditional HSTS as above.
- **Request body size cap**
  (`server/app/middleware.py:BodySizeLimitMiddleware`): rejects (413)
  any body over `PLDDS_MAX_BODY_BYTES` (default 2 MiB) both via a fast
  `Content-Length` pre-check and a streaming byte-count guard (so a
  client can't lie about `Content-Length` or use chunked transfer to
  dodge the pre-check). Runs before signature verification or JSON
  parsing, so an oversized payload never gets buffered into memory or
  hashed.
- **Rate limiting** (`slowapi`, per-IP): `/devices/register` and
  `/diagnostics` get their own limits (`PLDDS_REGISTER_RATE_LIMIT`,
  `PLDDS_DIAGNOSTICS_RATE_LIMIT`, defaults 20/min and 60/min) since
  those are the two endpoints that do real signature-verification work
  and DB writes; read endpoints share a looser `PLDDS_READ_RATE_LIMIT`
  (default 120/min). This is a backstop, not the primary defense — a
  fleet behind NAT can share an IP, so a reverse proxy/WAF should carry
  the serious rate limiting in a real deployment.
- **CORS narrowed**: `allow_headers` went from `"*"` to the exact set
  the dashboard's fetch client sends (`Content-Type`, `Accept`). Still
  GET-only, unchanged from Phase 11 — the dashboard has no
  write-capable credential and shouldn't.
- **Error responses sanitized**:
  - `RequestValidationError` (422s): in production, the raw submitted
    `input` value is stripped from the response body (FastAPI's default
    echoes it back per failed field, which can leak PII/secrets a
    device operator pasted into a field). Field path + message are kept
    so the response is still actionable. Full detail preserved outside
    production for local debugging.
  - Any unhandled exception now hits a catch-all handler that logs the
    real exception server-side and returns a flat, generic 500 to the
    caller — no stack traces, DB error text, or internal paths ever
    reach a client.
  - `/health`'s DB-unreachable path no longer risks leaking connection
    details through the exception message; it logs and returns a flat
    `"unreachable"` status, matching the schema's existing contract.
- **Docs/schema hidden in production**: `/docs`, `/redoc`, and
  `/openapi.json` are disabled when `PLDDS_ENV=production` — no reason
  to publish a machine-readable map of the API surface once the
  agent/dashboard integrations are stable.

## 5. Config additions (`server/app/config.py`)

All new behavior is env-driven, defaults preserve current (Phase 11)
behavior for local dev:

| Variable | Default | Purpose |
|---|---|---|
| `PLDDS_MAX_BODY_BYTES` | `2097152` (2 MiB) | Request body size cap |
| `PLDDS_REGISTER_RATE_LIMIT` | `20/minute` | Per-IP limit on `/devices/register` |
| `PLDDS_DIAGNOSTICS_RATE_LIMIT` | `60/minute` | Per-IP limit on `/diagnostics` |
| `PLDDS_READ_RATE_LIMIT` | `120/minute` | Per-IP limit on GET endpoints |
| `PLDDS_NONCE_RETENTION_SECONDS` | `600` | How long `cleanup_nonces.py` keeps used nonces |
| `PLDDS_TRUSTED_PROXY_HOPS` | `1` | Whether to trust forwarded-proto for HSTS decisions |

## 6. Explicitly out of scope this phase

- **Agent-side hardening of the boot/init flow** (Phase 9's territory)
  and **remaining collectors** (Phase 8) — unrelated attack surfaces,
  not touched here.
- **Secrets management / KMS for the device signing key** — Phase 9's
  "KNOWN LIMITATION" (key currently lives on tmpfs, regenerated every
  boot) is a durability problem, not directly a security regression;
  tracked separately, revisit at Phase 13 (real persistent storage).
- **WAF / DDoS protection at the network edge** — assumed to be the
  hosting platform's job; this phase's rate limiting is an
  application-level backstop only.
- **Full penetration test** — this is a self-review pass, not a
  substitute for third-party testing before any production rollout
  with real user devices.

## 7. Verifying this phase

```bash
cd server
pip install -r requirements.txt
pytest tests/test_security_hardening.py -v   # header-shape validation, no DB needed
```

For the full effect (rate limiting, body cap, headers) run the server
against a real Postgres and exercise it with `curl`/the agent — those
paths need the live app, not just unit tests.
