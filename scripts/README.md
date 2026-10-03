# 开发工具

从仓库根目录执行，应用不依赖 Python 运行时。

- `setup-fork.py` / `test_setup_fork.py`：安装本地防误推配置及后续离线回归。
- `package.py --matrix`：读取 `build/targets.json`；`--target <triple>` 打包已有同目标二进制，生成资产清单与 SHA256。
- `package-zip.sh <triple>`：上述标准库打包器的兼容入口。Linux 还需先用 Tauri 生成 DEB/RPM。
- `test_web_only.py`：后续检查运行功能边界和五目标清单；本次没有执行。
- `audit_dependencies.py`：查询公开依赖元数据；历史快照不能代替新版本审计。
- `extract_liqi.py`：人工协议工具；日常协议定义随上游同步。

打包/审计使用 Python 3.11+，Git 配置脚本使用 Python 3 标准库。Python/uv 不随应用分发。
旧复制浏览器会话工具已经删除；常用 Edge 附加只读取 DevToolsActivePort，不需要复制会话。

Rust 手动工具：`browser_probe` 检查实际浏览器接入，`replay_session_analysis` 分析本地回放；
`login_acceptance` 与 `audit_session_logs` 涉及本机内存凭据检查，只在用户明确授权时运行，绝不用于 CI。
本次没有读取 account 或调用凭据工具。页面订阅成功也不能代表牌局/自动操作通过。
