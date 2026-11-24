#!/usr/bin/env bash
# TM Trading Rust - Workspace Cleanup Utility
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "=========================================================="
echo "  TM TRADING - WORKSPACE CLEANUP UTILITY                 "
echo "=========================================================="

if [ -d "target" ]; then
    echo "[...] Cleaning Cargo build cache (cargo clean)..."
    cargo clean
    echo "[OK] Cargo target directory cleaned."
else
    echo "[INFO] Cargo target directory is already clean."
fi

# Clean temporary directories and logs
rm -rf target/tmp .tmp-qa .tmp-ui-audit *.log

if [ "${1:-}" = "--full" ]; then
    echo "[...] Removing dist/ release directory..."
    rm -rf dist
    echo "[OK] dist/ directory cleaned."
fi

echo "=========================================================="
echo "  CLEANUP COMPLETED SUCCESSFULLY!                         "
echo "=========================================================="
