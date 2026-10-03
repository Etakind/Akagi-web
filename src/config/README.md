# 配置与兼容

`AppConfig` 提供通用、日志、Majsoul/Tenhou、内置机器人、浏览器采集、自动操作及悬浮窗设置。
`capture.enabled` 显式值优先，否则读取旧 `proxy.enabled`；默认 true。模式固定 `chromium`。
旧 MITM 或不支持游戏停采并提示，不重置无关设置。Tenhou Chromium 配置恢复识别。

`merge.rs` 正常保存时删除已废弃的已知代理、云、机器人、镜像与云开发开关字段，保留未知字段和注释。
未知字段不会恢复已删除运行能力。旧外部机器人选择迁移到内置模型，提示并关闭自动操作。
`record_all` 仅扩大脱敏元数据范围；旧 `bodies=true` 不恢复正文。

附加模式目录为空可发现标准浏览器；独立模式仅用隔离目录。只允许回环，不能关闭沙箱/TLS/Origin 检查。
配置错误不输出原始内容；文件由 private_fs 保护。后续迁移回归需同时覆盖 TOML 和 IPC JSON。
