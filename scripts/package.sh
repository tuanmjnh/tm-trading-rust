#!/usr/bin/env bash
# ==============================================================================
# TM Trading Rust - Universal Multi-Platform Build & Packaging Script
# Supports: Linux (x86_64, aarch64), macOS (Intel, Apple Silicon), and Windows (MSYS/Git Bash)
# ==============================================================================

set -euo pipefail

# Determine repository root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "=========================================================="
echo "  TM Trading Rust - Universal Build & Packaging Tool"
echo "=========================================================="

# 1. Detect Operating System and Architecture
OS_NAME="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH_NAME="$(uname -m)"

case "${ARCH_NAME}" in
  x86_64|amd64)
    ARCH_NORMALIZED="x64"
    ;;
  aarch64|arm64)
    ARCH_NORMALIZED="arm64"
    ;;
  *)
    ARCH_NORMALIZED="${ARCH_NAME}"
    ;;
esac

case "${OS_NAME}" in
  linux*)
    PLATFORM="linux-${ARCH_NORMALIZED}"
    EXE_EXT=""
    ARCHIVE_EXT="tar.gz"
    ;;
  darwin*)
    PLATFORM="macos-${ARCH_NORMALIZED}"
    EXE_EXT=""
    ARCHIVE_EXT="tar.gz"
    ;;
  msys*|mingw*|cygwin*)
    PLATFORM="windows-${ARCH_NORMALIZED}"
    EXE_EXT=".exe"
    ARCHIVE_EXT="zip"
    ;;
  *)
    PLATFORM="${OS_NAME}-${ARCH_NORMALIZED}"
    EXE_EXT=""
    ARCHIVE_EXT="tar.gz"
    ;;
esac

echo "Detected host platform: ${PLATFORM} (${OS_NAME} / ${ARCH_NAME})"

# 2. Extract version from Cargo.toml
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*version.*=.*"([^"]+)".*/\1/' || echo "0.1.0")
if [ -z "${VERSION}" ]; then
  VERSION="0.1.0"
fi
echo "Target release version: v${VERSION}"

# 3. Check prerequisites on Linux
if [[ "${PLATFORM}" == linux* ]]; then
  echo "[1/5] Checking Linux native build dependencies..."
  if ! command -v pkg-config >/dev/null 2>&1; then
    echo "Warning: pkg-config not found. Make sure build-essential/base-devel is installed."
  fi
fi

# 4. Build release binaries
SKIP_BUILD="${1:-}"
if [ "${SKIP_BUILD}" != "--skip-build" ]; then
  echo "[2/5] Compiling release binaries (cargo build --workspace --release)..."
  cargo build --workspace --release
else
  echo "[2/5] Skipping build step (--skip-build specified)..."
fi

# 5. Prepare staging directory
DIST_DIR="${REPO_ROOT}/dist"
STAGE_DIR="${DIST_DIR}/tm-trading-${PLATFORM}"
ARCHIVE_NAME="tm-trading-${PLATFORM}-v${VERSION}.${ARCHIVE_EXT}"
ARCHIVE_PATH="${DIST_DIR}/${ARCHIVE_NAME}"
SHA_PATH="${DIST_DIR}/${ARCHIVE_NAME}.sha256"

rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/bin"
mkdir -p "${STAGE_DIR}/data"

echo "[3/5] Collecting binaries into staging directory..."
BINARIES=(
  "tm-trading-desktop"
  "tm-trading-management-api"
  "tm-trading-agent"
  "tm-trading-market-worker"
  "tm-trading-news-worker"
  "tm-trading-ai-worker"
  "tm-trading-notification-worker"
  "tm-trading-research-worker"
  "tm-trading-sample-worker"
)

TARGET_RELEASE="${REPO_ROOT}/target/release"
for BIN in "${BINARIES[@]}"; do
  BIN_FILE="${BIN}${EXE_EXT}"
  SRC_PATH="${TARGET_RELEASE}/${BIN_FILE}"
  if [ -f "${SRC_PATH}" ]; then
    cp "${SRC_PATH}" "${STAGE_DIR}/bin/"
    chmod +x "${STAGE_DIR}/bin/${BIN_FILE}"
    echo "  + Included: ${BIN_FILE}"
  else
    echo "  ! Warning: Binary not found: ${SRC_PATH}"
  fi
done

# 6. Add configuration template & launch scripts
echo "[4/5] Adding launch scripts and configuration templates..."

cat << 'EOF' > "${STAGE_DIR}/.env.example"
# TM Trading Desktop & Local Services Configuration
TM_TRADING_API_URL=http://127.0.0.1:8787
TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token
TM_TRADING_AGENT_API_TOKEN=tm-local-agent-token
TM_TRADING_LOCALE=vi
TM_LOG_FORMAT=compact
TM_TRADING_DATA_DIR=data
EOF

cat << EOF > "${STAGE_DIR}/README.txt"
========================================================================
  TM Trading Desktop Control Center & Services (${PLATFORM})
  Version: v${VERSION}
========================================================================

SYSTEM PREREQUISITES:
  - Linux: webkit2gtk-4.1 / gtk3 packages installed.
  - macOS: macOS 11+ (Big Sur or newer).
  - Windows: Windows 10/11 x64 with Microsoft Edge WebView2 Evergreen.

QUICK START:
  1. Copy '.env.example' to '.env' and adjust tokens if desired.
  2. Run './start-all-services.sh' (or 'start-all-services.cmd' on Windows)
     to launch Management API and Desktop Control Center.
  3. Or run './start-desktop-only.sh' to connect to an existing server.

SECURITY & PRODUCT BOUNDARY:
  - This system is strictly read-only and information-first.
  - Live order execution and broker trading credentials are intentionally omitted.
  - Management API binds strictly to loopback (127.0.0.1).
========================================================================
EOF

if [[ "${PLATFORM}" == windows* ]]; then
  # Windows batch files
  cat << 'EOF' > "${STAGE_DIR}/start-all-services.cmd"
@echo off
title TM Trading Desktop & Services
setlocal
cd /d "%~dp0"
if exist ".env" (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        set "%%A=%%B"
    )
)
if "%TM_TRADING_API_URL%"=="" set "TM_TRADING_API_URL=http://127.0.0.1:8787"
if "%TM_TRADING_LOCAL_API_TOKEN%"=="" set "TM_TRADING_LOCAL_API_TOKEN=tm-local-dev-token"

echo Starting Management API Server in background...
start "TM Trading Management API" /min "%~dp0bin\tm-trading-management-api.exe"
timeout /t 2 /nobreak >nul
echo Launching TM Trading Control Center...
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
EOF

  cat << 'EOF' > "${STAGE_DIR}/start-desktop-only.cmd"
@echo off
title TM Trading Control Center
setlocal
cd /d "%~dp0"
start "" "%~dp0bin\tm-trading-desktop.exe"
exit /b 0
EOF
else
  # POSIX shell scripts for Linux & macOS
  cat << 'EOF' > "${STAGE_DIR}/start-all-services.sh"
#!/usr/bin/env bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${DIR}"

if [ -f ".env" ]; then
  export $(grep -v '^#' .env | xargs)
fi

export TM_TRADING_API_URL="${TM_TRADING_API_URL:-http://127.0.0.1:8787}"
export TM_TRADING_LOCAL_API_TOKEN="${TM_TRADING_LOCAL_API_TOKEN:-tm-local-dev-token}"

echo "Starting Management API server in background..."
"${DIR}/bin/tm-trading-management-api" > "${DIR}/api.log" 2>&1 &
API_PID=$!
echo "Management API PID: ${API_PID}"

sleep 2

echo "Launching TM Trading Desktop..."
"${DIR}/bin/tm-trading-desktop"
EOF
  chmod +x "${STAGE_DIR}/start-all-services.sh"

  cat << 'EOF' > "${STAGE_DIR}/start-desktop-only.sh"
#!/usr/bin/env bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"${DIR}/bin/tm-trading-desktop"
EOF
  chmod +x "${STAGE_DIR}/start-desktop-only.sh"
fi

# 7. Compress into archive
echo "[5/5] Packaging distribution archive (${ARCHIVE_NAME})..."
rm -f "${ARCHIVE_PATH}"

if [ "${ARCHIVE_EXT}" = "zip" ]; then
  if command -v zip >/dev/null 2>&1; then
    (cd "${STAGE_DIR}" && zip -q -r "${ARCHIVE_PATH}" .)
  else
    powershell.exe -Command "Compress-Archive -Path '${STAGE_DIR}/*' -DestinationPath '${ARCHIVE_PATH}' -Force"
  fi
else
  tar -czf "${ARCHIVE_PATH}" -C "${DIST_DIR}" "tm-trading-${PLATFORM}"
fi

# 8. Calculate SHA-256 Checksum
if command -v sha256sum >/dev/null 2>&1; then
  (cd "${DIST_DIR}" && sha256sum "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "${DIST_DIR}" && shasum -a 256 "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256")
else
  powershell.exe -Command "(Get-FileHash -Path '${ARCHIVE_PATH}' -Algorithm SHA256).Hash + '  ${ARCHIVE_NAME}'" > "${SHA_PATH}"
fi

echo "=========================================================="
echo "  Packaging Complete!"
echo "  Platform: ${PLATFORM}"
echo "  Archive:  ${ARCHIVE_PATH}"
echo "  Checksum: ${SHA_PATH}"
echo "=========================================================="
