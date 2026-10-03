# 桌面接口

`commands.rs` 注册本地 Tauri 命令，`state.rs` 持有共享状态，`capture_supervisor.rs` 管理浏览器采集，`overlay.rs` 管理建议悬浮窗。
浏览器/游戏/自动操作开关变化使旧绑定失效并重启采集；关闭 capture.enabled 停采，不自动刷新游戏。
Rust 与前端类型同步，日志/Inspector 广播脱敏。云服务、外部机器人和更新覆盖安装命令已经删除。
保留个人版本查询及用户主动下载官方浏览器，所有 HTTP 客户端由 src/network.rs 按用途创建。
