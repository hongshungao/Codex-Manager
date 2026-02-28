<p align="center">
  <img src="assets/logo/logo.png" alt="CodexManager Logo" width="220" />
</p>

<h1 align="center">CodexManager</h1>

<p align="center">本地桌面端 + 服务进程的 Codex 账号池管理器</p>

<p align="center">
  <a href="README.en.md">English</a>
</p>

本地桌面端 + 服务进程的 Codex 账号池管理器，用于统一管理账号、用量与平台 Key，并提供本地网关能力。

## 最近变更
- `v0.1.3`（最新，包含自 `v0.1.1` 以来全部更新）
- 新增 Service 一键启动器 `codexmanager-start`：单进程拉起 `service + web`，支持 `Ctrl+C` 联动关闭，降低“多进程手工启动”成本。
- Service 版 `codexmanager-web` 支持 `embedded-ui`：前端静态资源内嵌进二进制，解压后无需额外 `web/` 目录。
- 新增 Service/Web Docker 独立部署与 `docker-compose` 编排，支持无桌面环境部署。
- 优化 Windows 图标注入策略：`service/start/web` 主包构建保留图标，修复桌面端开发模式 `LNK1123` 链接失败问题。
- 新增 Azure OpenAI 协议支持：平台 Key 可选择 `azure_openai`，支持独立 Endpoint 配置与 Azure API Key 鉴权链路。
- 网关新增 Azure 专属转发模块（独立文件），在不破坏原有 OpenAI/Anthropic 路径的前提下完成协议分流。
- 平台 Key 弹窗优化：Azure 配置改为 `Endpoint + API Key` 直填模式，交互更直观。
- 请求日志体验优化：当账号信息缺失时，账号列使用 Key 前缀兜底展示，避免空白。
- 启动速度优化：启动阶段改为“本地优先加载”（账号/用量/模型先读本地），模型列表引入本地缓存与后台按需刷新（缓存为空立即拉取，随后按周期刷新），显著降低首屏等待。
- 网关模块重构：`gateway` 按 `auth/core/request/routing/observability/upstream` 分层，代码可维护性与定位效率提升。
- 前端交互优化：账号页与日志页显著降卡顿；刷新任务加入通用并发上限；并新增统一请求封装（`timeout/retry/取消`）提升弱网稳定性。
- 刷新体验升级：账号页“刷新所有”支持进度展示（完成/剩余）与按钮 busy 稳定处理，避免“点了没反应”感知。
- 账号导入增强：支持大批量导入分批处理，默认导入分组为 `IMPORT`，空分组账号自动补齐。
- 用量状态统一：后端引入统一可用状态枚举，并透出到前端进行一致文案映射（可用/单窗口可用/不可用/未知）。
- 请求日志体验优化：窄屏下按优先级隐藏次要列，保留账号/路径/模型/状态核心信息。
- 按钮与布局统一：页面主按钮、操作列按钮与弹窗按钮统一尺寸规范，账号管理与仪表盘版心对齐。
- 网关观测增强：`http_bridge` 输出累积加上限；新增 `/metrics` 暴露 DB busy、HTTP 队列深度、upstream attempt latency 等指标。
- 发布流程提速与防错：三平台 workflow 全手动触发；统一 Tauri CLI 版本；校验 tag/version；产物附带 `SHA256SUMS`/`manifest.json`；upload-artifact 关闭压缩；concurrency 按 `workflow+tag` 隔离避免跨平台互相取消；release 创建竞态自动降级为 `edit + upload`。

## 功能概览
- 账号池管理：分组、标签、排序、备注
- 用量展示：兼容 5 小时 + 7 日双窗口，以及仅返回 7 日单窗口（如免费周额度）的账号
- 授权登录：浏览器授权 + 手动回调解析
- 平台 Key：生成、禁用、删除、模型绑定
- 本地服务：自动拉起、可自定义端口
- 本地网关：为 CLI/第三方工具提供统一 OpenAI 兼容入口

## 截图
![仪表盘](assets/images/dashboard.png)
![账号管理](assets/images/accounts.png)
![平台 Key](assets/images/platform-key.png)
![日志视图](assets/images/log.png)
![设置页](assets/images/themes.png)

## 技术栈
- 前端：Vite + 原生 JavaScript
- 桌面端：Tauri (Rust)
- 服务端：Rust（本地 HTTP/RPC + Gateway）

## 目录结构
```text
.
├─ apps/                # 前端与 Tauri 桌面端
│  ├─ src/
│  ├─ src-tauri/
│  └─ dist/
├─ crates/              # Rust core/service
│  ├─ core
│  ├─ service
│  ├─ start              # Service 版本一键启动器（拉起 service + web）
│  └─ web                # Service 版本 Web UI（可内嵌静态资源 + /api/rpc 代理）
├─ scripts/             # 构建与发布脚本
├─ portable/            # 便携版输出目录
└─ README.md
```

## 快速开始
1. 启动桌面端，点击“启动服务”。
2. 进入“账号管理”，添加账号并完成授权。
3. 如回调失败，粘贴回调链接手动完成解析。
4. 刷新用量并确认账号状态。

## Service 版本（后台服务 + Web UI，无桌面环境）
1. 下载 Release 中的 `CodexManager-service-<platform>-<arch>.zip` 并解压。
2. 推荐：启动 `codexmanager-start`（一个进程拉起 service + web，且可在控制台 Ctrl+C 关闭）。
3. 也可以只启动 `codexmanager-web`（会自动拉起同目录的 `codexmanager-service`，并打开浏览器）。
4. 或者先启动 `codexmanager-service`（会显示控制台日志），再启动 `codexmanager-web`。
5. 默认地址：service `localhost:48760`，Web UI `http://localhost:48761/`。
6. 关闭：访问 `http://localhost:48761/__quit`（会关闭 web；若 web 自动拉起过 service，会尝试一并关闭 service）。

## Docker 部署
### 方式 1：docker compose（推荐）
```bash
docker compose -f docker/docker-compose.yml up --build
```
浏览器打开：`http://localhost:48761/`

### 方式 2：分别构建/运行
```bash
# service
docker build -f docker/Dockerfile.service -t codexmanager-service .
docker run --rm -p 48760:48760 -v codexmanager-data:/data \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-service

# web（需要能访问到 service）
docker build -f docker/Dockerfile.web -t codexmanager-web .
docker run --rm -p 48761:48761 \
  -e CODEXMANAGER_WEB_NO_SPAWN_SERVICE=1 \
  -e CODEXMANAGER_SERVICE_ADDR=host.docker.internal:48760 \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-web
```

## 开发与构建
### 前端
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
cargo build -p codexmanager-web --release
cargo build -p codexmanager-start --release

# 发行物/容器：将前端静态资源打进 codexmanager-web（二进制单文件）
pnpm -C apps run build
cargo build -p codexmanager-web --release --features embedded-ui
```

### Tauri 打包（Windows）
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Tauri 打包（Linux/macOS）
```bash
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

## GitHub Actions（全部手动触发）
当前 workflow 均为 `workflow_dispatch`，不会自动触发。

- `ci-verify.yml`
  - 用途：质量门（Rust tests + 前端 tests + 前端 build）
  - 触发：手动
- `release-windows.yml`
  - 用途：Windows 打包与 release 发布（安装包 + portable）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
- `release-linux.yml`
  - 用途：Linux 打包与 release 发布（AppImage/deb/rpm + portable）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
- `release-macos-beta.yml`
  - 用途：macOS 内测包发布（未签名，仅内测）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
- `release-service-windows.yml`
  - 用途：Windows Service 版本打包与 release 发布（zip）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
- `release-service-linux.yml`
  - 用途：Linux Service 版本打包与 release 发布（zip）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
- `release-service-macos.yml`
  - 用途：macOS Service 版本打包与 release 发布（zip）
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）

## 脚本说明
### `scripts/rebuild.ps1`（Windows）
默认用于本地 Windows 打包；`-AllPlatforms` 模式会调用 GitHub workflow。

常用示例：
```powershell
# 本地 Windows 构建
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable

# 触发 release workflow（并下载工件）
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.0.9 `
  -GithubToken <token>

# 跳过 workflow 内质量门
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9 -GithubToken <token> -NoVerify
```

参数（含默认值）：
- `-Bundle nsis|msi`：默认 `nsis`
- `-NoBundle`：仅编译，不出安装包
- `-CleanDist`：构建前清理 `apps/dist`
- `-Portable`：额外输出便携版
- `-PortableDir <path>`：便携版输出目录，默认 `portable/`
- `-AllPlatforms`：触发指定 release workflow（由 `-WorkflowFile` 指定）
- `-GithubToken <token>`：GitHub token；不传时尝试 `GITHUB_TOKEN`/`GH_TOKEN`
- `-WorkflowFile <name>`：默认由脚本配置决定，建议显式传入（如 `release-windows.yml`）
- `-GitRef <ref>`：workflow 构建 ref；默认当前分支或当前 tag
- `-ReleaseTag <tag>`：发布 tag；`-AllPlatforms` 时建议显式传入
- `-NoVerify`：将 workflow 输入 `run_verify` 设为 `false`
- `-DownloadArtifacts <bool>`：默认 `true`
- `-ArtifactsDir <path>`：工件下载目录，默认 `artifacts/`
- `-PollIntervalSec <n>`：轮询间隔，默认 `10`
- `-TimeoutMin <n>`：超时分钟数，默认 `60`
- `-DryRun`：仅打印执行计划

### `scripts/bump-version.ps1`（统一版本号）
用于一次性更新发版版本号，避免手改多个文件。

```powershell
pwsh -NoLogo -NoProfile -File scripts/bump-version.ps1 -Version 0.1.3
```

会同步更新：
- 根 `Cargo.toml` 的 workspace 版本
- `apps/src-tauri/Cargo.toml`
- `apps/src-tauri/tauri.conf.json`

## 环境变量说明
### 加载与优先级
- 桌面端 / `codexmanager-service` / `codexmanager-web` 均会在可执行文件同目录按顺序查找环境文件：`codexmanager.env` -> `CodexManager.env` -> `.env`（命中第一个即停止）。
- 环境文件中只会注入“当前进程尚未定义”的变量，已有系统/用户变量不会被覆盖。
- 绝大多数变量均为可选；若运行目录不可写（如安装目录），可用 `CODEXMANAGER_DB_PATH` 指向可写路径。
- 下表按“常用/高级”拆分；若需要完整列表，可在源码中搜索 `CODEXMANAGER_` 作为最终准入标准。

### 常用变量（`CODEXMANAGER_*`）
| 变量 | 默认值 | 说明 |
|---|---|---|
| `CODEXMANAGER_SERVICE_ADDR` | `localhost:48760` | service 监听地址；桌面端也会用它作为默认 RPC 目标地址。 |
| `CODEXMANAGER_WEB_ADDR` | `localhost:48761` | Service 版本 Web UI 监听地址（仅 `codexmanager-web` 使用）。 |
| `CODEXMANAGER_WEB_ROOT` | 同目录 `web/` | Web 静态资源目录（仅 `codexmanager-web` 使用；若使用内嵌前端资源则无需该目录）。 |
| `CODEXMANAGER_WEB_NO_OPEN` | 未设置 | 若设置则 `codexmanager-web` 不会自动打开浏览器。 |
| `CODEXMANAGER_WEB_NO_SPAWN_SERVICE` | 未设置 | 若设置则 `codexmanager-web` 不会尝试自动拉起同目录的 `codexmanager-service`。 |
| `CODEXMANAGER_DB_PATH` | 同目录 `codexmanager.db`（Service/Web）；桌面端自动设置 | SQLite 数据库路径。桌面端会自动设为 `app_data_dir/codexmanager.db`。 |
| `CODEXMANAGER_RPC_TOKEN` | 自动生成 64 位十六进制随机串 | `/rpc` 鉴权 token。未设置时自动生成，并默认落盘到 `codexmanager.rpc-token` 便于跨进程复用。 |
| `CODEXMANAGER_RPC_TOKEN_FILE` | 同目录 `codexmanager.rpc-token` | 指定 `/rpc` token 文件路径（相对路径以 DB 所在目录为基准）。 |
| `CODEXMANAGER_NO_SERVICE` | 未设置 | 只要变量存在（值可为空）就不自动拉起内嵌 service。 |
| `CODEXMANAGER_ISSUER` | `https://auth.openai.com` | OAuth issuer。 |
| `CODEXMANAGER_CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` | OAuth client id。 |
| `CODEXMANAGER_ORIGINATOR` | `codex_cli_rs` | OAuth authorize 请求中的 `originator`。 |
| `CODEXMANAGER_REDIRECT_URI` | `http://localhost:1455/auth/callback`（或登录服务动态端口） | OAuth 回调地址。 |
| `CODEXMANAGER_LOGIN_ADDR` | `localhost:1455` | 本地登录回调监听地址。 |
| `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` | `false` | 是否允许非 loopback 回调地址。仅 `1/true/TRUE/yes/YES` 视为开启。 |
| `CODEXMANAGER_USAGE_BASE_URL` | `https://chatgpt.com` | 用量接口 base URL。 |
| `CODEXMANAGER_DISABLE_POLLING` | 未设置（即开启轮询） | 只要变量存在（值可为空）就禁用后台用量轮询线程。 |
| `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` | `600` | 用量轮询间隔（秒），最小 `30`。非法值回退默认。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS` | `180` | Gateway keepalive 间隔（秒），最小 `30`。 |
| `CODEXMANAGER_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` | 主上游地址。若填 `https://chatgpt.com`/`https://chat.openai.com` 会自动归一化到 backend-api/codex。 |
| `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL` | 自动推断 | 明确指定 fallback 上游。若未设置且主上游是 ChatGPT backend，则默认 fallback 到 `https://api.openai.com/v1`。 |
| `CODEXMANAGER_UPSTREAM_COOKIE` | 未设置 | 上游 Cookie（主要用于 Cloudflare/WAF challenge 场景）。 |
| `CODEXMANAGER_ROUTE_STRATEGY` | `ordered` | 网关账号选路策略：默认 `ordered`（按账号顺序优先，失败再下一个）；可设 `balanced`/`round_robin`/`rr` 启用按 `Key+模型` 的均衡轮询起点。 |
| `CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS` | `15` | 上游连接阶段超时（秒）。 |
| `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` | `120000` | 上游单次请求总超时（毫秒）。设为 `0` 表示关闭总超时。 |
| `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS` | `300000` | 上游流式请求超时（毫秒）。设为 `0` 表示关闭流式超时。 |
| `CODEXMANAGER_PROXY_LIST` | 未设置 | 上游代理池（最多 5 条，逗号/分号/换行分隔）。按 `account_id` 稳定哈希绑定到某个代理，避免同账号跨代理漂移。 |
| `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS` | `300` | 请求闸门等待预算（毫秒）。 |
| `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT` | `0` | 单账号并发软上限。`0` 表示不限制。 |
| `CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES` | `0` | Trace body 预览最大字节数。`0` 表示关闭 body 预览。 |
| `CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES` | `16777216` | 前置代理允许的请求体最大字节数（默认 16 MiB）。 |
| `CODEXMANAGER_HTTP_WORKER_FACTOR` | `4` | backend worker 数量系数，worker = `max(cpu * factor, worker_min)`。 |
| `CODEXMANAGER_HTTP_WORKER_MIN` | `8` | backend worker 最小值。 |
| `CODEXMANAGER_HTTP_QUEUE_FACTOR` | `4` | backend 请求队列系数，queue = `max(worker * factor, queue_min)`。 |
| `CODEXMANAGER_HTTP_QUEUE_MIN` | `32` | backend 请求队列最小值。 |

### 高级变量（可选）
| 变量 | 默认值 | 说明 |
|---|---|---|
| `CODEXMANAGER_ACCOUNT_IMPORT_BATCH_SIZE` | `200` | 账号导入分批大小（用于一次导入大量 auth.json）。 |
| `CODEXMANAGER_TRACE_QUEUE_CAPACITY` | `2048` | gateway trace 异步写队列容量（过小可能丢 trace；过大可能占内存）。 |
| `CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR` | `1` | backend stream worker 数量系数（SSE 等长连接请求）。 |
| `CODEXMANAGER_HTTP_STREAM_WORKER_MIN` | `2` | backend stream worker 最小值。 |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_FACTOR` | `2` | backend stream 队列系数。 |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_MIN` | `16` | backend stream 队列最小值。 |
| `CODEXMANAGER_POLL_JITTER_SECS` | 未设置 | 通用轮询 jitter（秒），可被各模块各自的 jitter 覆盖。 |
| `CODEXMANAGER_POLL_FAILURE_BACKOFF_MAX_SECS` | 未设置 | 通用失败退避上限（秒），可被各模块各自的 backoff 覆盖。 |
| `CODEXMANAGER_USAGE_POLL_JITTER_SECS` | `5` | 用量轮询 jitter（秒）。 |
| `CODEXMANAGER_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS` | `1800` | 用量轮询失败退避上限（秒）。 |
| `CODEXMANAGER_USAGE_REFRESH_WORKERS` | `4` | 用量刷新 worker 数。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_JITTER_SECS` | `5` | keepalive jitter（秒）。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS` | `900` | keepalive 失败退避上限（秒）。 |
| `CODEXMANAGER_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS` | `60` | 用量刷新失败事件去重窗口（秒），避免瞬时抖动刷爆事件表。 |
| `CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT` | `200` | 每账号保留用量快照条数（0 表示不裁剪）。 |
| `CODEXMANAGER_CANDIDATE_CACHE_TTL_MS` | `500` | 网关候选快照缓存 TTL（毫秒），减少高频请求时的 DB 压力；设为 `0` 关闭缓存。 |
| `CODEXMANAGER_PROMPT_CACHE_TTL_SECS` | `3600` | prompt cache TTL（秒）。 |
| `CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS` | `60` | prompt cache 清理间隔（秒）。 |
| `CODEXMANAGER_PROMPT_CACHE_CAPACITY` | `4096` | prompt cache 容量上限（0 表示不限制）。 |
| `CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES` | `131072` | 上游响应 `output_text` 累积上限（字节），避免内存增长（0 关闭限制）。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED` | `true` | 是否启用候选健康度 P2C（Power of Two Choices）选路。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW` | `3` | `ordered` 模式下 P2C 参与窗口大小。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW` | `6` | `balanced` 模式下 P2C 参与窗口大小。 |
| `CODEXMANAGER_ROUTE_STATE_TTL_SECS` | `21600` | 路由状态 TTL（秒），避免 key/model 高基数导致状态无限增长。 |
| `CODEXMANAGER_ROUTE_STATE_CAPACITY` | `4096` | 路由状态容量上限。 |
| `CODEXMANAGER_UPDATE_REPO` | `qxcnm/Codex-Manager` | 应用内更新检查的 GitHub 仓库（`owner/name`）。 |
| `CODEXMANAGER_GITHUB_TOKEN` | 未设置 | 应用内“一键更新”用 GitHub token（也会回退到 `GITHUB_TOKEN`/`GH_TOKEN`）；不设置可能受 API 限流影响导致下载元数据降级。 |

### 发布脚本相关变量
| 变量 | 默认值 | 是否必填 | 说明 |
|---|---|---|---|
| `GITHUB_TOKEN` | 无 | 条件必填 | 仅在 `rebuild.ps1 -AllPlatforms` 且未传 `-GithubToken` 时必填。 |
| `GH_TOKEN` | 无 | 条件必填 | 与 `GITHUB_TOKEN` 等价的后备变量。 |

## 环境文件示例（放在可执行文件同目录）
```dotenv
# codexmanager.env / CodexManager.env / .env
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_WEB_ADDR=localhost:48761
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
# 可选：固定 RPC token 方便外部工具长期复用
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

说明：
- 环境文件在**桌面端 / service / web 进程启动时**读取一次；修改文件后需要重启对应进程才会生效。
- 桌面端会把 service 端口保存到本地存储；环境变量更多用于首次默认值（若需强制按环境变量重置，请在 UI 手动修改端口，或清理本地存储后重启）。
- 环境文件只会注入“当前进程尚未定义”的变量；若你已在系统环境变量中设置了同名 `CODEXMANAGER_*`，则系统环境变量优先生效。

## 常见问题
- 授权回调失败：优先检查 `CODEXMANAGER_LOGIN_ADDR` 是否被占用，或在 UI 使用手动回调解析。
- 模型列表/请求被挑战拦截：可尝试设置 `CODEXMANAGER_UPSTREAM_COOKIE`，或显式配置 `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL`。
- 独立运行 service/Web：若所在目录不可写（如安装目录），请设置 `CODEXMANAGER_DB_PATH` 到可写路径。
- macOS 代理环境下请求 `502/503`：优先确认系统代理未接管本地回环请求（`localhost/127.0.0.1` 走 `DIRECT`），并确保地址使用小写 `localhost:<port>`（例如 `localhost:48760`）。

## 账号命中规则
- `ordered`（顺序优先）模式下，网关按账号 `sort` 升序构建候选并依次尝试（例如 `0 -> 1 -> 2 -> 3`）。
- 这表示“按顺序尝试”，不是“永远命中 0 号”：前序账号若不可用/失败，会自动切到下一个。
- 以下情况会导致前序账号不被命中：
  - 账号状态不是 `active`
  - 账号缺少 token
  - 用量判定不可用（如主窗口已用尽、用量字段缺失等）
  - 账号处于 cooldown 或并发软上限触发跳过
- `balanced`（均衡轮询）模式会按 `Key + 模型` 维度轮换起点，不保证从最小 `sort` 开始。
- 排查时可查看数据库同目录 `gateway-trace.log`：
  - `CANDIDATE_POOL`：本次请求候选顺序
  - `CANDIDATE_START` / `CANDIDATE_SKIP`：实际尝试与跳过原因
  - `REQUEST_FINAL`：最终命中账号

## 🤝 鸣谢项目 (Special Thanks)
本项目在网关协议适配与稳定性治理上参考了以下开源项目的思路：

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)

对应实现可见：
- `crates/service/src/gateway/protocol_adapter/request_mapping.rs`
- `crates/service/src/gateway/upstream/transport.rs`

## 联系方式
![个人](assets/images/personal.jpg)
![交流群](assets/images/group.jpg)

有兴趣的可以关注我微信公众号 七线牛马
