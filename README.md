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
- 网关协议适配继续拆分：`protocol_adapter` 的请求映射与响应转换进一步模块化，响应转换按 JSON/SSE 分离，维护边界更清晰。
- 后端路由边界收敛：统一 backend router 分发路径，减少网关/代理双栈逻辑重复。
- 稳定性增强：前端刷新与请求日志竞态治理、复制能力统一降级（clipboard API + execCommand fallback）、关键事件绑定幂等化。
- 安全与运行时控制增强：`/rpc` token 鉴权强制开启，请求闸门等待预算、上游连接超时、代理 body 限制、账号并发上限等参数可配置。
- 可观测性增强：补齐 route/status_class/protocol 维度指标，并新增 RPC 与 usage refresh 指标。
- 工程链路增强：release workflow 仍保持手动触发，并新增可选质量门（`run_verify`）、目标 SHA 追踪、release 元信息文件。
- 发布脚本对齐：`scripts/rebuild.ps1` 支持传递 `tag/ref/run_verify` 到 workflow，并按 `head_sha` 轮询匹配正确的 workflow run。

## 功能概览
- 账号池管理：分组、标签、排序、备注
- 用量展示：5 小时 + 7 日用量快照
- 授权登录：浏览器授权 + 手动回调解析
- 平台 Key：生成、禁用、删除、模型绑定
- 本地服务：自动拉起、可自定义端口
- 本地网关：为 CLI/第三方工具提供统一 OpenAI 兼容入口

## 截图
![仪表盘](assets/images/dashboard.png)
![账号管理](assets/images/accounts.png)
![平台 Key](assets/images/platform-key.png)
![日志视图](assets/images/log.png)
![主题切换](assets/images/themes.png)

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
│  └─ service
├─ scripts/             # 构建与发布脚本
├─ portable/            # 便携版输出目录
└─ README.md
```

## 快速开始
1. 启动桌面端，点击“启动服务”。
2. 进入“账号管理”，添加账号并完成授权。
3. 如回调失败，粘贴回调链接手动完成解析。
4. 刷新用量并确认账号状态。

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
- `release-multi-platform.yml`
  - 用途：三平台打包与 release 发布
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

# 触发三平台 workflow（并下载工件）
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
- `-AllPlatforms`：触发 `release-multi-platform.yml`
- `-GithubToken <token>`：GitHub token；不传时尝试 `GITHUB_TOKEN`/`GH_TOKEN`
- `-WorkflowFile <name>`：默认 `release-multi-platform.yml`
- `-GitRef <ref>`：workflow 构建 ref；默认当前分支或当前 tag
- `-ReleaseTag <tag>`：发布 tag；`-AllPlatforms` 时建议显式传入
- `-NoVerify`：将 workflow 输入 `run_verify` 设为 `false`
- `-DownloadArtifacts <bool>`：默认 `true`
- `-ArtifactsDir <path>`：工件下载目录，默认 `artifacts/`
- `-PollIntervalSec <n>`：轮询间隔，默认 `10`
- `-TimeoutMin <n>`：超时分钟数，默认 `60`
- `-DryRun`：仅打印执行计划

## 环境变量说明（完整）
### 加载与优先级
- 桌面端会在可执行文件同目录按顺序查找环境文件：`codexmanager.env` -> `CodexManager.env` -> `.env`（命中第一个即停止）。
- 环境文件中只会注入“当前进程尚未定义”的变量，已有系统/用户变量不会被覆盖。
- 绝大多数变量均为可选；`CODEXMANAGER_DB_PATH` 在“独立运行 service 二进制”场景下属于必填。

### 运行时变量（`CODEXMANAGER_*`）
| 变量 | 默认值 | 是否必填 | 说明 |
|---|---|---|---|
| `CODEXMANAGER_SERVICE_ADDR` | `localhost:48760` | 可选 | service 监听地址；桌面端也会用它作为默认 RPC 目标地址。 |
| `CODEXMANAGER_DB_PATH` | 无 | 条件必填 | 数据库路径。桌面端会自动设为 `app_data_dir/codexmanager.db`；独立运行 `codexmanager-service` 时建议显式设置。 |
| `CODEXMANAGER_RPC_TOKEN` | 自动生成 64 位十六进制随机串 | 可选 | `/rpc` 鉴权 token。未设置时进程启动后自动生成，仅当前进程有效。 |
| `CODEXMANAGER_NO_SERVICE` | 未设置 | 可选 | 只要变量存在（值可为空）就不自动拉起内嵌 service。 |
| `CODEXMANAGER_ISSUER` | `https://auth.openai.com` | 可选 | OAuth issuer。 |
| `CODEXMANAGER_CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` | 可选 | OAuth client id。 |
| `CODEXMANAGER_ORIGINATOR` | `codex_cli_rs` | 可选 | OAuth authorize 请求中的 `originator`。 |
| `CODEXMANAGER_REDIRECT_URI` | `http://localhost:1455/auth/callback`（或登录服务动态端口） | 可选 | OAuth 回调地址。 |
| `CODEXMANAGER_LOGIN_ADDR` | `localhost:1455` | 可选 | 本地登录回调监听地址。 |
| `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` | `false` | 可选 | 是否允许非 loopback 回调地址。仅 `1/true/TRUE/yes/YES` 视为开启。 |
| `CODEXMANAGER_USAGE_BASE_URL` | `https://chatgpt.com` | 可选 | 用量接口 base URL。 |
| `CODEXMANAGER_DISABLE_POLLING` | 未设置（即开启轮询） | 可选 | 只要变量存在（值可为空）就禁用后台用量轮询线程。 |
| `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` | `600` | 可选 | 用量轮询间隔（秒），最小 `30`。非法值回退默认。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS` | `180` | 可选 | Gateway keepalive 间隔（秒），最小 `30`。 |
| `CODEXMANAGER_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` | 可选 | 主上游地址。若填 `https://chatgpt.com`/`https://chat.openai.com` 会自动归一化到 backend-api/codex。 |
| `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL` | 自动推断 | 可选 | 明确指定 fallback 上游。若未设置且主上游是 ChatGPT backend，则默认 fallback 到 `https://api.openai.com/v1`。 |
| `CODEXMANAGER_UPSTREAM_COOKIE` | 未设置 | 可选 | 上游 Cookie（主要用于 Cloudflare/WAF challenge 场景）。 |
| `CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS` | `15` | 可选 | 上游连接阶段超时（秒）。 |
| `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS` | `300` | 可选 | 请求闸门等待预算（毫秒）。 |
| `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT` | `0` | 可选 | 单账号并发软上限。`0` 表示不限制。 |
| `CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES` | `0` | 可选 | Trace body 预览最大字节数。`0` 表示关闭 body 预览。 |
| `CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES` | `16777216` | 可选 | 前置代理允许的请求体最大字节数（默认 16 MiB）。 |
| `CODEXMANAGER_HTTP_WORKER_FACTOR` | `4` | 可选 | backend worker 数量系数，worker = `max(cpu * factor, worker_min)`。 |
| `CODEXMANAGER_HTTP_WORKER_MIN` | `8` | 可选 | backend worker 最小值。 |
| `CODEXMANAGER_HTTP_QUEUE_FACTOR` | `4` | 可选 | backend 请求队列系数，queue = `max(worker * factor, queue_min)`。 |
| `CODEXMANAGER_HTTP_QUEUE_MIN` | `32` | 可选 | backend 请求队列最小值。 |

### 发布脚本相关变量
| 变量 | 默认值 | 是否必填 | 说明 |
|---|---|---|---|
| `GITHUB_TOKEN` | 无 | 条件必填 | 仅在 `rebuild.ps1 -AllPlatforms` 且未传 `-GithubToken` 时必填。 |
| `GH_TOKEN` | 无 | 条件必填 | 与 `GITHUB_TOKEN` 等价的后备变量。 |

## 环境文件示例（放在可执行文件同目录）
```dotenv
# codexmanager.env / CodexManager.env / .env
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
# 可选：固定 RPC token 方便外部工具长期复用
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

## 常见问题
- 授权回调失败：优先检查 `CODEXMANAGER_LOGIN_ADDR` 是否被占用，或在 UI 使用手动回调解析。
- 模型列表/请求被挑战拦截：可尝试设置 `CODEXMANAGER_UPSTREAM_COOKIE`，或显式配置 `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL`。
- 独立运行 service 报存储不可用：先设置 `CODEXMANAGER_DB_PATH` 到可写路径。

## 🤝 鸣谢项目 (Special Thanks)
本项目在网关协议适配与稳定性治理上参考了以下开源项目的思路：

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)

对应实现可见：
- `crates/codexmanager-service/src/gateway/protocol_adapter/request_mapping.rs`
- `crates/codexmanager-service/src/gateway/upstream/transport.rs`

## 联系方式
![个人](assets/images/personal.jpg)
![交流群](assets/images/group.jpg)

有兴趣的可以关注我微信公众号 七线牛马
