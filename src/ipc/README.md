# 桌面界面接口

`commands.rs` 注册 Tauri 命令；`state.rs` 持有共享状态；`capture_supervisor.rs` 管理浏览器采集；`overlay.rs` 管理建议悬浮窗。

保存浏览器配置会重启采集；关闭 capture.enabled 会停止采集。旧模式提示 unavailable_reason，不能启动被移除后端。启动/停止状态的 kind 仅为 chromium。

Rust 与前端类型必须同步。日志与 Inspector 广播统一脱敏；游戏原始帧不得进入通知或错误文本。机器人安装、云服务等保留独立的主动操作接口。
