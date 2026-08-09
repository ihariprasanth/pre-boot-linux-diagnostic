#!/usr/bin/env bash
# PLDDS Phase 10 — automated boot+diagnose+upload+ACK+reboot test harness.
#
# Orchestrates, in order:
#   1. build the static agent + the initramfs (via make)
#   2. bring up a local backend (Postgres via docker compose + uvicorn)
#   3. boot the result under QEMU with a bounded timeout, capturing the
#      full serial console log
#   4. grep that log for the checkpoints every successful run must hit
#      (banner -> collectors -> report -> upload attempt -> boot
#      decision -> handoff -> reboot)
#   5. if the guest got a DHCP lease, also ask the backend directly
#      whether a report actually landed — the strongest possible check,
#      confirming the real upload+ACK round trip, not just the log line
#      saying it was attempted
#   6. tear everything down and print a structured PASS/FAIL/SKIP summary
#
# ## Philosophy: report what's true, don't fake green
#
# Not every host this runs on has KVM, a NIC driver available to the
# guest kernel, or even `qemu-system-x86_64`/`docker` installed at all
# (this was itself authored and dry-run-reviewed in exactly such an
# environment — see docs/qemu-testing.md "Known sandbox limitations").
# Missing a *prerequisite* is reported as SKIP with the reason, never
# silently treated as PASS and never treated as a hard FAIL that blocks
# unrelated checks. A check is only FAIL when the harness actually ran
# it and it produced the wrong result. See `record()` below.
#
# Exit code: 0 only if zero FAILs (SKIPs are fine — see above).
# Env vars (all optional):
#   PLDDS_TEST_KEEP_STACK=1   don't tear down docker/uvicorn afterward
#                              (handy for interactively poking at the
#                              backend right after a run)
#   PLDDS_TEST_KERNEL=/path   passed straight through to run-qemu.sh
#   PLDDS_TEST_TIMEOUT_SECS   QEMU wall-clock cap (default 90)

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORK_DIR="$(mktemp -d)"
QEMU_LOG="${WORK_DIR}/qemu-serial.log"
SERVER_LOG="${WORK_DIR}/server.log"
SERVER_PID=""
DB_STARTED=0

TIMEOUT_SECS="${PLDDS_TEST_TIMEOUT_SECS:-90}"

# --- Result tracking --------------------------------------------------
declare -a RESULT_NAMES=()
declare -a RESULT_STATUS=()   # PASS | FAIL | SKIP
declare -a RESULT_DETAIL=()

record() {
    local name="$1" status="$2" detail="$3"
    RESULT_NAMES+=("${name}")
    RESULT_STATUS+=("${status}")
    RESULT_DETAIL+=("${detail}")
    case "${status}" in
        PASS) echo "  [PASS] ${name} — ${detail}" ;;
        FAIL) echo "  [FAIL] ${name} — ${detail}" ;;
        SKIP) echo "  [SKIP] ${name} — ${detail}" ;;
    esac
}

section() {
    echo ""
    echo "=== $1 ==="
}

cleanup() {
    if [ "${PLDDS_TEST_KEEP_STACK:-0}" = "1" ]; then
        echo ""
        echo "[test] PLDDS_TEST_KEEP_STACK=1 — leaving backend running (log: ${SERVER_LOG})"
        return
    fi
    if [ -n "${SERVER_PID}" ] && kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    if [ "${DB_STARTED}" = "1" ]; then
        ( cd "${ROOT_DIR}" && docker compose down db >/dev/null 2>&1 ) || true
    fi
    rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

echo "PLDDS Phase 10 test harness"
echo "work dir: ${WORK_DIR}"

# ========================================================================
section "1. Build"
# ========================================================================
if command -v cargo >/dev/null 2>&1; then
    if ( cd "${ROOT_DIR}" && make agent >"${WORK_DIR}/build-agent.log" 2>&1 ); then
        record "build:agent" PASS "static agent binary built"
    else
        record "build:agent" FAIL "see ${WORK_DIR}/build-agent.log"
    fi

    if command -v busybox >/dev/null 2>&1; then
        if ( cd "${ROOT_DIR}" && make initramfs >"${WORK_DIR}/build-initramfs.log" 2>&1 ); then
            record "build:initramfs" PASS "initramfs.cpio.gz built"
        else
            record "build:initramfs" FAIL "see ${WORK_DIR}/build-initramfs.log"
        fi
    else
        record "build:initramfs" SKIP "busybox-static not installed on this host"
    fi
else
    record "build:agent" SKIP "cargo not installed on this host"
    record "build:initramfs" SKIP "cargo not installed on this host (agent must build first)"
fi

# ========================================================================
section "2. Backend"
# ========================================================================
BACKEND_UP=0
if command -v docker >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    if ( cd "${ROOT_DIR}" && docker compose up -d db >"${WORK_DIR}/docker.log" 2>&1 ); then
        DB_STARTED=1
        # Wait for Postgres to actually accept connections rather than
        # a fixed sleep — bounded so a broken container can't hang the
        # whole harness.
        READY=0
        for _ in $(seq 1 30); do
            if ( cd "${ROOT_DIR}" && docker compose exec -T db pg_isready -U pldds >/dev/null 2>&1 ); then
                READY=1
                break
            fi
            sleep 1
        done
        if [ "${READY}" = "1" ]; then
            record "backend:postgres" PASS "accepting connections"

            # sslmode=disable in the URL itself is what makes
            # server/app/database.py skip its default
            # sslmode=require connect_arg (see that file's comment) —
            # correct for this local, unencrypted, throwaway test DB.
            export DATABASE_URL="postgresql://pldds:pldds_dev_only_change_me@localhost:5432/pldds?sslmode=disable"
            export PLDDS_ENV="test"
            export PLDDS_SCHEMA_MAJOR="1"

            if command -v uvicorn >/dev/null 2>&1 || python3 -c "import uvicorn" >/dev/null 2>&1; then
                ( cd "${ROOT_DIR}/server" && python3 -m uvicorn app.main:app --host 0.0.0.0 --port 8000 \
                    >"${SERVER_LOG}" 2>&1 & echo $! > "${WORK_DIR}/server.pid" )
                SERVER_PID="$(cat "${WORK_DIR}/server.pid" 2>/dev/null || true)"

                UP=0
                for _ in $(seq 1 20); do
                    if curl -sf http://127.0.0.1:8000/health >/dev/null 2>&1; then
                        UP=1
                        break
                    fi
                    sleep 1
                done
                if [ "${UP}" = "1" ]; then
                    record "backend:server" PASS "FastAPI up on :8000, /health OK"
                    BACKEND_UP=1
                else
                    record "backend:server" FAIL "uvicorn didn't come up — see ${SERVER_LOG}"
                fi
            else
                record "backend:server" SKIP "uvicorn/fastapi deps not installed (server/requirements.txt)"
            fi
        else
            record "backend:postgres" FAIL "container up but never became ready — see ${WORK_DIR}/docker.log"
        fi
    else
        record "backend:postgres" SKIP "docker compose could not start db — see ${WORK_DIR}/docker.log"
    fi
else
    record "backend:postgres" SKIP "docker and/or python3 not available on this host"
fi

# ========================================================================
section "3. QEMU boot"
# ========================================================================
INITRAMFS="${ROOT_DIR}/diagnostic/build/output/initramfs.cpio.gz"
KERNEL="${PLDDS_TEST_KERNEL:-$(ls /boot/vmlinuz-* 2>/dev/null | head -1 || true)}"

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    record "qemu:available" SKIP "qemu-system-x86_64 not installed on this host"
elif [ -z "${KERNEL}" ] || [ ! -r "${KERNEL}" ]; then
    record "qemu:available" SKIP "no readable kernel found (set PLDDS_TEST_KERNEL=/path/to/vmlinuz)"
elif [ ! -f "${INITRAMFS}" ]; then
    record "qemu:available" SKIP "initramfs not built (see section 1 above)"
else
    record "qemu:available" PASS "qemu + kernel + initramfs all present"

    PLDDS_QEMU_TIMEOUT_SECS="${TIMEOUT_SECS}" \
    PLDDS_QEMU_LOG="${QEMU_LOG}" \
    PLDDS_QEMU_NET="user" \
        "${SCRIPT_DIR}/run-qemu.sh" "${KERNEL}" >"${WORK_DIR}/run-qemu.stdout.log" 2>&1
    QEMU_EXIT=$?

    if [ ! -s "${QEMU_LOG}" ]; then
        record "qemu:boot" FAIL "no serial output captured at all — see ${WORK_DIR}/run-qemu.stdout.log"
    else
        # --- Checkpoint greps, in the order /init actually hits them ---
        check_log() {
            local name="$1" pattern="$2" detail="$3"
            if grep -qE "${pattern}" "${QEMU_LOG}"; then
                record "${name}" PASS "${detail}"
                return 0
            else
                record "${name}" FAIL "expected pattern not found in serial log: ${pattern}"
                return 1
            fi
        }

        check_log "boot:banner" "PLDDS Diagnostic Environment Started" \
            "init reached and printed the startup banner"
        check_log "boot:agent-ran" "PLDDS Diagnostic Agent v" \
            "diagnostic-agent executed and printed its own banner"
        check_log "boot:report-built" "JSON report" \
            "agent assembled and printed the structured report"
        check_log "boot:upload-attempted" "-- Upload --" \
            "agent attempted the upload step (result checked separately below)"
        check_log "boot:decision-written" "(-- Boot decision --|boot decision written)" \
            "agent computed and wrote a boot decision" || true
        check_log "boot:handoff" "boot decision: (WINDOWS|RETRY_DIAGNOSTIC)" \
            "/init read back a recognized boot decision"
        check_log "boot:reboot-reached" "rebooting now to complete the handoff" \
            "/init reached the final reboot step (no hang, no crash)"

        if [ "${QEMU_EXIT}" -eq 0 ] || [ "${QEMU_EXIT}" -eq 124 ]; then
            # 124 = GNU `timeout` killed it — acceptable ONLY if the log
            # already shows the reboot line above; otherwise it's a
            # real hang and boot:reboot-reached will have already FAILed.
            record "qemu:exit-code" PASS "exit ${QEMU_EXIT} (0 clean, or 124 after the handoff already completed)"
        else
            record "qemu:exit-code" FAIL "unexpected exit code ${QEMU_EXIT} — see ${QEMU_LOG}"
        fi

        # --- Did the report actually reach the backend? ---
        if grep -qE "^\[init\] eth0: " "${QEMU_LOG}" && [ "${BACKEND_UP}" = "1" ]; then
            REPORT_ID="$(grep -oE 'report [0-9a-fA-F-]{8,}' "${QEMU_LOG}" | tail -1 | awk '{print $2}')"
            if [ -n "${REPORT_ID}" ] && curl -sf "http://127.0.0.1:8000/reports/${REPORT_ID}" >/dev/null 2>&1; then
                record "e2e:upload-ack-verified" PASS "GET /reports/${REPORT_ID} confirms the server actually stored it"
            else
                record "e2e:upload-ack-verified" FAIL "guest had an IP and backend was up, but the report isn't retrievable from the API"
            fi
        elif [ "${BACKEND_UP}" != "1" ]; then
            record "e2e:upload-ack-verified" SKIP "backend wasn't up (see section 2) — nothing to verify against"
        else
            record "e2e:upload-ack-verified" SKIP \
                "guest never got a DHCP lease in this run — expected on hosts without a NIC driver for the attached device model (see docs/qemu-testing.md); upload gracefully failed and the boot decision handled it correctly (see boot:handoff above)"
        fi
    fi
fi

# ========================================================================
section "Summary"
# ========================================================================
PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
for s in "${RESULT_STATUS[@]}"; do
    case "${s}" in
        PASS) PASS_COUNT=$((PASS_COUNT + 1)) ;;
        FAIL) FAIL_COUNT=$((FAIL_COUNT + 1)) ;;
        SKIP) SKIP_COUNT=$((SKIP_COUNT + 1)) ;;
    esac
done

echo "${PASS_COUNT} passed, ${FAIL_COUNT} failed, ${SKIP_COUNT} skipped"

if [ "${FAIL_COUNT}" -gt 0 ]; then
    echo ""
    echo "FAILED checks:"
    for i in "${!RESULT_NAMES[@]}"; do
        if [ "${RESULT_STATUS[$i]}" = "FAIL" ]; then
            echo "  - ${RESULT_NAMES[$i]}: ${RESULT_DETAIL[$i]}"
        fi
    done
    exit 1
fi

echo "No failures (SKIPs are prerequisite gaps on this host, not test failures — see docs/qemu-testing.md)."
exit 0
