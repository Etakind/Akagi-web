# 开发与打包

安装和日常使用请参阅 [README](../README.zh-CN.md)。

README 中的 Cargo 命令生成嵌入前端的 release 程序，运行时不需要 Node.js、Rust、protoc 或前端开发服务器。不要省略 custom-protocol，否则普通 Cargo 构建会指向开发服务器。已安装编译依赖并进入项目根目录后，开发模式使用：

```sh
npm exec --prefix frontend -- tauri dev
```
Windows PowerShell 将 npm 写为 npm.cmd。tauri dev 会启动 Vite，不要将它与直接运行 release 程序混用。

仅运行 Python 打包及检查脚本时才需要 Python 3.11+。macOS 可用 brew install python，Windows 使用 [Python 官方安装器](https://www.python.org/downloads/windows/)并启用 PATH，Linux 使用发行版提供的 Python 3.11+，先核对 python3 --version；Ubuntu 22.04 默认的 3.10 不满足脚本要求，需按 [Python 官方 Unix 安装说明](https://docs.python.org/3/using/unix.html)另装 3.11+。下述 python3 若指向旧版，应改用已安装的 python3.12 等命令。Linux DEB/RPM 打包另需 patchelf 和 rpm；Ubuntu/Debian 使用 apt install patchelf rpm，Fedora 使用 dnf install patchelf rpm，Arch 使用 pacman -S patchelf rpm-tools。

目标三元组和安装包格式以 [build/targets.json](../build/targets.json) 为准。下面仅为 Apple Silicon 原生打包示例；Intel Mac 应使用 x86_64-apple-darwin，Windows 使用 x86_64-pc-windows-msvc：

```sh
rustup target add aarch64-apple-darwin
npm exec --prefix frontend -- tauri build --no-bundle --target aarch64-apple-darwin
python3 scripts/package.py --target aarch64-apple-darwin
```
Windows 使用 py -3 运行脚本。Linux 在匹配 x86_64/ARM64 主机上使用对应 GNU 三元组，并将 tauri build 的 --no-bundle 替换为 --bundles deb,rpm，然后运行相同打包脚本。添加 Rust target 不等于安装跨系统 SDK/链接器，也不代表自动支持交叉编译。其他平台的步骤经文档与 CI 核对，不等同于在本机完成安装验证。

系统依赖参考：[Tauri 前置要求](https://v2.tauri.app/start/prerequisites/)、[AWS-LC 构建要求](https://aws.github.io/aws-lc-rs/requirements/)、[nvm](https://github.com/nvm-sh/nvm#installing-and-updating)。
