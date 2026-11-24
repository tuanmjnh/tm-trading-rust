#!/usr/bin/env bash
# Starts the full TM Trading development stack (Backend Management API + Desktop UI)
set -eo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

LOCALE="${1:-vi}"
API_URL="${TM_TRADING_API_URL:-http://127.0.0.1:8787}"
API_TOKEN="${TM_TRADING_LOCAL_API_TOKEN:-local-dev-operator-token}"

export TM_TRADING_LOCALE="$LOCALE"
export TM_TRADING_API_URL="$API_URL"
export TM_TRADING_LOCAL_API_TOKEN="$API_TOKEN"
export TM_TRADING_AGENT_TOKEN="local-dev-agent-token"
export TM_TRADING_LOG_FORMAT="compact"
export RUST_LOG="info,tm_trading_desktop=info"

# Shared local MongoDB database environment
export TM_DATABASE_URI="mongodb://127.0.0.1:27017"
export TM_DATABASE_NAME="tm-trading-rust"
export TM_STORAGE_ALLOW_INSECURE_DEV="1"
export TM_TRADING_STORE="mongodb"

# Worker write permissions and default telemetry targets
export TM_TRADING_MARKET_SOURCE_OF_TRUTH="1"
export TM_TRADING_MARKET_SYMBOLS="BTCUSDT,ETHUSDT,SOLUSDT"
export TM_TRADING_RESEARCH_OWNER_SCOPE="default"
export TM_TRADING_RESEARCH_SOURCE_OF_TRUTH="1"
export TM_TRADING_RESEARCH_SYMBOLS="BTCUSDT,ETHUSDT,SOLUSDT"
export TM_TRADING_NEWS_SOURCE_OF_TRUTH="1"

echo "=========================================================="
echo "  TM TRADING - DESKTOP & API DEV ENVIRONMENT RUNNER (UNIX)"
echo "=========================================================="

API_PID=""
cleanup() {
    if [ -n "$API_PID" ] && kill -0 "$API_PID" 2>/dev/null; then
        echo "[CLEANUP] Stopping background Management API ($API_PID)..."
        kill "$API_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

# 1. Start API if port 8787 is not listening
if nc -z 127.0.0.1 8787 2>/dev/null; then
    echo "[OK] Management API is already running on 127.0.0.1:8787"
else
    echo "[...] Starting Management API on 127.0.0.1:8787..."
    cargo run -p tm-trading-management-api &
    API_PID=$!
    echo "[...] Waiting for API to listen on 8787..."
    for i in {1..30}; do
        if nc -z 127.0.0.1 8787 2>/dev/null; then
            echo "[OK] Management API is ready!"
            break
        fi
        sleep 0.8
    done
fi

export TM_TRADING_MARKET_WORKER_ENABLED="1"
export TM_TRADING_RESEARCH_WORKER_ENABLED="1"
export TM_TRADING_RESEARCH_METHODS="vsa,price-action,trend,wonderwhales,sweep"
export TM_TRADING_NEWS_WORKER_ENABLED="1"
export TM_TRADING_AI_WORKER_ENABLED="1"
export TM_TRADING_NOTIFICATION_WORKER_ENABLED="1"

cargo run -p tm-trading-agent -- --api-url http://127.0.0.1:8787 --start sample-worker &
AGENT_PID=$!
cleanup() {
    if [ -n "$API_PID" ] && kill -0 "$API_PID" 2>/dev/null; then
        echo "[CLEANUP] Stopping background Management API ($API_PID)..."
        kill "$API_PID" 2>/dev/null || true
    fi
    if [ -n "$AGENT_PID" ] && kill -0 "$AGENT_PID" 2>/dev/null; then
        echo "[CLEANUP] Stopping background Host Agent ($AGENT_PID)..."
        kill "$AGENT_PID" 2>/dev/null || true
    fi
}

# 2. Run Desktop UI
if command -v dx >/dev/null 2>&1; then
    echo "[DX] Using Dioxus CLI with native Hot-Reload!"
    cd "$REPO_ROOT/apps/desktop"
    dx serve --platform desktop
else
    echo "[RUN] Launching Desktop UI (cargo run -p tm-trading-desktop)..."
    cargo run -p tm-trading-desktop
fi
