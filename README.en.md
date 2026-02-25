<p align="center">
  <img src="assets/logo/logo.png" alt="CodexManager Logo" width="220" />
</p>

<h1 align="center">CodexManager</h1>

<p align="center">A local desktop + service toolkit for Codex-compatible account and gateway management.</p>

<p align="center">
  <a href="README.md">中文</a>
</p>

A local desktop + service toolkit for managing a Codex-compatible ChatGPT account pool, usage, and platform keys, with a built-in local gateway.

## Recent Changes
- UI previews refreshed: dashboard/accounts/platform key/request logs/settings screenshots in `assets/images` now match the latest interface.
- Gateway protocol adapter was further modularized: request mapping and response conversion were split, and response conversion is now separated into JSON/SSE modules.
- Backend routing boundaries were unified to reduce duplicated gateway/proxy dispatch logic.
- Stability hardening: frontend refresh flow and request-log race handling were improved; clipboard fallback behavior was unified (clipboard API + execCommand fallback).
- Security and runtime controls were strengthened: `/rpc` token auth is enforced; request-gate budget, upstream connect timeout, proxy body size, and account inflight limits are configurable.
- Observability was expanded: route/status_class/protocol metrics were refined, and RPC + usage-refresh metrics were added.
- Release engineering was hardened while staying manual-only: release workflow includes optional verify gate (`run_verify`), target SHA resolution, and release metadata output.
- `scripts/rebuild.ps1` is aligned with workflow inputs (`tag/ref/run_verify`) and now matches runs by `head_sha`.

## Features
- Account pool management: group, tag, sort, note
- Usage dashboard: 5-hour + 7-day snapshots
- OAuth login: browser flow + manual callback parsing
- Platform keys: create, disable, delete, bind model
- Local service: auto-start with configurable port
- Local gateway: OpenAI-compatible entry for CLI/tools

## Screenshots
![Dashboard](assets/images/dashboard.png)
![Accounts](assets/images/accounts.png)
![Platform Key](assets/images/platform-key.png)
![Logs](assets/images/log.png)
![Settings](assets/images/themes.png)

## Tech Stack
- Frontend: Vite + vanilla JavaScript
- Desktop: Tauri (Rust)
- Service: Rust (local HTTP/RPC + Gateway)

## Project Structure
```text
.
├─ apps/                # Frontend + Tauri desktop app
│  ├─ src/
│  ├─ src-tauri/
│  └─ dist/
├─ crates/              # Rust core/service
│  ├─ core
│  └─ service
├─ scripts/             # build/release scripts
├─ portable/            # portable output
└─ README.en.md
```

## Quick Start
1. Launch desktop app and click "Start Service".
2. Add accounts in Account Management and finish OAuth.
3. If callback fails, paste callback URL into manual parser.
4. Refresh usage and verify account status.

## Development & Build
### Frontend
```bash
pnpm -C apps install
pnpm -C apps run dev
pnpm -C apps run test
pnpm -C apps run test:ui
pnpm -C apps run build
```

### Rust
```bash
cargo test --workspace
cargo build -p codexmanager-service --release
```

### Tauri Packaging (Windows)
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Tauri Packaging (Linux/macOS)
```bash
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

## GitHub Actions (Manual Only)
All workflows are `workflow_dispatch` only.

- `ci-verify.yml`
  - Purpose: quality gate (Rust tests + frontend tests + frontend build)
  - Trigger: manual only
- `release-multi-platform.yml`
  - Purpose: multi-platform packaging and release publishing
  - Trigger: manual only
  - Inputs:
    - `tag` (required)
    - `ref` (default: `main`)
    - `run_verify` (default: `true`)

## Script Reference
### `scripts/rebuild.ps1` (Windows)
Primarily for local Windows packaging. `-AllPlatforms` mode dispatches GitHub workflow.

Examples:
```powershell
# Local Windows build
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable

# Dispatch multi-platform workflow (and download artifacts)
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.0.9 `
  -GithubToken <token>

# Skip verify gate inside release workflow
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9 -GithubToken <token> -NoVerify
```

Parameters (with defaults):
- `-Bundle nsis|msi`: default `nsis`
- `-NoBundle`: compile only, no installer bundle
- `-CleanDist`: clean `apps/dist` before build
- `-Portable`: also stage portable output
- `-PortableDir <path>`: portable output dir, default `portable/`
- `-AllPlatforms`: dispatch `release-multi-platform.yml`
- `-GithubToken <token>`: GitHub token; falls back to `GITHUB_TOKEN`/`GH_TOKEN`
- `-WorkflowFile <name>`: default `release-multi-platform.yml`
- `-GitRef <ref>`: workflow ref; defaults to current branch or current tag
- `-ReleaseTag <tag>`: release tag; strongly recommended in `-AllPlatforms`
- `-NoVerify`: sets workflow input `run_verify=false`
- `-DownloadArtifacts <bool>`: default `true`
- `-ArtifactsDir <path>`: artifact download dir, default `artifacts/`
- `-PollIntervalSec <n>`: polling interval, default `10`
- `-TimeoutMin <n>`: timeout minutes, default `60`
- `-DryRun`: print plan only

## Environment Variables (Complete)
### Load Rules and Precedence
- Desktop app loads env files from executable directory in this order:
  - `codexmanager.env` -> `CodexManager.env` -> `.env` (first hit wins)
- Existing process/system env vars are not overridden by env-file values.
- Most vars are optional. `CODEXMANAGER_DB_PATH` is conditionally required when running `codexmanager-service` standalone.

### Runtime Variables (`CODEXMANAGER_*`)
| Variable | Default | Required | Description |
|---|---|---|---|
| `CODEXMANAGER_SERVICE_ADDR` | `localhost:48760` | Optional | Service bind address and default RPC target used by desktop app. |
| `CODEXMANAGER_DB_PATH` | None | Conditionally required | SQLite path. Desktop sets `app_data_dir/codexmanager.db`; set explicitly for standalone service runs. |
| `CODEXMANAGER_RPC_TOKEN` | Auto-generated random 64-hex string | Optional | `/rpc` auth token. Generated at runtime if missing or empty. |
| `CODEXMANAGER_NO_SERVICE` | Unset | Optional | If present (any value), desktop app does not auto-start embedded service. |
| `CODEXMANAGER_ISSUER` | `https://auth.openai.com` | Optional | OAuth issuer. |
| `CODEXMANAGER_CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` | Optional | OAuth client id. |
| `CODEXMANAGER_ORIGINATOR` | `codex_cli_rs` | Optional | OAuth authorize `originator` value. |
| `CODEXMANAGER_REDIRECT_URI` | `http://localhost:1455/auth/callback` (or dynamic login-server port) | Optional | OAuth redirect URI. |
| `CODEXMANAGER_LOGIN_ADDR` | `localhost:1455` | Optional | Local login callback listener address. |
| `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` | `false` | Optional | Allows non-loopback login callback address when set to `1/true/TRUE/yes/YES`. |
| `CODEXMANAGER_USAGE_BASE_URL` | `https://chatgpt.com` | Optional | Base URL for usage requests. |
| `CODEXMANAGER_DISABLE_POLLING` | Unset (polling enabled) | Optional | If present (any value), disables usage polling thread. |
| `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` | `600` | Optional | Usage polling interval in seconds, minimum `30`. Invalid values fall back to default. |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS` | `180` | Optional | Gateway keepalive interval in seconds, minimum `30`. |
| `CODEXMANAGER_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` | Optional | Primary upstream base URL. Bare ChatGPT host values are normalized to backend-api/codex. |
| `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL` | Auto-inferred | Optional | Explicit fallback upstream. If unset and primary is ChatGPT backend, fallback defaults to `https://api.openai.com/v1`. |
| `CODEXMANAGER_UPSTREAM_COOKIE` | Unset | Optional | Upstream Cookie, mainly for Cloudflare/WAF challenge scenarios. |
| `CODEXMANAGER_ROUTE_STRATEGY` | `ordered` | Optional | Gateway account routing strategy: default `ordered` (follow account order, fail over to next on failure); set `balanced`/`round_robin`/`rr` to enable key+model-based balanced round-robin starts. |
| `CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS` | `15` | Optional | Upstream connect timeout in seconds. |
| `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS` | `300` | Optional | Request-gate wait budget in milliseconds. |
| `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT` | `0` | Optional | Per-account soft inflight cap. `0` means unlimited. |
| `CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES` | `0` | Optional | Max bytes for trace body preview. `0` disables body preview. |
| `CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES` | `16777216` | Optional | Max accepted request body size for front proxy (16 MiB default). |
| `CODEXMANAGER_HTTP_WORKER_FACTOR` | `4` | Optional | Backend worker factor; workers = `max(cpu * factor, worker_min)`. |
| `CODEXMANAGER_HTTP_WORKER_MIN` | `8` | Optional | Minimum backend workers. |
| `CODEXMANAGER_HTTP_QUEUE_FACTOR` | `4` | Optional | Backend queue factor; queue = `max(worker * factor, queue_min)`. |
| `CODEXMANAGER_HTTP_QUEUE_MIN` | `32` | Optional | Minimum backend queue size. |

### Release-Script Related Variables
| Variable | Default | Required | Description |
|---|---|---|---|
| `GITHUB_TOKEN` | None | Conditionally required | Required for `rebuild.ps1 -AllPlatforms` when `-GithubToken` is not passed. |
| `GH_TOKEN` | None | Conditionally required | Fallback token variable equivalent to `GITHUB_TOKEN`. |

## Env File Example (next to executable)
```dotenv
# codexmanager.env / CodexManager.env / .env
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
# Optional: fixed RPC token for external clients
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

## Troubleshooting
- OAuth callback failures: check `CODEXMANAGER_LOGIN_ADDR` conflicts, or use manual callback parsing in UI.
- Model list/request blocked by challenge: try `CODEXMANAGER_UPSTREAM_COOKIE` or explicit `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL`.
- Standalone service reports storage unavailable: set `CODEXMANAGER_DB_PATH` to a writable path first.

## 🤝 Special Thanks
This project references the following open-source project for gateway protocol adaptation and stability hardening ideas:

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)

Related implementation points:
- `crates/codexmanager-service/src/gateway/protocol_adapter/request_mapping.rs`
- `crates/codexmanager-service/src/gateway/upstream/transport.rs`

## Contact
![Personal](assets/images/personal.jpg)
![Group](assets/images/group.jpg)
