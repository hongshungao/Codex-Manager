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
.\rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

Artifacts:
- Installer bundles: `apps/src-tauri/target/release/bundle/`
- Portable build: `portable/`

## Contact
![Personal](assets/images/personal.jpg)
![Group](assets/images/group.jpg)
