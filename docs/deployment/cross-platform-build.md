# TM Trading Rust - Cross-Platform Build & Packaging Guide (Windows, Linux, macOS)

> Status: Complete as of Phase 9 (2026-11-23).<br>
> Authority: [`docs/TM-TRADING-RUST-MASTER-ROADMAP.md`](../TM-TRADING-RUST-MASTER-ROADMAP.md)

---

## 1. Overview & Multi-Platform Support Matrix

The TM Trading Rust codebase is designed for high-performance, cross-platform compilation. Every component compiles cleanly with standard Rust (`cargo`):

| Component | Target Binary | Windows (x64) | Linux (x64 / aarch64) | macOS (Intel / Apple Silicon) |
| :--- | :--- | :---: | :---: | :---: |
| **Desktop Control Center** | `tm-trading-desktop` | WebView2 | WebKitGTK 4.1 | WebKit (Cocoa) |
| **Management API Server** | `tm-trading-management-api` | Win32 Sockets | Linux epoll | macOS kqueue |
| **Supervised Host Agent** | `tm-trading-agent` | Job Object | Process Groups | Process Groups |
| **Market Worker** | `tm-trading-market-worker` | Native | Native | Native |
| **News Worker** | `tm-trading-news-worker` | Native | Native | Native |
| **AI Worker** | `tm-trading-ai-worker` | Native | Native | Native |
| **Notification Worker** | `tm-trading-notification-worker` | Native | Native | Native |
| **Research Worker** | `tm-trading-research-worker` | Native | Native | Native |

---

## 2. One-Click Packaging Scripts

The repository includes universal build and packaging automation scripts in the [`scripts/`](../../scripts) directory:

### On Linux or macOS (Bash)
```bash
# Make script executable (first time):
chmod +x ./scripts/package.sh

# Run compilation and packaging:
./scripts/package.sh

# Skip build if binaries are already compiled:
./scripts/package.sh --skip-build
```

### On Windows (PowerShell)
```powershell
# In Windows PowerShell or PowerShell 7+:
.\scripts\package.ps1

# Or skip build if already compiled:
.\scripts\package.ps1 -SkipBuild
```

The script automatically detects the host OS and architecture, compiles release binaries, stages configuration templates, creates an archive (`.zip` on Windows, `.tar.gz` on Linux/macOS), and calculates the SHA-256 integrity checksum into `dist/`.

---

## 3. Platform-Specific Prerequisites

### A. Windows (x86_64)
1. **Rust Toolchain**: `x86_64-pc-windows-msvc` (Visual Studio C++ Build Tools or Community Edition).
2. **WebView2 Runtime**: Microsoft Edge WebView2 Evergreen (pre-installed on Windows 11 and modern Windows 10; [Bootstrapper Link](https://go.microsoft.com/fwlink/p/?LinkId=2124703) if missing).
3. **Build Command**:
   ```powershell
   cargo build --workspace --release
   ```
   Binaries are produced in `target\release\*.exe`.

---

### B. Linux (Ubuntu / Debian / Fedora / Arch)
Dioxus Desktop uses `wry` which requires GTK3 and WebKitGTK development packages.

#### 1. Install System Dependencies:
- **Ubuntu / Debian / Linux Mint**:
  ```bash
  sudo apt-get update
  sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev build-essential pkg-config libssl-dev
  ```
- **Fedora / RHEL**:
  ```bash
  sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel gcc pkgconf-pkg-config openssl-devel
  ```
- **Arch Linux / Manjaro**:
  ```bash
  sudo pacman -S webkit2gtk-4.1 gtk3 libayatana-appindicator base-devel pkgconf openssl
  ```

#### 2. Build:
```bash
cargo build --workspace --release
```
Binaries are produced in `target/release/` (e.g. `tm-trading-desktop`, `tm-trading-management-api`).

---

### C. macOS (Apple Silicon M1/M2/M3/M4 & Intel x86_64)
Dioxus Desktop uses Apple's native WebKit framework via Cocoa; no extra third-party libraries are required.

#### 1. Install Xcode Command Line Tools:
```bash
xcode-select --install
```

#### 2. Build Native Binary:
```bash
# Apple Silicon (aarch64-apple-darwin) or Intel (x86_64-apple-darwin):
cargo build --workspace --release
```

#### 3. Build Universal Binary (Optional, supporting both Intel & Apple Silicon):
```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --workspace --release --target aarch64-apple-darwin
cargo build --workspace --release --target x86_64-apple-darwin
lipo -create -output target/release/tm-trading-desktop \
  target/aarch64-apple-darwin/release/tm-trading-desktop \
  target/x86_64-apple-darwin/release/tm-trading-desktop
```

---

## 4. Output Executable Binaries Reference

All executables reside in `target/release/` (or `dist/tm-trading-<platform>/bin/`):

1. **`tm-trading-desktop`** (`.exe` on Windows): Dioxus Desktop Control Center with bilingual EN/VI UI.
2. **`tm-trading-management-api`** (`.exe` on Windows): Loopback HTTP/REST management and metrics server.
3. **`tm-trading-agent`** (`.exe` on Windows): Supervisor host agent managing worker life-cycles.
4. **`tm-trading-market-worker`** (`.exe` on Windows): Closed public bar ingestion worker.
5. **`tm-trading-news-worker`** (`.exe` on Windows): Rights-safe news feed worker.
6. **`tm-trading-ai-worker`** (`.exe` on Windows): Multi-provider AI reasoning & briefing worker.
7. **`tm-trading-notification-worker`** (`.exe` on Windows): Multichannel Telegram & audit worker.
8. **`tm-trading-research-worker`** (`.exe` on Windows): VSA & technical indicator scanning worker.
9. **`tm-trading-sample-worker`** (`.exe` on Windows): Reference IPC protocol worker.
