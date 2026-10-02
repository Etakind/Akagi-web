# 数据类型

`mjai/` 定义机器人与牌局之间的通用事件；`ipc.rs` 定义桌面命令/事件；`history.rs` 定义持久化历史；`inspector.rs` 定义脱敏诊断记录。

运行采集 kind 只有 chromium。配置游戏选择只有 Majsoul，但历史 Platform / MatchInfo 保留旧标签以读取既有记录；这不表示存在其他游戏运行实现。
Inspector 的 CaptureSource::Mitm 仅用于旧文件兼容；FrameRaw 保留旧 text/binary 读取能力，当前写入 redacted 及省略原因。
修改这些格式必须同步前端 `types.ts`、读取逻辑和序列化测试；不能只改一个语言的类型。
