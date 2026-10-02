# Akagi 雀魂网页维护版

这是 [Etakind/Akagi](https://github.com/Etakind/Akagi) 的个人维护分支，基于 [shinkuan/Akagi](https://github.com/shinkuan/Akagi) 开发。
仅支持 **雀魂网页端**；正式构建提供 **macOS Apple Silicon / Windows x64**。Akagi 本身仍是桌面程序。

保留实时牌局解析、内置本地机器人建议、分析、悬浮窗、历史记录、脱敏 Inspector，以及用户主动开启的自动打牌。
不提供 MITM、根证书管理、雀魂原生客户端、天凤、一番街、Linux 应用或移动端版本。

- [仓库维护、上游同步与差异表](docs/FORK_MAINTENANCE.md)
- [安全行为与历史验证记录](SECURITY_HARDENING.md)
- [开发与验收工具](scripts/README.md)

## 常用 Edge 接入

1. 在 Edge 开启仅本机访问的远程调试，确认页面显示 `127.0.0.1:9222`。若显示 `starting…`，服务尚未就绪。
2. 在这个 Edge 实例打开官方雀魂页面，例如 `https://game.maj-soul.com/1/`，正常登录。
3. 启动 Akagi，在设置的浏览器采集中填写调试端口 `9222`，启用采集并保存。浏览器出现本机调试授权提示时允许连接。
4. 只保留一个官方游戏页供自动操作绑定。建议先在大厅接通，再进入人机局；中途接入缺少初始状态时，按提示通过游戏自身恢复连接。
5. 确认牌局和建议正常后，可单独开启自动打牌。接入或点击失败时 Akagi 不自动刷新正在进行的对局。

附加模式不复制 Cookie，不要求提供账号密码，也不会在停止采集或退出 Akagi 时关闭常用 Edge。
403 表示调试连接被拒绝，不能通过关闭 TLS 校验解决；确认授权及调试服务后手动重启采集。

## 独立浏览器模式

将调试端口设为 `0`，选择本机已安装的 Edge / Chrome，启动独立浏览器目录并在其中登录。
设置中的配置目录仅用于此独立会话；不要把常用浏览器目录当作独立目录交给 Akagi 管理。
附加模式指定目录时只读取其 `DevToolsActivePort`。没有系统浏览器时，可主动下载官方 Chrome for Testing。
此模式无需安装根证书或配置系统代理，也不使用关闭沙箱、忽略证书错误的参数。

## 功能与数据

- 内置四麻、三麻机器人无需另行安装。手动安装的外部机器人以当前用户权限执行，Python 虚拟环境不是安全沙箱。
- 云推理默认关闭，云复盘需要主动提交。开启云服务会向相应服务发送推理或复盘数据。
- 悬浮窗、日志级别、自动操作时序等在设置中调整。历史列表在整场结束后写入，一局结束不等于整场结束。
- 新日志仅记录脱敏元数据；原始 WS 帧、HTTP 正文、认证头和 URL 查询不落盘或广播。MJAI 和历史仍包含牌局信息，应按个人数据管理。
- 上游版本提示仅供参考；禁止上游更新包覆盖本维护版。个人构建通过 Git 同步及手动构建更新。

旧 `proxy.enabled` 仅作为 `capture.enabled` 的读取兼容项，显式新值优先；保存设置时清理旧代理字段。
旧 MITM 或其他游戏配置会停止采集并显示提示，机器人等无关设置保留。旧历史与 Inspector 文件仍可读取，不会被自动改写。

## 开发与构建

需要 Rust、Node.js/npm、Python 3、Git、Protobuf 编译器；Windows 使用 MSVC 构建工具和 WebView2。
本地源码可在 macOS 开发工具链上回归；CI 分别验证正式目标，不能将本地结果当作 Windows 实机证明。

```sh
git clone --branch dev git@github.com:Etakind/Akagi.git
cd Akagi
python3 scripts/setup-fork.py
cd frontend
npm ci
npm run build
cd ..
cargo test --locked --all-targets
cargo test --locked --manifest-path native_bot/Cargo.toml
cargo build --locked --release --features custom-protocol
```

运行 `target/release/akagi`（Windows 为 `akagi.exe`）。开发界面使用 `cargo tauri dev`，需要 Tauri CLI。
手动 Actions 构建默认只保存构建产物，不创建发布；打包脚本仅接受 `aarch64-apple-darwin` 与 `x86_64-pc-windows-msvc`。
Linux 可跑前端、Python 等通用 CI，不构建应用。

## 本地遗留文件

`account`、配置、浏览器目录、CA 私钥、日志和历史均不提交。删除 MITM 代码不等于删除本机旧 CA 或撤销系统信任。
旧 `ca/`、自定义证书目录、系统中手工信任的 Akagi 证书、旧日志及导出副本需用户自行核查处理；本项目不会代为删除。
保留的 `LICENSE.txt`、`NOTICE` 及第三方归属信息继续适用。
