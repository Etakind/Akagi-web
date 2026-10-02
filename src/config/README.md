# 配置与兼容

`AppConfig` 提供通用、日志、雀魂平台、机器人、浏览器采集、自动操作、悬浮窗和下载配置。完整使用说明见根 README。

`capture.enabled` 控制自动启动；缺失时读取旧 `proxy.enabled`。模式固定 `chromium`，游戏固定 `Majsoul`。旧 MITM 或其他游戏配置标为不可用并暂停采集，不重置无关设置。新模式从 Rust 默认配置到首次设置向导保持一致。

`merge.rs` 在保存时合并已知字段、保留未知字段和注释，删除已废弃的已知代理字段。配置读取错误不输出原始内容。`capture.http.record_all` 仅扩大脱敏元数据范围；旧 `bodies=true` 不恢复正文。

浏览器参数不能关闭沙箱或证书校验，调试接口仅允许回环；云推理默认关闭。敏感配置 Debug 脱敏；文件经 private_fs 写入。配置解析和保存须同时验证 TOML 与 IPC JSON。
