# 本地网页维护版开发指南

[English](FORK_MAINTENANCE.md) | **简体中文**

本指南介绍维护版的设计、安全边界和贡献规则。安装与日常使用从 [README](../README.zh-CN.md) 开始。
上游对比基于 [Akagi v3 的 cd68865 提交](https://github.com/shinkuan/Akagi/tree/cd68865f9e93eddcda6451cd18874a6f68c5fb49)。

## 功能区别与原因

| 范围 | upstream → 本维护版 | 原因与实现入口 |
|---|---|---|
| 采集 | 多游戏/多后端 → Majsoul、Tenhou 官方网页与 Chromium CDP | 移除拦截证书和系统代理管理。入口 `src/capture/chromium/`、`src/bridge/{majsoul,tenhou}/`；保留严格 TLS、回环连接与官方页面匹配。 |
| 推理 | 本地、外部及远端模型 → 内置四麻/三麻模型 | 移除远端推理上传与任意机器人/依赖执行。入口 `src/bot/{native,manager,supervisor}.rs`；训练与转换工具仅供开发，不是应用运行时。 |
| 在线服务 | 云复盘/分享、密钥、订阅支付 → 删除 | 应用不包含账号、支付或上传服务。剩余 HTTP 客户端集中在 `src/network.rs`。 |
| 自动打牌 | 平台适配器 → 主动开启、唯一官方页面和有效决策窗口 | 防止操作其他页面或过期牌局状态。入口 `src/autoplay/`；失败暂停，绝不刷新游戏。 |
| 诊断 | 原始协议/配置输出 → 脱敏传输元数据 | 减少凭据落盘，保留 MJAI、本地分析、历史和 Inspector。入口 `src/privacy.rs`、`src/logger/`、`src/inspector/`、`src/util/private_fs.rs`。 |
| 主题与界面 | 远端主题/CSS 表达式 → 本地 JSON 与字面颜色 | 避免主题加载远端资源。入口 `frontend/src/stores/themeStore.ts`、`tauri.conf.json`；本地 Blob Worker 生成牌面，远端脚本及 Worker 仍被禁止。构建适配器 `frontend/scripts/mahgen-csp.mjs` 替换已知旧运行库初始化，依赖结构变化时停止构建并要求审查。 |
| 更新与下载 | 上游覆盖安装包及镜像 → 维护版发布信息与主动官方浏览器下载 | 防止上游安装器覆盖维护版改动。入口 `src/updater/check.rs`、`src/capture/chromium/cft.rs`；安装保持手动。 |
| 分发 | 上游打包方案 → 五种原生目标，不含 Python/uv 或 AppImage | 使安装依赖与本地推理设计一致。入口 `build/targets.json`、`scripts/package.py` 和构建工作流。 |

## 浏览器与自动操作边界

附加模式连接配置的回环端口。显式填写用户数据根目录时，以其定位文件为准，拒绝不安全或端口不匹配的文件。
未填写目录时，HTTP 发现失败后检查标准浏览器目录中的 `DevToolsActivePort`，只接受唯一且端口匹配的地址。
显式选择浏览器程序会将搜索限制在对应浏览器种类。发现过程不读取 Cookie/会话数据库、不递归扫描目录，也不修改现有浏览器目录权限。

每次重连重新发现地址。HTTP 发现失败与浏览器授权失败分别处理；403 停止重试，不能通过弱化 Origin 检查绕过。
独立模式先在空白页准备监听，再导航到官方客户端，使用隔离目录并保留浏览器沙箱和 TLS 校验。

自动输入要求所选游戏的官方 HTTPS 主机及路径、唯一页面和完整牌局状态。两个适配器均将 `location.href` 解析为 URL 后校验。
切换自动打牌开关取消排队和执行中的旧动作，但不重启采集或丢弃牌局；新动作必须来自开启后开始的推理。
页面或游戏变化也会取消待执行动作。暂停状态同步显示到界面。

天凤脚本适配仅在开启自动打牌时工作，只处理已识别的官方脚本 URL 和受支持的结构。
已经加载的客户端不会因开启开关而补入入口；缺少入口时请在安全时机自行重新进入。
适配缺失、页面歧义或状态不完整时停止输入，保留观察能力。Akagi 不会为修复自动打牌刷新游戏。
Lua 延时环境不提供文件、进程或网络访问。

## 数据、网络与配置

Akagi 不上传账号、牌局、历史、日志或推理数据，推理全部本地执行。
剩余请求仅用于维护版发布信息、用户主动下载官方 Chrome for Testing，以及回环 CDP 发现与控制。
游戏网页正常连接自身服务器。发布 API 不可访问时保留发布页入口，不索要浏览器 Cookie，也不回退安装上游包。

新 WebSocket 记录省略原始帧；HTTP 记录去除 URL 用户信息、查询和片段、敏感头及正文；Inspector 广播使用相同脱敏规则。
MJAI、分析和历史仍含对局与玩家信息，分享时应注意范围。新 Unix 私密文件使用 0600，目录使用 0700，并检查所有者和链接。
Windows 文件隐私依赖用户目录 ACL，Unix 权限位不能提供 Windows 访问控制。
便携安装使用可写程序目录，系统安装使用用户数据/配置目录；保留显式配置路径。

| 配置 | 行为 |
|---|---|
| `capture.enabled` | 显式新值优先，否则读取旧 `proxy.enabled`；默认开启。 |
| `capture.mode` | 仅 `chromium`；不支持的旧模式停采并提示。 |
| `platform.kind` | `Majsoul` 或 `Tenhou`；不支持的游戏不导致其他设置重置。 |
| `capture.chromium.attach_port` | 非零附加，零独立启动；两种模式下空目录的含义不同。 |
| `bot.active_4p/active_3p` | 仅内置模型；迁移外部模型选择时关闭自动打牌并提示。 |
| 废弃云端/代理/机器人字段 | 正常保存时删除已知废弃字段；未知用户字段保留，但不能开启已移除能力。 |
| `capture.http.bodies` / `record_all` | 原始正文不捕获；`record_all` 只扩大脱敏元数据范围。 |
| 历史 / Inspector | 保留旧标签及帧格式读取；新原始帧记录采用 `redacted`。 |

## 构建与发布

Rust Tauri 与前端 API 必须保持相同主版本/次版本，同步更新锁文件并执行 Tauri CLI 构建；直接 Cargo 构建不会检查这项匹配。

系统依赖和命令见 [README 源码构建](../README.zh-CN.md#源码构建)。
共同目标清单 `build/targets.json` 定义 Windows x86_64、macOS x86_64/ARM64、Linux x86_64/ARM64。
各目标提供便携 ZIP，Linux 额外提供 DEB/RPM。Linux 使用 Ubuntu 22.04、GTK3/WebKitGTK 4.1 构建基线。
DEB 安装说明面向 Ubuntu 22.04/24.04、Debian 12/13；RPM 面向 Fedora，Arch 从源码构建。
系统库必须满足安装包依赖，不能将这些包当作通用 Linux 二进制。

打包前验证目标合法性。包内包含模型、许可证、NOTICE 和使用文档，不含用户数据或 Python/uv。
每次构建生成资产清单和 SHA256；可选 minisign 签名要求预配置密钥与仓库公钥匹配，不自动创建或轮换身份。
SHA256 检查文件一致性，可信签名用于确认发布者身份。

CI 覆盖 `main` 推送及目标为 `main` 的 PR。管理员请求的 `/build-artifacts` 与手动发布复用原生构建流程，产物从工作流运行页面获取。
手动发布默认仅构建产物，正式发布要求显式指定该提交已有的标签。不设置标签自动发布、定时协议更新或自动合并。

应用与独立 `native_bot` 的 Cargo 配置在开发/测试模式下优化 `gemm-common` 和 `gemm-f16`，
规避其 [AArch64 调试编译问题](https://github.com/sarah-quinones/gemm/issues/31)，保留运行时 CPU 特性分派。
不要替换为全局 `+fp16` 或 `target-cpu=native`；依赖修复辅助函数的特性声明后，应重新评估这些包级优化设置。

## 剩余风险与改进方向

| 风险及触发条件 | 当前保护 | 改进方向 |
|---|---|---|
| CDP 可访问浏览器会话 | 回环、浏览器授权和官方页面绑定 | 优先隔离目录，不用时关闭调试，跟进浏览器授权变化。 |
| 自动打牌与游戏规则冲突，或客户端界面变化 | 主动开启、取消过期动作、失败暂停 | 使用前核对游戏规则，维护客户端样本和适配兼容性。 |
| 天凤脚本结构变化 | 仅改写已知脚本 | 为适配样本增加版本标记并扩展回归覆盖。 |
| 本地对局与玩家信息 | 传输日志脱敏、私密文件创建 | 改进 Windows ACL/reparse point 处理和导出隐私控制。 |
| 未签名/未公证程序 | 手动安装与来源/签名核验 | 使用持续维护的发布者身份增加平台签名。 |
| Linux GTK/glib 依赖 | Linux 包含 `glib 0.18.5`，受 [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) 影响 | 问题涉及 `VariantStrIter` 不安全迭代，需分析应用可达性；协调兼容升级或经过审查的回移修复，上游 0.20.0 已修复。 |
| 其他依赖及系统漏洞 | 锁定依赖，严格 TLS | 根据启用功能和可达输入分析，优先升级兼容修复并维护系统库。 |

## 贡献检查

提交改动前运行 `cargo test --locked --all-targets`、前端测试及生产构建。
`node frontend/scripts/test-tiles-browser.mjs` 使用临时 Chromium 目录验证生产 CSP 下的真实牌面生成，
可通过 `AKAGI_TEST_BROWSER` 指定本地浏览器程序。检查只打开本地样本页，不打开游戏或现有浏览器目录。

双语文档保持一致。合并上游时必须保留本地推理、脱敏、严格 TLS、安全文件写入、官方页面绑定及主动开启自动操作的规则。
不得恢复拦截 CA、系统代理、远端主题、云上传或外部机器人执行。
相关实现变化时，应检查附加与重连、配置迁移、回放/历史兼容、自动操作取消和打包目标验证。

## 仓库同步与推送防护

`origin` 指向公开仓库 [Etakind/Akagi-web](https://github.com/Etakind/Akagi-web)。
`main` 是维护版默认分支与发布主线；`upstream` 指向 shinkuan/Akagi，仅获取更新，通过 `upstream/v3` 跟踪上游。
不再维护本地上游镜像分支。保留上游历史及现有维护提交，不改写已发布历史；Akagi Web 从 0.1.0 开始独立编号。

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
python3 scripts/setup-fork.py
python3 scripts/setup-fork.py --check
```

脚本仅使用 Python 3 标准库和 Git，只设置当前仓库的 `remote.pushDefault=origin`、`push.default=simple`、
`pull.ff=only`、不可用的 upstream 推送地址及本地 pre-push 防护。
防护同时拦截上游名称与 SSH/HTTPS 地址，安装在 Git 管理目录内，并串联已有 hook，不覆盖共享 hook 配置。
这是防误操作措施，不是不可突破的权限隔离。Windows 如使用 `python` 启动 Python 3，请相应替换命令名。

工作区干净时，从 `main` 创建 `feature/*`、`fix/*` 或 `experiment/*`：

```sh
git switch main
git pull --ff-only origin main
git switch -c feature/example
# 提交审查后的改动，再推送：
git push -u origin feature/example
```

上游更新通过独立分支审查，不用上游覆盖 main：

```sh
git fetch --no-tags origin
git fetch --no-tags upstream
git switch main
git merge --ff-only origin/main
git switch -c maintenance/upstream-sync
git merge upstream/v3
# 解决冲突、核对维护版边界并完成完整测试矩阵。
git push -u origin maintenance/upstream-sync
```

创建目标为 `main` 的 PR。放弃冲突合并可用 `git merge --abort`，不要通过强推掩盖分叉。
只对尚未发布的提交 rebase；已合并且不再使用的功能分支可以删除。不配置定时自动合并。

发布前同步 Cargo 版本、前端回退版本和 `docs/releases/vX.Y.Z.md` 中的双语发布说明。
为审查后的提交创建标签，再对该标签运行手动 Release 工作流；五目标构建及资产清单、SHA256 校验通过后才能发布。
所有目标检查通过后，附件先上传到草稿；上传后的文件列表、大小和 GitHub SHA256 摘要全部匹配才公开。
上传中断或校验失败时保留草稿，不公开缺件版本。
保留的 `upstream.minisign.pub` 仅属于上游，不是 Akagi Web 签名身份。可选签名需要独立发布者的 `minisign.pub`
和匹配的已配置密钥；未签名发布应明确标注。
