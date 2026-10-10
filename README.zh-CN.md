# Akagi Web

[English](README.md) | **简体中文**

Akagi Web 是用于雀魂和天凤网页端的麻将助手，使用内置模型在本机分析牌局。

- 支持三麻和四麻，提供出牌与鸣牌建议。
- 默认开启悬浮窗，可在游戏旁查看建议并调整字号和透明度。
- 可在设置中开启自动打牌。
- 对局结束后可在历史页面查看记录。

## 与上游的差异

本项目基于 [Akagi v3](https://github.com/shinkuan/Akagi/tree/v3)，面向本地网页对局使用。

| 项目 | 本维护版的调整 |
|---|---|
| 游戏与采集 | 聚焦雀魂、天凤官方网页，通过 Edge/Chrome 读取牌局，省去代理和证书配置。 |
| AI 与服务 | 内置三麻、四麻模型，本机运行；云推理、云复盘、订阅和外部机器人已移除。 |
| 诊断与隐私 | 诊断日志脱敏，保留本地分析、历史和日志查看。 |
| 安装与更新 | 提供 Windows、macOS、Linux 安装包，更新使用本仓库发布页。 |

## 安全与隐私

AI 在本机推理，历史记录本地保存，Akagi 不上传账号、牌局或日志；诊断日志会对敏感信息脱敏。
网络访问用于游戏连接、更新检查和用户主动下载浏览器。浏览器调试连接仅使用本机地址，并遵循浏览器授权。
启用自动打牌前，请了解所用游戏平台的规则。更多说明见[维护指南](docs/FORK_MAINTENANCE.zh-CN.md)。

## 下载安装

从[发布页](https://github.com/Etakind/Akagi-web/releases)下载适合系统和处理器的安装包。

| 系统 | 处理器 | 下载文件 |
|---|---|---|
| Windows | x64 | `windows-x64.zip` |
| macOS | Intel | `macos-x64.zip` |
| macOS | Apple Silicon | `macos-arm64.zip` |
| Linux | x64 / ARM64 | 对应的 ZIP、DEB 或 RPM |

ZIP 解压后，Windows 双击 `akagi.exe`，macOS/Linux 运行 `./akagi`。
Windows 需要安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。
macOS 首次打开时，如出现安全提示，可在“系统设置 → 隐私与安全性”中允许打开。

Ubuntu/Debian 安装 DEB，Fedora 安装 RPM，系统会同时安装所需依赖：

```sh
sudo apt install ./akagi-*.deb
```

```sh
sudo dnf install ./akagi-*.rpm
```

Linux ZIP 用户需先安装对应系统的 GTK/WebKitGTK 依赖，参见下方源码安装说明。

## 首次使用

1. 打开 Akagi，在设置向导中选择游戏、浏览器和采集方式，点击“保存”。
2. 在游戏浏览器中登录雀魂或天凤，保持该游戏的一个标签页打开。
3. 开始对局，在“对局”页面或悬浮窗中查看建议。自动打牌可在“设置”中开启。

独立浏览器可沿用登录状态，会话过期时重新登录；移动程序时，一并保留 `chrome-profile` 目录。
悬浮窗和浏览器选项都可以在“设置”中调整，设置向导也可重新运行。

雀魂对局中才启动 Akagi 时，启动采集并点击“恢复并继续”，按界面提示完成恢复。

## 浏览器配置

**独立浏览器（默认）：** “常用浏览器调试端口”设为 `0`，选择已安装的 Edge 或 Chrome，用户数据目录留空即可。
也可通过设置下载 Chrome for Testing。Akagi 会打开独立的游戏窗口。

**使用已有浏览器：** 在 Edge/Chrome 中开启本地远程调试，记下端口；
在 Akagi 设置中填入“常用浏览器调试端口”（例如 `9222`），重新启动采集，并允许浏览器弹出的调试连接请求。
使用自定义浏览器目录时，填写用户数据根目录。

## 源码安装

编译需要 Git、最新 stable Rust、Node.js/npm、`protoc` 和 C/C++ 工具链。
推荐 Node.js **24 LTS**，最低 **22.12**。展开对应系统，先检查已有环境，再补齐缺少的工具。检查命令有版本输出表示工具可用；提示找不到命令时先检查 PATH。
首次运行后按上方“首次使用”完成设置，浏览器配置入口位于“设置”。

<details>
<summary>macOS（Intel / Apple Silicon）</summary>

Intel 与 Apple Silicon 均使用本机原生终端。每项先检查，仅补齐缺少的工具。

**1. 编译工具：** 先查看 Xcode 工具目录和编译器版本。均正常时继续下一项；工具缺失时执行 `xcode-select --install`，等待安装完成。

```sh
xcode-select -p
clang --version
```

**2. Homebrew：** 下面会检查当前 PATH 和两个常见安装位置。有版本输出即已安装，可以跳过安装器；位于常见目录的已有安装会同时加载到当前终端。

```sh
if command -v brew >/dev/null 2>&1; then
  brew --version
elif [ -x /opt/homebrew/bin/brew ]; then
  eval "$(/opt/homebrew/bin/brew shellenv)"
  brew --version
elif [ -x /usr/local/bin/brew ]; then
  eval "$(/usr/local/bin/brew shellenv)"
  brew --version
else
  echo "Homebrew not found in PATH or standard locations"
fi
```

显示 `Homebrew not found` 时，如曾安装到自定义位置，先按实际路径加载 `brew shellenv`；确认未安装后才运行 [Homebrew 安装器](https://docs.brew.sh/Installation)：

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

完成后按安装器的 **Next steps** 配置终端，重新执行上面的检查。

**3. Git、Node.js/npm、protoc、CMake：** 查看版本。Node.js 达到 22.12 且 npm 可用时，可以沿用；推荐 24 LTS。

```sh
git --version
node -v
npm -v
protoc --version
cmake --version
```

如果 `node` 找不到，先确认是否已安装 `node@24`；有版本输出时加载 PATH，再检查 `node -v`、`npm -v`：

```sh
brew list --versions node@24
export PATH="$(brew --prefix node@24)/bin:$PATH"
```

仅安装缺少的包：`git`、`node@24`（含 npm）、`protobuf`（提供 protoc）、`cmake`。以下命令包含全部包，可按检查结果保留需要的包名：

```sh
brew install git node@24 protobuf cmake
```

将 Homebrew 的 `shellenv` 和需要的 Node.js PATH 配置加入 `~/.zprofile`（bash 使用 `~/.bash_profile`），供新终端加载。

**4. Rust：** 先加载已有环境并检查。`rustc`、`cargo` 均可用时沿用现有安装；使用 rustup 管理的旧版可执行 `rustup update stable` 和 `rustup default stable`。

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

加载后仍找不到 Rust 时，才运行 [Rust 安装器](https://rust-lang.org/tools/install/)：

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

环境准备好后，获取源码、编译并运行：

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Windows x64（PowerShell）</summary>

**1. 检查已有工具。** 已安装 Visual Studio/Build Tools 时，从开始菜单打开 **Developer PowerShell**；先执行：

```powershell
git --version
node -v
npm.cmd -v
rustup --version
rustc -vV
cargo --version
protoc --version
cmake --version
nasm -v
where.exe cl
```

有版本或路径输出的工具可沿用。Node.js 至少 22.12，推荐 24 LTS；Rust 的 host 应为 `x86_64-pc-windows-msvc`。找不到 `cl` 时，先确认当前使用的是 Developer PowerShell。

如果仅 Rust 命令找不到，检查默认安装目录；返回 `True` 时先补入当前终端的 PATH，再重试 Rust 检查：

```powershell
Test-Path "$env:USERPROFILE\.cargo\bin\rustc.exe"
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

**2. 补齐缺少的工具。** 安装时保留 PATH 选项；安装完成或修改 PATH 后重新打开终端，再执行上面的检查。

- [Git](https://git-scm.com/install/windows) 和 [Node.js 24 LTS](https://nodejs.org/en/download)，保留 npm 和 PATH 选项。
- [C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)：勾选“使用 C++ 的桌面开发”、MSVC x64/x86 工具及 Windows SDK。
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) 和 [CMake](https://cmake.org/download/)。
- [NASM](https://www.nasm.us/pub/nasm/releasebuilds/)，将可执行文件目录加入 PATH。从 [protobuf 发布页](https://github.com/protocolbuffers/protobuf/releases)下载 win64 ZIP，解压到 `C:\Tools\protobuf`，将 `C:\Tools\protobuf\bin` 加入 PATH。
- [Rust](https://rust-lang.org/tools/install/)：运行 `rustup-init.exe`，选择 stable MSVC。

WebView2 可在 Windows 的“已安装的应用”中查看；已具备运行环境时可跳过。已有 Visual Studio 时，在其 Installer 中核对 C++ 工作负载和 Windows SDK。已有 rustup 时可用 `rustup update stable-msvc`、`rustup default stable-msvc` 更新工具链。

环境准备好后，在同一 PowerShell 中获取源码、编译并运行：

```powershell
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm.cmd ci --prefix frontend
npm.cmd run build --prefix frontend
cargo build --locked --release --features custom-protocol
.\target\release\akagi.exe
```

</details>

<details>
<summary>Ubuntu / Debian</summary>

在桌面环境中打开终端。

**1. 检查系统工具和图形库：**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

下面的包管理命令用于补齐完整系统依赖，已安装的包由包管理器检查，无需卸载重装：

```sh
sudo apt update
sudo apt install -y git curl ca-certificates xz-utils file build-essential cmake pkg-config \
  libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev protobuf-compiler
```

**2. 检查 Rust：** 先加载已有安装；`rustc`、`cargo` 正常时可跳过安装器。已有 rustup 可用 `rustup update stable`、`rustup default stable` 更新工具链。

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

仅在加载后仍缺少 Rust 时安装：

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. 检查 Node.js/npm 和 nvm：** 已有 Node.js 22.12+ 且 npm 可用时，跳过这一项的安装命令。下面也会加载已有 nvm：

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

缺少 Node.js 或版本过旧时使用 [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) 安装 24 LTS。`command -v nvm` 输出 `nvm` 时，跳过下面第一行：

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

重新执行 `node -v`、`npm -v` 确认环境，然后获取源码、编译并运行：

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Fedora</summary>

在桌面环境中打开终端。

**1. 检查系统工具和图形库：**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

下面的包管理命令用于补齐完整系统依赖，已安装的包由包管理器检查，无需卸载重装：

```sh
sudo dnf install -y git curl ca-certificates xz file gcc gcc-c++ make cmake \
  pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel gtk3-devel \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel protobuf-compiler
```

**2. 检查 Rust：** 先加载已有安装；`rustc`、`cargo` 正常时可跳过安装器。已有 rustup 可用 `rustup update stable`、`rustup default stable` 更新工具链。

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

仅在加载后仍缺少 Rust 时安装：

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. 检查 Node.js/npm 和 nvm：** 已有 Node.js 22.12+ 且 npm 可用时，跳过这一项的安装命令。下面也会加载已有 nvm：

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

缺少 Node.js 或版本过旧时使用 [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) 安装 24 LTS。`command -v nvm` 输出 `nvm` 时，跳过下面第一行：

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

重新执行 `node -v`、`npm -v` 确认环境，然后获取源码、编译并运行：

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Arch Linux</summary>

在桌面环境中打开终端。

**1. 检查系统工具和图形库：**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

下面的包管理命令用于补齐完整系统依赖，已安装的包由包管理器检查，无需卸载重装：

```sh
sudo pacman -Syu --needed git curl ca-certificates xz file base-devel cmake pkgconf \
  openssl webkit2gtk-4.1 gtk3 appmenu-gtk-module libappindicator-gtk3 \
  librsvg xdotool protobuf
```

**2. 检查 Rust：** 先加载已有安装；`rustc`、`cargo` 正常时可跳过安装器。已有 rustup 可用 `rustup update stable`、`rustup default stable` 更新工具链。

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

仅在加载后仍缺少 Rust 时安装：

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. 检查 Node.js/npm 和 nvm：** 已有 Node.js 22.12+ 且 npm 可用时，跳过这一项的安装命令。下面也会加载已有 nvm：

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

缺少 Node.js 或版本过旧时使用 [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) 安装 24 LTS。`command -v nvm` 输出 `nvm` 时，跳过下面第一行：

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

重新执行 `node -v`、`npm -v` 确认环境，然后获取源码、编译并运行：

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

开发模式和打包参见[开发指南](docs/BUILDING.zh-CN.md)。

基于 [shinkuan/Akagi](https://github.com/shinkuan/Akagi/tree/v3)。许可证与第三方声明：[LICENSE.txt](LICENSE.txt)、[NOTICE](NOTICE)。
