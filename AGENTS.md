# Repository Guidelines

## Project Structure & Module Organization
This repository combines a desktop UI, a web/service runtime, and shared Rust crates.

- `apps/`: Vite-based frontend, Tauri desktop wrapper in `apps/src-tauri`, and JS tests in `apps/tests`.
- `apps/src/`: UI entrypoints, services, views, styles, and colocated tests such as `apps/src/views/**/tests`.
- `crates/core`: shared storage, migrations, and domain logic.
- `crates/service`: local API/gateway service.
- `crates/web`: web host for the service UI; `crates/start` boots service + web together.
- `assets/` stores screenshots and logos; `scripts/` holds packaging and release helpers.

## Build, Test, and Development Commands
- `pnpm -C apps install`: install frontend dependencies.
- `pnpm -C apps run dev`: start the Vite frontend for local UI work.
- `pnpm -C apps run build`: build `apps/dist` for web or embedded packaging.
- `pnpm -C apps run test`: run JS unit tests under `apps/src` with Node’s test runner.
- `pnpm -C apps run test:ui`: run UI/structure tests in `apps/tests`.
- `cargo test --workspace`: run Rust tests across all workspace crates.
- `cargo build -p codexmanager-web --release --features embedded-ui`: build the single-binary web release.

## Coding Style & Naming Conventions
Use existing style rather than introducing new patterns. Frontend code uses ESM, semicolons, 2-space indentation, and descriptive camelCase function names such as `refreshAccountsPage`. Keep view-specific code under `apps/src/views/*` and service logic under `apps/src/services/*`. Rust code follows standard 2021 edition conventions; run `cargo fmt` before submitting Rust changes. Name tests after behavior, for example `refresh-flow.test.mjs` or `gateway_logs.rs`.

## Testing Guidelines
Add or update tests with every behavior change. Prefer narrow tests close to the changed code, then run broader suites only as needed. JS source tests should end in `.test.js`; higher-level UI checks in `apps/tests` use `.test.mjs`. Rust integration tests live in `crates/*/tests`. Before opening a PR, run `pnpm -C apps run check` and `cargo test --workspace`.

## Commit & Pull Request Guidelines
Recent history favors short, imperative commit subjects, often in Chinese, for example `修复发布流程并优化账号批量导入`. Keep subjects focused on one change. Reference issues in the subject or body when relevant, e.g. `closes #19`. PRs should include: a brief summary, affected areas (`apps`, `crates/service`, etc.), test evidence, linked issues, and screenshots for UI changes.

## Security & Configuration Tips
Do not commit real tokens, exported account JSON, or local data files. Use `CODEXMANAGER_RPC_TOKEN` for service auth and document any new env vars in `README.md`. Default local ports are `48760` for the service and `48761` for the web UI; call out port changes in PRs.
