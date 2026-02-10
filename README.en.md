# CodexManager

A local desktop + service toolkit for managing a Codex-compatible ChatGPT account pool. It helps you manage accounts, usage, and platform keys, and provides a local gateway/service for tools like Codex CLI.

[中文](README.md)

## Overview
- Desktop app (Tauri): account management, usage dashboard, OAuth login, platform key management
- Service (Rust): local RPC + gateway, usage polling/refresh, account selection/failover
- Supports manual parsing of OAuth callback URLs to avoid port conflicts or callback failures

## Key Features
- Account pool management: grouping/tags/sorting/notes
- Usage dashboard: 5-hour and 7-day usage snapshots
- OAuth login: browser flow + manual callback parsing
- Platform keys: create/disable/delete
- Local service: auto-start, customizable port
- Gateway: unified local entry for CLI/tools

## Screenshots
![Dashboard](assets/images/dashboard.png)
![Accounts](assets/images/accounts.png)
![Platform Key](assets/images/platform-key.png)
![Logs](assets/images/log.png)
![Themes](assets/images/themes.png)

## Tech Stack
- Frontend: Vite + vanilla JS
- Desktop: Tauri (Rust)
- Service: Rust (local HTTP/RPC + gateway)

## Project Structure
```
.
├─ apps/                # Frontend + Tauri desktop app
│  ├─ src/              # Frontend source
│  ├─ src-tauri/        # Tauri source
│  └─ dist/             # Frontend build output
├─ crates/              # Rust core + service
│  ├─ gpttools-core
│  └─ gpttools-service
├─ assets/images/       # Screenshots (GitHub previewable)
├─ portable/            # Portable build output
├─ rebuild.ps1          # Build script
└─ README.md
```

## Build & Packaging
### Frontend dev
```
pnpm run dev
```

### Frontend build
```
pnpm run build
```

### Build Rust service only
```
cargo build -p gpttools-service --release
```

Output:
- `target/release/gpttools-service.exe`

### Build Tauri bundles
```
.\scripts\rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

Artifacts:
- Installer bundles: `apps/src-tauri/target/release/bundle/`
- Portable build: `portable/`

## Multi-platform build scripts
Note: Run each script on its target OS.

### Prerequisites
- Node.js 20+
- pnpm 9+ (recommended via `corepack enable`)
- Rust stable (`rustup default stable`)
- Tauri CLI (`cargo install tauri-cli --locked`)

### Platform-specific dependencies
- Windows: Visual Studio C++ Build Tools (with Windows SDK)
- Linux (Ubuntu 22.04+):
```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libsoup-3.0-dev
```
- macOS:
```bash
xcode-select --install
```

### Windows
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Linux
```bash
chmod +x scripts/rebuild-linux.sh
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
```

### macOS
```bash
chmod +x scripts/rebuild-macos.sh
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

Expected artifacts:
- Windows: `apps/src-tauri/target/release/bundle/nsis` or `bundle/msi`
- Linux: `apps/src-tauri/target/release/bundle/appimage` and `bundle/deb`
- macOS: `apps/src-tauri/target/release/bundle/dmg`

### Script arguments
- Windows script `scripts/rebuild.ps1`
- `-Bundle nsis|msi`: choose installer format
- `-NoBundle`: compile only
- `-CleanDist`: clean `apps/dist` before build
- `-Portable`: also stage portable output to `portable/`
- Linux/macOS scripts
- `--bundles "<types>"`: bundle targets (e.g. `appimage,deb` or `dmg`)
- `--no-bundle`: compile only
- `--clean-dist`: clean frontend output before build
- `--dry-run`: print planned commands only

### Recommended release flow (without GitHub Actions cost)
1. Pull latest code and open repo root.
2. Install deps: run `pnpm install` in `apps/`.
3. Run the platform script on each target OS.
4. Verify artifacts under `apps/src-tauri/target/release/bundle/`.
5. Upload artifacts manually to GitHub Release.

### Troubleshooting
- `pnpm: command not found`
- Cause: pnpm not installed or corepack not enabled.
- Fix: `corepack enable && corepack prepare pnpm@9 --activate`
- Missing Linux libs during `cargo tauri build`
- Cause: missing webkit/gtk runtime dependencies.
- Fix: install the listed Linux packages and retry.
- Windows shows malware warning
- Cause: unsigned binaries are often flagged by SmartScreen/AV heuristics.
- Fix: prefer installer bundles, sign binaries, and submit false-positive reports.

## Contact
![Personal](assets/images/personal.jpg)
![Group](assets/images/group.jpg)
