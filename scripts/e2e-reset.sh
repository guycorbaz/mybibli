#!/usr/bin/env bash
# Reset the E2E docker stack to a pristine state.
#
# Epic 7 retrospective Action 4 (2026-04-17) — introduced after story 7-5
# discovery that an 11-hour-old stack with accumulated DB data broke 51
# tests on a single run. This script gives the dev-loop a one-shot reset
# equivalent to what CI does on every PR.
#
# Usage:
#     ./scripts/e2e-reset.sh
#
# What it does:
#   1. Takes down the E2E stack AND its volumes (-v), so the DB is wiped.
#   2. Rebuilds the `mybibli` image from the current working tree.
#   3. Brings the stack back up detached.
#   4. Waits for the app to answer `GET /login` before returning 0.
#
# Safe to run at any time. Does NOT touch the dev stack (docker-compose.yml
# in project root) — only tests/e2e/docker-compose.test.yml. Local dev DB
# state is preserved.
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${PROJECT_ROOT}/tests/e2e/docker-compose.test.yml"
APP_URL="${E2E_APP_URL:-http://localhost:8080/login}"
WAIT_TIMEOUT="${E2E_WAIT_TIMEOUT:-120}"

if [[ ! -f "${COMPOSE_FILE}" ]]; then
    echo "error: ${COMPOSE_FILE} not found — run from the project tree."
    exit 1
fi

echo "→ Tearing down E2E stack and removing volumes..."
docker compose -f "${COMPOSE_FILE}" down -v

echo "→ Rebuilding mybibli image + starting stack..."
docker compose -f "${COMPOSE_FILE}" up -d --build mybibli

echo "→ Waiting for app at ${APP_URL} (timeout ${WAIT_TIMEOUT}s)..."
elapsed=0
until curl -sf "${APP_URL}" > /dev/null 2>&1; do
    if (( elapsed >= WAIT_TIMEOUT )); then
        echo "error: app did not become ready within ${WAIT_TIMEOUT}s."
        echo "     Last compose state:"
        docker compose -f "${COMPOSE_FILE}" ps
        exit 1
    fi
    sleep 2
    elapsed=$(( elapsed + 2 ))
done

echo "✓ E2E stack reset — app ready at ${APP_URL%/login}"
