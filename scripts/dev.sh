#!/usr/bin/env bash
# PLDDS local dev stack: Postgres (docker) + FastAPI (uvicorn --reload) +
# dashboard (vite dev server). Foreground process is the dashboard so
# Ctrl-C naturally ends the session; the trap tears the rest down.
#
# Same philosophy as scripts/test.sh: a missing prerequisite is reported
# and skipped, never silently treated as fine.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

SERVER_PID=""
DB_STARTED=0

cleanup() {
    echo ""
    echo "[dev] shutting down..."
    if [ -n "${SERVER_PID}" ] && kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    if [ "${DB_STARTED}" = "1" ]; then
        ( cd "${ROOT_DIR}" && docker compose down db >/dev/null 2>&1 ) || true
    fi
}
trap cleanup EXIT INT TERM

echo "PLDDS local dev stack"

# --- 1. Postgres ---------------------------------------------------------
if command -v docker >/dev/null 2>&1; then
    echo "[dev] starting Postgres (docker compose up -d db)..."
    if ( cd "${ROOT_DIR}" && docker compose up -d db ); then
        DB_STARTED=1
        for _ in $(seq 1 30); do
            if ( cd "${ROOT_DIR}" && docker compose exec -T db pg_isready -U pldds >/dev/null 2>&1 ); then
                break
            fi
            sleep 1
        done
        echo "[dev] Postgres up on :5432"
    else
        echo "[dev] docker compose could not start db — continuing without it (server will fail to start)"
    fi
else
    echo "[dev] docker not found — skipping Postgres. Set DATABASE_URL yourself if you have one running elsewhere."
fi

# --- 2. Backend -----------------------------------------------------------
if python3 -c "import uvicorn" >/dev/null 2>&1; then
    export DATABASE_URL="${DATABASE_URL:-postgresql://pldds:pldds_dev_only_change_me@localhost:5432/pldds?sslmode=disable}"
    export PLDDS_ENV="${PLDDS_ENV:-development}"
    export PLDDS_SCHEMA_MAJOR="${PLDDS_SCHEMA_MAJOR:-1}"
    export PLDDS_DASHBOARD_ORIGINS="${PLDDS_DASHBOARD_ORIGINS:-http://localhost:3000}"

    echo "[dev] starting FastAPI (uvicorn --reload) on :8000..."
    ( cd "${ROOT_DIR}/server" && python3 -m uvicorn app.main:app --host 0.0.0.0 --port 8000 --reload ) &
    SERVER_PID=$!

    for _ in $(seq 1 20); do
        if curl -sf http://127.0.0.1:8000/health >/dev/null 2>&1; then
            echo "[dev] backend up: http://localhost:8000 (docs: /docs)"
            break
        fi
        sleep 1
    done
else
    echo "[dev] server/requirements.txt not installed (uvicorn missing) — skipping backend."
    echo "      run: pip install -r server/requirements.txt"
fi

# --- 3. Dashboard -----------------------------------------------------------
if command -v npm >/dev/null 2>&1; then
    echo "[dev] starting dashboard (vite) on :3000 — this is the foreground process, Ctrl-C to stop everything"
    ( cd "${ROOT_DIR}/dashboard" && npm install && npm run dev )
else
    echo "[dev] npm not found — cannot start the dashboard. Install Node.js, then: cd dashboard && npm install && npm run dev"
    echo "[dev] backend will keep running in the foreground instead; Ctrl-C to stop."
    wait "${SERVER_PID}" 2>/dev/null || true
fi
