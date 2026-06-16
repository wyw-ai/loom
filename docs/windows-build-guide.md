# Windows 环境下构建 Loom 项目指南

> **最后更新**: 2026-06-17  
> **适用版本**: Loom Desktop v0.1.0 (dev 分支)  
> **本文档独立可读**，无需依赖 thread 对话历史即可完成构建。

---

## 目录

1. [前置依赖](#1-前置依赖)
2. [项目克隆与结构](#2-项目克隆与结构)
3. [Server 构建](#3-server-构建)
4. [Daemon 构建](#4-daemon-构建)
5. [CLI 构建](#5-cli-构建)
6. [Shell 构建](#6-shell-构建)
7. [GUI 构建](#7-gui-构建)
8. [验证方法](#8-验证方法)
9. [常见问题](#9-常见问题)
10. [产物路径速查](#10-产物路径速查)

---

## 1. 前置依赖

### 1.1 Rust 工具链

```powershell
# 安装 rustup（选择 "Visual Studio" 作为默认 host triple）
winget install Rustlang.Rustup

# 或从官网下载安装器: https://rustup.rs
```

安装后确认：

```powershell
rustc --version   # 应 ≥ 1.75.0
cargo --version
rustup show
```

**重要**：确保已安装 `x86_64-pc-windows-msvc` 工具链：

```powershell
rustup target list --installed | findstr x86_64-pc-windows-msvc
```

### 1.2 Microsoft Visual Studio C++ Build Tools

Tauri v2 编译需要 MSVC 工具链（用于编译 Windows 原生依赖如 `windows-sys`、`openssl` 等）：

1. 下载 [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
2. 安装时勾选 **"使用 C++ 的桌面开发"** 工作负载
3. 确保选中以下组件：
   - MSVC v143 - VS 2022 C++ x64/x86 生成工具
   - Windows 11 SDK (或 Windows 10 SDK)
   - C++ CMake 工具

### 1.3 Node.js + pnpm

```powershell
# 安装 Node.js 22 LTS
winget install OpenJS.NodeJS.LTS

# 安装 pnpm
npm install -g pnpm@9.15.0
```

确认版本：

```powershell
node --version   # 应 ≥ 22.x
pnpm --version   # 应 = 9.15.0
```

### 1.4 Git

```powershell
winget install Git.Git
git --version
```

### 1.5 WebView2 Runtime

Windows 10/11 已预装。如需手动安装：

```powershell
winget install Microsoft.EdgeWebView2Runtime
```

---

## 2. 项目克隆与结构

```powershell
git clone https://github.com/plumeink/joi-apps-temp.git
cd joi-apps-temp
git checkout dev
```

### Crate 结构概览

```
joi-apps-temp/
├── Cargo.toml                 # 工作区根配置
├── crates/
│   ├── agent-runtime/         # Agent 运行时核心 (库)
│   ├── cli/                   # CLI 工具包
│   │   ├── src/main.rs        #   → loom.exe (CLI 入口)
│   │   └── src/bin/
│   │       └── loom_daemon.rs #   → loom-daemon.exe (守护进程)
│   ├── gui/                   # Tauri v2 桌面应用
│   │   ├── tauri.conf.json    #   Tauri 配置
│   │   └── src-tauri/         #   Rust 后端 (loom-gui)
│   ├── loom-shell/            # Shell 执行代理 (loom-shell.exe)
│   ├── proto/                 # Protobuf 协议定义
│   ├── server/                # 后台服务 (loom-server.exe)
│   └── windows-console/       # Windows 控制台工具
└── apps/
    └── gui-web/               # 前端 (React + Vite + TypeScript)
        ├── package.json
        └── pnpm-lock.yaml
```

---

## 3. Server 构建

```powershell
cargo build -p loom-server --release
```

**产物路径**：`target/release/loom-server.exe`

**可选参数**：
- 去掉 `--release` 构建 debug 版本（更大、有调试符号、未优化）
- 添加 `--target x86_64-pc-windows-msvc` 显式指定目标平台

---

## 4. Daemon 构建

```powershell
cargo build -p loom-daemon --release
```

**产物路径**：`target/release/loom-daemon.exe`

loom-daemon 是 Loom 多 actor 协作的守护进程，负责管理 agent 生命周期和消息路由。

---

## 5. CLI 构建

```powershell
cargo build -p loom --release
```

**产物路径**：`target/release/loom.exe`

CLI 工具用于与 loom-daemon 交互（发送消息、查询状态等）。

---

## 6. Shell 构建

```powershell
cargo build -p loom-shell --release
```

**产物路径**：`target/release/loom-shell.exe`

loom-shell 是 Agent 执行 shell 命令的代理进程，独立于 Tauri GUI 运行。

---

## 7. GUI 构建

### 7.1 前端构建（独立验证）

```powershell
cd apps/gui-web
pnpm install --frozen-lockfile
pnpm build
```

产物输出到 `apps/gui-web/dist/`。

### 7.2 Tauri 桌面应用构建

**前置步骤**：确保已安装 Tauri CLI：

```powershell
cargo install tauri-cli --version "^2" --locked
```

**构建命令**：

```powershell
# 从项目根目录执行
cd crates/gui

# 构建 NSIS 安装包（推荐，Windows 原生支持）
cargo tauri build --ci --bundles nsis
```

> **说明**：
> - `--ci` 跳过交互式提示，适合脚本化构建
> - `--bundles nsis` 指定使用 NSIS 打包格式（WiX Toolset v3 非 GitHub Windows runner 预装，本地开发如有 WiX 可改用 `msi`）
> - `beforeBuildCommand`（配置在 `tauri.conf.json`）会自动执行 `pnpm build` 编译前端，无需手动预先构建

**产物路径**：

| 产物 | 路径 |
|------|------|
| 安装包 (.exe) | `target/release/bundle/nsis/Loom Desktop_0.1.0_x64-setup.exe` |
| 可执行文件 | `target/release/loom-gui.exe` |

---

## 8. 验证方法

### Server 验证

```powershell
.\target\release\loom-server.exe --version
```

### Daemon 验证

```powershell
.\target\release\loom-daemon.exe --version
```

### CLI 验证

```powershell
.\target\release\loom.exe --help
```

### Shell 验证

```powershell
.\target\release\loom-shell.exe --version
```

### GUI 验证

直接运行安装包完成安装，或运行：

```powershell
.\target\release\loom-gui.exe
```

### 一键构建所有组件

```powershell
cargo build -p loom-server -p loom-daemon -p loom -p loom-shell --release
```

---

## 9. 常见问题

### 9.1 `error: linker 'link.exe' not found`

**原因**：未安装 Visual Studio C++ Build Tools 或未正确配置 MSVC 工具链。

**解决**：
```powershell
# 确认已安装 MSVC 构建工具
where link.exe
# 如未找到，重新运行 Visual Studio Installer，安装 "使用 C++ 的桌面开发" 工作负载
```

### 9.2 `error: could not compile openssl-sys`

**原因**：Tauri 依赖 OpenSSL C 库，需要系统级编译工具。

**解决**：
```powershell
# 方案 A：安装 vcpkg 并安装 OpenSSL
git clone https://github.com/Microsoft/vcpkg.git C:\vcpkg
cd C:\vcpkg
.\bootstrap-vcpkg.bat
.\vcpkg install openssl:x64-windows-static
set OPENSSL_DIR=C:\vcpkg\packages\openssl_x64-windows-static

# 方案 B：安装 Strawberry Perl（提供 gcc 工具链）
winget install StrawberryPerl.StrawberryPerl
```

### 9.3 `mingw-w64-gcc not found`

**原因**：部分 Rust crate 的 C 依赖需要 GCC 编译器（`openssl-sys`、`ring` 等）。

**解决**：
```powershell
# 方案 A：安装 MSYS2 + MinGW-w64
winget install MSYS2.MSYS2
# 打开 MSYS2 UCRT64 终端，执行：
pacman -S mingw-w64-ucrt-x86_64-gcc

# 方案 B：使用 vcpkg（见 9.2）
```

### 9.4 路径过长错误 (MAX_PATH / Error 206 / os error 3)

**原因**：agent ID、thread ID、channel ID 拼接的目录路径超过 Windows MAX_PATH 260 字符限制。

**状态**：✅ **已修复**（Phase 1/2/3，commit 58b3972 及之前）。项目已在所有 `create_dir_all` 调用点应用 UNC 前缀（`\\?\`），覆盖 27 个生产路径站点：

- **Phase 1**：spawn 命令路径 + cwd（`acp.rs`）
- **Phase 2**：用户命令 spawn（`command.rs`）
- **Phase 3a**：4 个运行时热路径（`ensure_session` / `start_blocking` / `spawn_and_collect` / `run_prompt_inner`）
- **Phase 3b**：5 个 agent host 启动路径（`agent_serve.rs`）
- **Phase 3c**：18 个持久化/冷路径（`command.rs`、`interactive.rs`、`profile.rs`、`agent_serve.rs`）

> **注意**：如果仍遇到路径过长问题，请确保使用最新 `dev` 分支代码，或手动启用 Windows 长路径支持：
> ```powershell
> # 管理员 PowerShell
> New-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" `
>   -Name "LongPathsEnabled" -Value 1 -PropertyType DWORD -Force
> ```

### 9.5 `pnpm install` 失败

**原因**：pnpm workspace 配置或 lockfile 版本不匹配。

**解决**：
```powershell
# 确保 pnpm 版本正确
pnpm --version  # 应为 9.15.0

# 清理后重试
rm -r node_modules apps/gui-web/node_modules
pnpm install --frozen-lockfile
```

### 9.6 GUI 构建时 `beforeBuildCommand` 失败

**原因**：前端依赖未安装或 TypeScript 编译错误。

**解决**：
```powershell
# 先单独验证前端构建
cd apps/gui-web
pnpm install --frozen-lockfile
pnpm build

# 确认 dist 目录有产出
ls dist/index.html
```

### 9.7 NSIS 安装包构建失败

**原因**：NSIS 工具未安装。

**解决**：
```powershell
winget install NSIS.NSIS
# 或将 --bundles nsis 改为 --bundles msi（需 WiX Toolset v3）
```

---

## 10. 产物路径速查

| 组件 | Cargo 包名 | 构建命令 | 产物路径 |
|------|-----------|----------|----------|
| Server | `loom-server` | `cargo build -p loom-server --release` | `target/release/loom-server.exe` |
| Daemon | `loom-daemon` | `cargo build -p loom-daemon --release` | `target/release/loom-daemon.exe` |
| CLI | `loom` | `cargo build -p loom --release` | `target/release/loom.exe` |
| Shell | `loom-shell` | `cargo build -p loom-shell --release` | `target/release/loom-shell.exe` |
| GUI 安装包 | `loom-gui` | `cd crates/gui && cargo tauri build --ci --bundles nsis` | `target/release/bundle/nsis/Loom Desktop_0.1.0_x64-setup.exe` |
| GUI 可执行 | `loom-gui` | （同上） | `target/release/loom-gui.exe` |

> **注意**：所有 `cargo build` 命令从项目根目录执行。`cargo tauri build` 必须从 `crates/gui/` 目录执行，因为 `tauri.conf.json` 中的 `beforeBuildCommand.cwd` 路径相对于该目录解析。

---

## 已知限制

1. **GitHub Actions CI 不适用**：当前 GitHub Actions 构建配额不足，本文档面向本地 Windows 构建。CI 配置（`.github/workflows/gui-build.yml`）已包含 NSIS 打包和 artifact 上传逻辑，配额恢复后可复用。
2. **Tauri NSIS 安装包仅 Windows 产出**：macOS/Linux 需要各自平台构建（`--bundles dmg` / `--bundles deb`）。
3. **GUI 构建时间较长**：首次构建需下载所有 Rust 依赖 + 前端 npm 包 + Tauri CLI，预计 15-30 分钟（取决于网络和机器性能）。
4. **Windows 构建路径建议短路径**：尽管项目已做 UNC 前缀修复，仍建议将项目克隆到短路径（如 `C:\src\joi-apps`）以避免超出系统级路径限制。
5. **loom-shell 与 GUI 的关系**：`loom-shell.exe` 是独立进程，不嵌入 Tauri GUI。GUI 通过进程间通信调用 shell。如果只构建 GUI 安装包，shell 不会自动包含 — 需单独分发或集成到安装脚本。
