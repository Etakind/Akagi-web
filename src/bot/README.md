# 机器人与运行时

默认使用内置 `akagi-native` / `akagi-native3p`，无需下载模型或安装 Python 依赖。主动安装的外部机器人通过 MJAI JSONL 子进程协议运行，权限等同当前用户；Python venv 只隔离依赖，不是安全沙箱。

## 数据与生命周期

- `manager.rs` 在牌局追踪器应用事件后接收 post-tracker 总线，只在本座位可决策时请求建议，并向建议总线发布结果。
- `runner.rs` 定义异步 BotRunner 和外部 SubprocessBot，管理 stdin/stdout、超时、重置及进程退出；`native.rs` 调用内置模型。
- `types.rs` 定义机器人响应及元数据；`sync_guard.rs` 防止同一机器人并发创建依赖环境。
- 自动操作独立消费建议，用户未开启时只提供建议；模型建议不能绕过官方页面及决策窗口检查。

## 安装与配置

`registry.rs` 枚举本地机器人目录；`manifest.rs` 解析 manifest.toml 和 settings.toml；`install.rs` 安装用户主动选择的 GitHub 包。
镜像包必须通过有效签名，或与直接从 GitHub 获取的可信摘要匹配；存在无效签名始终拒绝。所有检查在解压及执行依赖安装之前完成。
手动本地机器人安装保留，用户需信任其代码。

`runtime.rs` 优先查找应用旁的 Python/uv，再尝试资源目录与系统 PATH。每个机器人在 `.akagi/venv` 建环境，利用清单/锁文件 stamp 避免重复同步。
目录搬迁后保留解释器链接修复与环境变量清理，这是 macOS/Windows 便携目录也需要的逻辑。用户主动重装环境可清理对应 venv 和 stamp。
正式运行时打包仅支持 macOS Apple Silicon 与 Windows x64，工具命令见 `scripts/README.md`。

## 可选云功能

`api.rs`、`purchase.rs` 管理用户主动启用的云推理及相关服务。默认关闭；该服务的出站代理设置仍可用，与已删除的游戏 MITM 代理没有关系。
敏感 API 密钥和代理认证 Debug 必须脱敏。开启云推理后会发送相应牌局事件；云复盘由用户主动提交。

验证包含内置模型回归、解析/分析流水线、子进程生命周期及下载信任边界。外部机器人集成测试缺少预备 Python 环境时会跳过，不能据此声称已验证实际外部模型。
