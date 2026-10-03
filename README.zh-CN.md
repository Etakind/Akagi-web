# Akagi — 本地网页维护版

[English](README.md) | **简体中文**

本仓库是 [shinkuan/Akagi v3](https://github.com/shinkuan/Akagi/tree/v3) 的维护分支，
仅通过 **Majsoul、Tenhou 官方网页端**采集，使用内置本地模型。

## 相较 upstream 的区别

| 范围 | 本维护版 |
|---|---|
| 采集 | 仅 Chromium CDP：附加常用 Edge/Chrome，或启动独立浏览器；无 MITM、根证书或系统代理。 |
| 推理 | 仅内置四麻/三麻模型；删除云推理、云复盘/分享、API 密钥、订阅支付及外部 Python 机器人。 |
| 诊断 | 协议/HTTP 元数据脱敏，保留本地分析、历史和 Inspector；不保存登录原始帧、敏感头及正文。 |
| 自动打牌 | 主动开启，绑定唯一官方页面与当前决策状态；失败不会自动刷新游戏。 |
| 更新 | 查询个人维护仓库，手动安装；不允许上游安装包覆盖本构建。 |
| 分发 | 下列五种目标配置；不含 Python/uv，不提供 AppImage 或移动客户端。 |

这些调整减少凭据落盘，删除远端推理/上传路径，避免依赖拦截 CA 或下载的可执行机器人。
CDP 仍具有较高的浏览器访问权限。实现边界、迁移、残余风险和上游合并规则见
[维护指南](docs/FORK_MAINTENANCE.zh-CN.md)。

Akagi 不向远端上传账号、牌局、历史、日志或推理数据；推理全部在本地完成。
保留更新检查和用户主动下载。游戏网页仍正常连接游戏服务器，自动操作也通过游戏客户端执行。


## 游戏与功能

| 官方网页 | 功能 |
|---|---|
| [Majsoul](https://game.maj-soul.com/1/) | 三麻/四麻解析、本地建议、悬浮窗、历史、Inspector、可选自动打牌 |
| [Tenhou](https://tenhou.net/4/) | 三麻/四麻解析、本地建议、历史及 PT 统计、Inspector、可选自动打牌 |

Tenhou 自动打牌需要客户端适配入口，仅开启自动打牌时准备脚本适配。
附加已加载页面而缺少入口时，请在安全时机自行重新进入或刷新，Akagi 不代为操作。
客户端结构变化或当前局状态不完整时停止操作，等待下一完整局。
历史记录在整场结束后完成落盘，不是每一小局结束后立即出现。

## 安装包和运行

| 系统 | CPU | 安装包格式 |
|---|---|---|
| Windows | x86_64 | `windows-x64.zip` |
| macOS | x86_64 | `macos-x64.zip` |
| macOS | ARM64 | `macos-arm64.zip` |
| Linux | x86_64 | `linux-x64.zip`、DEB、RPM |
| Linux | ARM64 | `linux-arm64.zip`、DEB、RPM |

从[个人维护版发布页](https://github.com/Etakind/Akagi/releases) 获取产物，需要正常的私有仓库权限。
核对 SHA256；附带 minisign 签名时使用相应公钥核验。

ZIP 解压到当前用户拥有的目录：Windows 运行 `akagi.exe`（需要 WebView2），macOS/Linux 运行 `./akagi`。
Linux 还需 GTK/WebKitGTK 系统库，ZIP 不包含完整系统环境。DEB 面向 Ubuntu 22.04/24.04、Debian 12/13，
RPM 安装说明面向 Fedora；分别执行 `sudo apt install ./akagi-*.deb` 或 `sudo dnf install ./akagi-*.rpm`。
Arch 采用源码构建。安装包兼容性取决于系统库版本。

程序可能未签名/未公证，允许系统执行前核对来源；不要关闭浏览器 TLS、沙箱或全局移除系统隔离保护。
可写便携目录通常在程序旁保存数据；只读系统安装使用用户配置/数据目录。保留显式配置路径。

## 浏览器接入

**常用 Edge：** 开启本地远程调试服务，确认回环端口（如 `127.0.0.1:9222`）。在 Akagi 设置中选择游戏，
附加端口填写 `9222`，重启采集，允许浏览器这一次的调试连接。保留所选游戏唯一的官方页面，尽量在进入一局之前启动 Akagi。

用户数据目录可以留空。标准 `/json/version` 不可用时（例如 Edge 界面开启的调试服务），
Akagi 只检查已知目录中的 `DevToolsActivePort`。指定浏览器程序后，自动发现仅限该浏览器家族。
端口不符、文件不安全或候选不唯一时不会随意选取。自定义目录请填**用户数据根目录**，不是 `Default`
等单个配置子目录。不会读取或复制 Cookie、登录数据库及会话内容；重连会重新发现地址。

**独立浏览器：** 附加端口设为 `0`。目录留空使用 Akagi 隔离目录，或指定单独目录，不要使用常用浏览器目录。
优先选择已安装浏览器；缺失时可在设置中主动下载官方 Chrome for Testing，采集和推理启动时不会自动下载。

端口/定位文件缺失、授权拒绝/超时、缺少游戏页面、缺少当前局状态分别处理。每次连接可能需要重新授权。
Akagi 不反复重试 403，不绕过 Origin 检查，也不自动刷新进行中的游戏。

## 源码构建

准备 Git、稳定版 Rust、Node.js **22+**、npm、工具所需 Python **3.11+**、Protocol Buffers `protoc`。
Python 不作为应用运行时。系统工具链参见 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)。

macOS 在对应 Intel/Apple Silicon 主机原生构建：

```sh
xcode-select --install
brew install node protobuf
```

Windows 安装 Visual Studio Build Tools 的“使用 C++ 的桌面开发”和 Windows SDK、WebView2、Rust MSVC
工具链、Node.js，将 `protoc` 加入 PATH。在 PowerShell 中：

```powershell
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
.\target\release\akagi.exe
```

Ubuntu/Debian：

```sh
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev protobuf-compiler patchelf
```

Fedora 安装 GCC/C++、`pkgconf-pkg-config`、`openssl-devel`、`webkit2gtk4.1-devel`、`gtk3-devel`、
`libappindicator-gtk3-devel`、`librsvg2-devel`、`libxdo-devel`、`protobuf-compiler`。
Arch 安装 `base-devel`、`pkgconf`、`openssl`、`webkit2gtk-4.1`、`gtk3`、`libappindicator-gtk3`、
`librsvg`、`xdotool`、`protobuf`，以及 Rust 和 Node.js/npm。随后 macOS/Linux 运行：

```sh
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

`custom-protocol` 用于嵌入生产前端；缺失时普通 Cargo release 构建仍指向开发服务器。
准确目标三元组见 [build/targets.json](build/targets.json)。在对应主机打包，例如：

```sh
npm exec --prefix frontend -- tauri build --no-bundle --target aarch64-apple-darwin
python3 scripts/package.py --target aarch64-apple-darwin
```

Linux 使用 `tauri build --target <三元组> --bundles deb,rpm` 后运行相同打包脚本。
工作流采用 Ubuntu 22.04 [Tauri 较旧构建基线](https://v2.tauri.app/distribute/appimage/)，不生成 AppImage。
这些原生构建命令不承诺自动完成跨系统交叉编译。

## 风险

CDP 可访问浏览器会话，只开放回环地址，不再使用时关闭。自动打牌可能违反游戏规则并带来账号处罚；
网页变化也可能使适配失效。本地历史/日志仍需保护，未签名程序和依赖/系统库问题也须审查。
Linux glib 的已有公告需继续处理，详见[风险与改进方向](docs/FORK_MAINTENANCE.zh-CN.md)。


许可证与归属：[LICENSE.txt](LICENSE.txt)、[NOTICE](NOTICE)。
