# 开发与验收工具

所有命令从仓库根目录运行。

- `setup-fork.py` / `test_setup_fork.py`：安装及离线验证本地防误推配置。
- `test_web_only.py`：验证双平台打包边界、不支持目标无文件副作用、废弃运行模块未恢复。
- `fetch-runtime.sh <target>` / `package-zip.sh <target>`：仅支持 aarch64-apple-darwin、x86_64-pc-windows-msvc，供手动构建使用。
- `audit_dependencies.py`：查询公开依赖元数据并更新根审计快照，不读取凭据。
- `extract_liqi.py`：手动协议工具；日常协议更新优先随上游同步。
- `prepare_edge_game_profile.py`：历史手动独立会话准备工具；常用 Edge 附加模式无需复制会话。只在明确需要独立目录时主动使用。

Rust examples 中 login_acceptance 是手动凭据验收，audit_session_logs 在本地内存检查凭据泄露，replay_session_analysis 用于牌局回放。不得在 CI 调用真实账号工具，不得打印 account 或会话内容。

本次主用已安装浏览器及内置机器人，不自动下载或安装外部机器人。虚拟环境不是安全沙箱。

`RUST_LOG=off cargo run --locked --example browser_probe -- attach` 检查本机 9222；`isolated` 使用临时无登录 Edge 目录。
该工具运行实际 Chromium 后端，输出固定结果码，诊断沿用项目脱敏；不读取 account、复制 Cookie、登录、刷新游戏或执行任何点击。
`BROWSER_PAGE_SUBSCRIBED` 只证明官方页面已订阅，不证明完成牌局或自动操作验收。
依赖审计工具需要 Python 3.11+ 的 tomllib；Git 配置及裁剪离线测试仅需 Python 3 标准库。
