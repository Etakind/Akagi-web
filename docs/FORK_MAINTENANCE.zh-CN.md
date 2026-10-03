# 本地网页维护版开发指南

[English](FORK_MAINTENANCE.md) | **简体中文** · [使用说明](../README.zh-CN.md)

本文件记录用户批准的 `Etakind/Akagi` 支持范围与上游合并规则。本次分支为
`feature/local-web-platforms`，起点为个人 `dev` 提交
`68b47ada54982f46a1206887193fd8ada1f80df9`。核查的上游为 `shinkuan/Akagi` 的 `v3`，
基线 `cd68865f9e93eddcda6451cd18874a6f68c5fb49`；下述比较不代表未经检查的未来上游版本。

## 支持范围与差异

运行时只支持 **Majsoul、Tenhou 官方网页端**。保留桌面界面、本地三麻/四麻建议、雀魂悬浮窗、
历史、Inspector 和主动开启的自动打牌。上游合并不得恢复 MITM、证书管理、系统代理、原生游戏
拦截、云服务、外部可执行机器人或随包 Python/uv。旧历史与 Inspector 的平台标签只用于读取兼容。

| 领域 | 上游基线 → 维护版 | 原因及合并规则 | 实现入口与迁移 | 证据与待办 |
|---|---|---|---|---|
| 采集 | 多后端/多游戏 → Chromium CDP 与两个官方网页客户端 | 缩小高权限拦截与无用功能的攻击面；不安装 CA，不改系统代理，不关闭 TLS、沙箱或 Origin 检查。 | `src/capture/chromium/`、`src/config/platform.rs`、`src/bridge/{majsoul,tenhou}/`；模式固定 `chromium`。 | 从指定基线按需恢复 Tenhou；本次未跑回放或真实对局回归。 |
| 浏览器连接 | 依赖 HTTP 发现 → 未指定目录时先 HTTP，再查固定定位文件 | Edge 的界面调试服务可能对 `/json/version` 返回 404；不能为发现浏览器读取会话数据库。 | `launch.rs`、`detect.rs`、`connection.rs`、`discovery.rs`；规则见下。 | 本机生产构建通过；地址发现已进入授权握手，随后超时。本次未确认官方页面订阅。 |
| 自动操作 | 平台适配器 → 主动开启、唯一页面、绑定代数与决策窗口检查 | 旧动作不能作用到另一页面、游戏或决策；失败暂停，绝不自动刷新对局。 | `src/autoplay/`、`cdp.rs`；切换游戏/开关时使旧上下文失效。 | 本次仅源码检查；实际点击和客户端变更行为未验收。 |
| 推理 | 本地/外部/远端选项 → 仅内置 `native` / `native3p` | 移除远端牌局上传与任意 Python 机器人/依赖执行。 | `src/bot/{native,manager,supervisor}.rs`；旧外部模型迁移并关闭自动操作。 | 本机编译；未执行建议一致性回归。训练/转换工具仍只供开发使用。 |
| 复盘与账户 | 云复盘、分享、密钥、计费订阅 → 删除 | 消除应用上传入口和远端账户/额度状态。 | 删除 API 模块、IPC、页面、状态及翻译；`src/network.rs` 统一剩余 HTTP 客户端。 | 网络源码清单不等于抓包证明；不新增本地复盘或模型导入。 |
| 主题 | 远程主题/任意表达式 → 本地 JSON 与受限颜色字面量 | 防止主题 CSS 引用远程资源；生产 CSP 只允许本地资源与必要 Tauri IPC。 | `themeStore.ts`、`tauri.conf.json`、`main.tsx`；启动时不注入旧缓存任意 CSS。 | 前端生产编译；其他系统 WebView/CSP 未实测。 |
| 日志与文件 | 原始协议/配置可能落盘 → 元数据脱敏与私密文件 | 登录帧、HTTP 正文及秘密不写入日志/Inspector；实际解析数据不改。 | `src/privacy.rs`、`logger/`、`inspector/`、`schema/inspector.rs`、`util/private_fs.rs`；旧 `text/binary` 可读，新记录用 `redacted`。 | 保留回归源码但本次未执行；历史结果按版本区分。 |
| 更新/下载 | 上游覆盖安装、镜像、机器人安装器 → 个人版本信息、手动安装、主动下载官方浏览器 | 保持已审查构建，避免加入不可信可执行机器人；不得恢复上游覆盖安装。 | `updater/check.rs`、`capture/chromium/cft.rs`、`network.rs`；旧镜像设置不再使用。 | 静态检查；本次未发布或下载安装。 |
| 分发 | 上游打包方案 → 五目标，无 Python/uv/AppImage | 单一目标清单减少工作流与产物名分歧；安装后的数据须写入用户可写位置。 | `build/targets.json`、`build_targets.rs`、`platform.rs`、`util/mod.rs`、`scripts/package.py`、工作流。 | 仅构建本机 macOS x86_64；目标清单不代表编译或实机通过。 |

### 网络边界

Akagi 不向远端上传账号、牌局、历史、日志或推理数据；推理全部在本地完成。
保留更新检查和用户主动下载。这是实现的源码边界，完整流量审计仍是后续事项。
游戏网页仍正常连接自己的游戏服务器，可选自动操作也通过该客户端执行；浏览器不是离线系统。

| 剩余请求用途 | 目的地与约束 | 数据 |
|---|---|---|
| 版本信息 | 固定 `Etakind/Akagi` GitHub API，严格 HTTPS，无重定向回退 | 普通元数据 GET、User-Agent 中的应用版本；无牌局/账号载荷 |
| 用户主动获取浏览器 | 官方 Chrome for Testing 清单及精确官方 HTTPS 资产；拒绝重定向 | 版本/平台选择；无对局载荷 |
| 浏览器控制 | 仅回环 HTTP/CDP，发现请求不走代理、不接受重定向 | 本机浏览器内的游戏观察与可选操作 |

私有 GitHub API 匿名访问可能失败。提供个人发布页供用户在浏览器查看，不索取 Cookie、不借用
浏览器凭据、不回退上游安装包。采集/推理启动不会自动下载浏览器。保留的打开链接命令是用户操作，
不作为后台上传器。

### Edge 地址发现与连接

- 显式用户数据目录：检查其 `DevToolsActivePort` 的类型、所有者、链接及端口；不安全或端口不符即拒绝，
  不转连其他浏览器。文件不存在时可尝试同一配置端口的标准 HTTP 发现。
- 目录为空：先 HTTP；不可用时只查已知浏览器标准根目录。指定可执行文件时限定浏览器种类。
  仅读取定位文件，不递归、不读取 Cookie/会话、不修改常用目录权限。
- 只接受匹配端口的回环地址并去重；唯一候选才连接，歧义要求填写目录。每次传输重连重新发现。
- 区分端口不可达、发现失败、文件被拒绝、候选歧义、授权拒绝/超时、无官方页面、缺少初始牌局状态。
  不记录完整端点、定位文件内容及任意握手响应。
- 403 可能来自授权、Origin 或其他拒绝，不能一概归因于用户；不循环重试、不加 Origin 绕过参数。
- 停止附加采集不会关闭常用浏览器；独立模式使用私有目录，拒绝已知常用浏览器根目录。
  不重置会话文件，也不通过导航已有页面解决接入失败。

允许范围是精确 HTTPS 主机/路径 `game.maj-soul.com/1/` 和 `tenhou.net/4/`，不接受 URL 认证信息或
非默认端口。相似域名被拒绝。多个匹配页面时解除绑定，不随意选择。URL 匹配不等于已经恢复牌局，
解析就绪状态单独判断。

### Tenhou 适配与自动打牌

默认关闭自动打牌；关闭时仅观察 WebSocket，不改写脚本。开启后，只对所选 Tenhou 官方页面中已识别
的 `/4/` 客户端脚本准备响应拦截；要求成功状态、大小上限及预期代码结构，才暴露内部弃牌入口。
不放宽 TLS、不运行外部机器人代码。

独立模式先准备监听和适配，再导航新页面；附加已加载的客户端可能没有入口，提示暂停操作，由用户
选择安全时机重新进入。Akagi 不代为刷新。未知客户端、入口缺失、页面歧义、过期窗口或初始状态不足
均停止动作，保留可用的观察与建议。延时 Lua 继续禁止文件、进程和网络 API。Tenhou 三麻/四麻、真实
点击仍需后续专项回归与人工验收。

## 配置与本地数据

| 设置或旧数据 | 当前行为 |
|---|---|
| `capture.enabled` | 显式值优先，否则读取旧 `proxy.enabled`；默认 true。 |
| `capture.mode` | 只支持 `chromium`；已移除模式停采并说明。 |
| `platform.kind` | `Majsoul` / `Tenhou`；旧 Tenhou Chromium 配置可重新识别。其他旧游戏停采，不重置无关设置。 |
| `capture.chromium.attach_port` | 非零附加，零独立启动；只有附加模式的空 `user_data_dir` 表示自动查定位文件。 |
| `bot.active_4p/active_3p` | 固定内置模型；旧外部选择迁移、提示并关闭自动打牌；旧云开关无法恢复请求。 |
| 已废弃字段 | 正常保存时删除已知代理、云、外部机器人、镜像字段；保留无关设置和未知字段，未知字段不会成为执行入口。 |
| HTTP `bodies` / `record_all` | 不捕获正文；`record_all` 仅扩大脱敏 HTTP 元数据范围。 |
| 历史 / Inspector | 现有文件不改写；旧来源标签及原始帧格式仍可读，新记录脱敏。 |

只清理受版本控制的废弃代码/资源。`account`、本机配置、浏览器目录、CA 私钥、日志、历史、旧机器人、
Python 环境不自动清理；即使不再使用，它们仍可能敏感。删除代码不等于撤销系统证书信任。清理文件与
系统信任须由用户分别处理，私有仓库也不能用于备份这些信息。

Unix 私密文件 0600、目录 0700，安全写入并检查链接。Windows ACL/reparse point 仍需专项实机审查，
不能用 Unix 权限声明 Windows 保密性。可写便携目录通常在程序旁保存数据；不可写系统安装目录改用
用户配置/数据位置。既有配置查找优先级和显式绝对路径继续有效，不往 `/usr` 写运行数据，不静默移动历史。

## 构建、打包与发布

`build/targets.json` 是 Rust 浏览器映射、CI、打包的共同目标清单：

| Rust 目标 | 原生构建环境/基线 | 产物 |
|---|---|---|
| `x86_64-pc-windows-msvc` | Windows | ZIP |
| `x86_64-apple-darwin` | Intel macOS | ZIP |
| `aarch64-apple-darwin` | Apple Silicon macOS | ZIP |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | ZIP / DEB / RPM |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 ARM | ZIP / DEB / RPM |

依赖及命令见 [README](../README.zh-CN.md)。Linux 需要 GTK3/WebKitGTK 4.1 及 `tauri.conf.json` 声明的
依赖。DEB 面向 Ubuntu 22.04/24.04、Debian 12/13；RPM 说明面向 Fedora；Arch 从源码构建。这些是预期
兼容范围，均未完成本次发行版验收。Ubuntu 22.04 是选定的较旧
[Tauri 构建基线](https://v2.tauri.app/distribute/appimage/)，不恢复 AppImage。

打包在创建、复制、下载、删除前验证目标。产物含内嵌模型、许可证、NOTICE 和说明，不含 Python/uv
或用户数据；Linux 原生包来自同目标 Tauri 构建。生成资产清单与 SHA256，摘要验证一致性而非发布者身份。
可选 minisign 先用探针确认既有私钥与仓库公钥匹配，才签名资产；无密钥则保持未签名，不匹配即停止。
不自动创建/轮换身份，不宣称完成 Apple 公证或 Windows 签名。

CI 面向 `dev` 推送和目标为 `dev` 的 PR。经授权的 `/build-artifacts` 与手动发布复用五目标构建。
不恢复标签自动发布、定时协议更新或自动合并。手动发布默认仅产出资产；真正发布要求用户指定指向
本次构建提交的既有标签。本次只改配置，没有触发工作流、跨平台包或正式 Release。

## 剩余风险与改进方向

| 风险与触发条件 | 当前边界 | 后续改进 |
|---|---|---|
| CDP 可访问浏览器会话 | 仅回环、浏览器授权、官方页面筛选；不读取会话数据库 | 优先隔离目录，不用时关闭调试；复核浏览器授权与定位文件机制变化。 |
| 自动操作与游戏规则 | 用户主动开启、页面/窗口检查、失败停止 | 用户需判断账号/游戏规则后果；每个客户端版本应先人工验证输入。 |
| Tenhou 脚本变化 | 只适配已知结构，入口缺失暂停 | 增加回放/变更脚本样本、适配版本标记及真实点击验收。 |
| 本地敏感数据 | 传输日志脱敏、新文件私有；MJAI/历史仍含玩家和牌局信息 | 审查旧日志/备份与 Windows 权限，不随意分享历史或旧原始 Inspector 导出。 |
| 未签名桌面程序 | 人工核对来源、摘要与可用签名，不自动改系统信任 | 若需要，单独配置个人签名/公证身份。 |
| Linux glib 公告 | 恢复 Linux 后 GTK/glib 再次进入运行依赖 | `glib 0.18.5` 命中 [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)，涉及 `VariantStrIter` 的不安全迭代；非法/恶意 variant 到达该迭代器是相关条件，本次未确认应用可达性。不能再沿用“Linux 未启用”的排除理由。需协调 GTK/Tauri 兼容升级或经审查回移修复，上游 glib 从 0.20.0 修复。 |
| 其他依赖与系统库问题 | 保留严格 TLS，不强制批量大版本升级 | 对变更锁文件和启用功能重新审计，维护系统库，结合解析器/输入条件判断，不只数告警。 |

`dependency-audit.json` 是历史快照，并非本次扫描。旧 quick-xml 结论涉及 plist 的属性/命名空间处理，
旧 rand 结论依赖日志重入条件，npm mahgen 传递依赖涉及 Node 图片/文件/下载路径。
这些只能作为新审计线索，不是本版本的无风险证明。停止维护的依赖仍是维护债务。
本次未建立恶意外传证据；删除上传入口能减少暴露，但不意味着所有漏洞均已消除。

## 仓库同步与推送防护

| 引用 | 用途 |
|---|---|
| `origin` | 私有 `git@github.com:Etakind/Akagi.git`；个人代码、多设备同步 |
| `upstream` | `https://github.com/shinkuan/Akagi`；仅获取，真实主线是 `v3` |
| `main` | 上游精确镜像，跟踪 `origin/main`；只快进，不放个人提交 |
| `dev` | 个人默认/集成分支，跟踪 `origin/dev` |
| `feature/*`、`fix/*`、`experiment/*` | 从 `dev` 创建，审查后集成 |

新设备克隆后执行：

```sh
git clone --branch dev git@github.com:Etakind/Akagi.git
cd Akagi
python3 scripts/setup-fork.py
python3 scripts/setup-fork.py --check
```

Windows 如用 `python` 启动 Python 3，则替换命令名。脚本仅用标准库/Git，设置仓库本地
`remote.pushDefault=origin`、`push.default=simple`、`pull.ff=only`、不可用 upstream push URL
及本地 `pre-push`。防护同时拦截 upstream 名称和原仓库 SSH/HTTPS 地址，安装于本地 Git 管理目录，
切换 main 后仍有效；保留并串联已有 hook，不覆盖外部共享 hook。新克隆、移动目录或更换 Python 后重装。
这是可主动绕过的防误操作措施，不是远端权限隔离。

同步前工作区必须干净，不能丢弃用户修改。日常开发：

```sh
git switch dev
git pull --ff-only origin dev
git switch -c feature/example
# 只暂存审查过的文件，提交后：
git push -u origin feature/example
```

上游同步逐条执行，失败立即停止：

```sh
git fetch --no-tags origin
git fetch --no-tags upstream
git switch main
git merge --ff-only origin/main
git merge-base --is-ancestor main upstream/v3
git merge --ff-only upstream/v3
git rev-parse main upstream/v3
# 两个 SHA 必须相同，然后才推送：
git push origin main:main
git switch dev
git merge --ff-only origin/dev
git merge main
# 核对个人差异、解决冲突并完成约定验证后：
git push origin dev:dev
```

main 分叉立即停下，不用强推/reset 掩盖。dev 冲突按本文规则解决；放弃合并可用 `git merge --abort`
回到原本干净的状态。只有未发布的个人提交可显式 rebase，不改写已发布历史。
通常合并前检查 CI；但**本次**用户明确要求不跑 CI、不建 PR、不合并，因此只推个人功能分支，保留 dev/main。

## 本次证据与后续验证

本次证据包括源码/静态审查、生产前端和本机 release 构建。
[浏览器验收记录](validation/2026-10-03-browser-attachment.md)区分地址发现成功、授权超时与未确认的页面接入。
本版本未运行自动测试、Clippy 回归、CI、流量审计、跨平台构建、真实对局、自动点击或其他设备验收。

后续回归清单（不能标记为本次通过）：

- 定位文件、重连、不安全文件、歧义、403/超时、唯一页面绑定。
- Tenhou/Majsoul 回放、本地建议一致性、中途接入、历史/PT 结果。
- 客户端脚本变化、关闭自动操作、过期窗口、动作期间切换游戏/开关。
- 旧云/外部机器人迁移、未知字段保留、零上传调用清单。
- 虚构凭据覆盖所有日志/Inspector 的泄露测试、旧记录兼容。
- Linux glib 可达性、重新 Cargo/npm 审计、各原生 WebView 的 CSP。
- Rust/前端/原生机器人回归，五目标原生打包与实机验收。

旧测试属于各自旧提交，保留于 [SECURITY_HARDENING.md](../SECURITY_HARDENING.md) 和 Git 历史，
不能当作当前结果。代码或上游合并须同步更新双语文档、入口与证据；支持范围、安全规则、自动操作、
发布政策变更仍须用户逐项确认，常规版本/验证证据随提交更新。

[本次实现记录](validation/2026-10-03-local-web-platforms.md)列出构建、静态检查及未完成验收。
