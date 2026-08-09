"""
Phase 12: security hardening middleware.

Two concerns, kept separate from app/security.py (which handles
per-device signature/replay auth) because these apply to *every*
request regardless of which endpoint or auth path it hits:

  - SecurityHeadersMiddleware: sets the standard set of defensive
    response headers. The API has no HTML surface of its own, but the
    dashboard renders API-adjacent data and a misconfigured proxy could
    still end up serving this API's responses somewhere a browser
    interprets them — cheap insurance.
  - BodySizeLimitMiddleware: rejects oversized request bodies before
    they're buffered/parsed, ahead of any signature verification or
    Pydantic validation. Without this, an attacker (or a buggy client)
    can send an arbitrarily large body and force the server to read all
    of it into memory before anything else gets a chance to reject it.
"""
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import JSONResponse, Response


class SecurityHeadersMiddleware(BaseHTTPMiddleware):
    def __init__(self, app, *, hsts: bool = False):
        super().__init__(app)
        # Only send HSTS when we're confident the deployment is actually
        # terminating/forwarding HTTPS (Settings.is_production AND a
        # trusted proxy hop configured) — sending it over plain HTTP in
        # dev would make browsers force-upgrade localhost and break the
        # dev loop.
        self._hsts = hsts

    async def dispatch(self, request: Request, call_next):
        response: Response = await call_next(request)

        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-Frame-Options"] = "DENY"
        response.headers["Referrer-Policy"] = "no-referrer"
        # This API serves JSON only — no reason any embedded content or
        # third-party script should ever run "as" it.
        response.headers["Content-Security-Policy"] = "default-src 'none'; frame-ancestors 'none'"
        response.headers["Permissions-Policy"] = (
            "geolocation=(), microphone=(), camera=(), payment=()"
        )
        # Server header left at Starlette/uvicorn's default is fine to
        # remove — no need to advertise stack details.
        if "server" in response.headers:
            del response.headers["server"]

        if self._hsts:
            response.headers["Strict-Transport-Security"] = (
                "max-age=63072000; includeSubDomains; preload"
            )

        return response


class BodySizeLimitMiddleware(BaseHTTPMiddleware):
    def __init__(self, app, *, max_bytes: int):
        super().__init__(app)
        self.max_bytes = max_bytes

    async def dispatch(self, request: Request, call_next):
        content_length = request.headers.get("content-length")
        if content_length is not None:
            try:
                if int(content_length) > self.max_bytes:
                    return JSONResponse(
                        status_code=413,
                        content={"detail": "request body too large"},
                    )
            except ValueError:
                # Malformed Content-Length — let downstream parsing reject it
                # rather than guessing; fall through to the streaming guard.
                pass

        # Content-Length can be absent/spoofed (chunked transfer, or a
        # lying client) — enforce the real limit as bytes stream in too.
        body_too_large = False
        original_receive = request.receive
        seen = 0

        async def limited_receive():
            nonlocal seen, body_too_large
            message = await original_receive()
            if message["type"] == "http.request":
                seen += len(message.get("body", b""))
                if seen > self.max_bytes:
                    body_too_large = True
            return message

        request._receive = limited_receive  # noqa: SLF001 — Starlette's supported override point

        response = await call_next(request)
        if body_too_large:
            return JSONResponse(status_code=413, content={"detail": "request body too large"})
        return response
