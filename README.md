# CodexManager

本地桌面端 + 服务进程的 Codex 账号池管理器，用于统一管理账号、用量与平台 Key，并提供本地网关与服务能力。

[English](README.en.md)

## 项目简介
- 桌面端（Tauri）负责账号管理、用量展示、授权登录与平台 Key 配置
- 服务端（Rust）提供本地 RPC + Gateway，支持用量刷新、账号轮询与鉴权转发
- 支持手动解析 OAuth 回调链接，避免端口冲突或回调失败

## 功能亮点
- 账号池管理：分组/标签/排序/备注
- 用量展示：5 小时 + 7 日用量快照
- 授权登录：浏览器授权 + 手动回调解析
- 平台 Key：生成/禁用/删除
- 本地服务：自动启动、可自定义端口
- 网关能力：为本地 CLI/工具提供统一入口

## 截图预览

![仪表盘](assets/images/dashboard.png)
![账号管理](assets/images/accounts.png)
![平台 Key](assets/images/platform-key.png)
![日志视图](assets/images/log.png)
![主题切换](assets/images/themes.png)

## 技术栈
- 前端：Vite + 原生 JS + 自定义 UI
- 桌面端：Tauri (Rust)
- 服务端：Rust（本地 HTTP/RPC + Gateway）

## 目录结构
```
.
├─ apps/                # 前端与 Tauri 桌面端
│  ├─ src/              # 前端源码
│  ├─ src-tauri/        # Tauri 端源码
│  └─ dist/             # 前端构建产物
├─ crates/              # Rust 核心与服务端
│  ├─ gpttools-core
│  └─ gpttools-service
├─ portable/            # 便携版输出目录
├─ rebuild.ps1          # 打包脚本
└─ README.md
```

## 快速开始
1. 启动桌面端后，点击“启动服务”
2. 进入“账号管理”添加账号并完成授权
3. 需要时粘贴回调链接完成手动解析
4. 刷新用量，检查状态与配额

## 开发与构建
### 前端开发
```
pnpm run dev
```

### 构建前端
```
pnpm run build
```

### Rust（service）单独构建
本项目的 service 可单独编译（用于调试、替换或嵌入桌面端）。

```
cargo build -p gpttools-service --release
```

产物默认在：
- `target/release/gpttools-service.exe`

### Tauri 打包
```
.\scripts\rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### 产物说明
- 安装包（nsis/msi）：`apps/src-tauri/target/release/bundle/`
- 便携版：`portable/`

## 三平台打包脚本
说明：Windows/Linux/macOS 需要在各自系统上执行对应脚本。

### 前置环境
- Node.js 20+
- pnpm 9+（建议 `corepack enable` 后使用）
- Rust stable（`rustup default stable`）
- Tauri CLI（`cargo install tauri-cli --locked`）

### 平台额外依赖
- Windows：Visual Studio C++ Build Tools（含 Windows SDK）
- Linux（Ubuntu 22.04+）：
```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libsoup-3.0-dev
```
- macOS：
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

预期产物：
- Windows: `apps/src-tauri/target/release/bundle/nsis` 或 `bundle/msi`
- Linux: `apps/src-tauri/target/release/bundle/appimage`、`bundle/deb`
- macOS: `apps/src-tauri/target/release/bundle/dmg`

### 参数说明
- Windows 脚本 `scripts/rebuild.ps1`
- `-Bundle nsis|msi`：指定安装包类型
- `-NoBundle`：仅编译，不生成安装包
- `-CleanDist`：清理前端 `apps/dist` 后再构建
- `-Portable`：额外生成便携版到 `portable/`
- Linux/macOS 脚本
- `--bundles "<types>"`：指定打包类型（例如 `appimage,deb` 或 `dmg`）
- `--no-bundle`：仅编译
- `--clean-dist`：清理前端产物后再构建
- `--dry-run`：仅打印执行计划，不真正执行

### 推荐打包流程
1. 拉取最新代码并进入仓库根目录。
2. 安装依赖：`pnpm install`（在 `apps/` 下执行）。
3. 执行对应平台脚本。
4. 校验产物是否存在于 `apps/src-tauri/target/release/bundle/`。
5. 手动上传产物到 GitHub Release（避免 CI 分钟费用）。

### 常见报错排查
- `pnpm: command not found`
- 原因：未安装 pnpm 或未启用 corepack。
- 处理：`corepack enable && corepack prepare pnpm@9 --activate`
- `cargo tauri build` 缺少系统库（Linux）
- 原因：缺少 webkit/gtk 等依赖。
- 处理：按上面的 Linux 依赖命令安装后重试。
- Windows 报“检测到病毒”
- 原因：未签名可执行文件容易被 SmartScreen/杀软误报。
- 处理：优先发布安装包、做代码签名、提交误报申诉。

## 常见问题
- 授权回调失败：可手动粘贴回调链接解析
- 端口被占用：修改顶部端口输入框后重新启动服务
- 用量查询失败：稍后重试，或检查服务状态与网络

## 联系我们 支持的话可以通过交流群团购team
![个人](assets/images/personal.jpg)
![交流群](assets/images/group.jpg)

有兴趣的可以关注我微信公众号 七线牛马
