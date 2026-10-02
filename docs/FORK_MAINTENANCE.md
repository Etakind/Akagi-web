# 个人维护版开发与同步指导

本文件记录 Etakind/Akagi 与 shinkuan/Akagi 的差异及用户逐项确认的维护规则。
它是后续开发、上游合并和多设备同步的依据；不使用 AGENTS.md 承载这些规则。
详细测试证据、历史排查及依赖公告见 [SECURITY_HARDENING.md](../SECURITY_HARDENING.md)。

## 仓库与支持目标

| 名称 | 用途 | 允许的修改 |
| --- | --- | --- |
| `upstream` | `https://github.com/shinkuan/Akagi`，原开源仓库 | 只获取更新，禁止推送。上游主线实际名为 `v3`。 |
| `origin` | `git@github.com:Etakind/Akagi.git`，个人私有仓库 | 个人分支与设备间同步的唯一常规推送目标。 |
| `main` | 镜像 `upstream/v3`，跟踪 `origin/main` | 只快进，不加入个人代码、文档或工作流。 |
| `dev` | 个人集成分支，跟踪 `origin/dev`；个人仓库默认分支 | 本维护版代码、文档及工作流。 |
| `feature/*`、`fix/*`、`experiment/*` | 从 `dev` 创建的短期分支 | 完成验证后合并回 `dev`。 |

初始上游基线为 `cd68865f9e93eddcda6451cd18874a6f68c5fb49`。原本名为 `upsteam` 的远端更名为 `upstream`。

维护范围现为 **macOS / Windows 上的雀魂网页端**；正式构建目标为 macOS Apple Silicon 与 Windows x64。
保留 Edge 附加模式和独立 Chromium 模式，不安装证书或修改系统代理。Linux 仅可运行通用 CI，不维护应用运行、下载或打包路径。
本地 Intel macOS 开发回归不代表新增 Intel 安装包承诺。Windows 编译及自动测试也不能替代实机浏览器和文件权限验收。

## 必须保留的差异

以下规则已由用户批准。上游合并不能以“恢复上游行为”为由撤销它们。

| 差异与原因 | 保留规则 | 代码入口 | 验证状态与限制 |
| --- | --- | --- | --- |
| 日志可能包含登录凭据及会话数据 | WS 原帧、完整协议载荷和 HTTP 正文不落盘、不广播；URL 去用户信息、查询与片段；敏感头及配置 Debug 脱敏。仅改变记录副本，保留 MJAI、分析与历史。旧 `bodies=true` 不恢复正文；兼容旧 Inspector 记录。 | `src/privacy.rs`、`src/logger/`、`src/inspector/`、`src/schema/inspector.rs` | 有虚构凭据泄露回归及本机日志检查；不追溯保证旧日志、副本或其他软件。 |
| 凭据与本地敏感数据保护 | 不提交账号、会话、配置、日志；Unix 私有文件 0600、目录 0700，拒绝不安全链接及所有者。移除 MITM 后不再创建或使用 CA，已有文件和系统信任由用户处理。 | `src/util/private_fs.rs`、`src/util/credentials.rs` | Unix 有自动测试；Windows 等效 ACL/链接保护仍待专项补齐，不能声称已验证。 |
| 只维护雀魂网页接入，减少代理攻击面与维护成本 | 删除 MITM/CA/证书改写、天凤与一番街运行代码；HTTP/CDP 元数据仍脱敏，剩余 HTTPS 请求严格验证 TLS。上游合并不得恢复已删后端。 | `src/capture/chromium/`、`src/bridge/majsoul/`、`src/config/` | 旧 MITM TLS 测试属于历史证据；当前版本不存在该链路。浏览器、日志及配置回归结果见下文。 |
| 常用 Edge 接入和页面发现不稳定 | 仅回环 CDP；只采集官方雀魂页面；发现与订阅有超时、传输重连有预算；403 与授权超时不循环重试；中途接入缺状态时提示游戏自身重连，不自动刷新牌桌。 | `src/capture/chromium/{launch,connection,discovery,cdp,mod}.rs` | 有模拟 CDP 回归和本机订阅证据；未证明彻底解决浏览器所有授权拒绝条件。 |
| 附加模式原先没有自动操作页面 | 建议为基础；自动打牌须用户主动开启；只有唯一官方游戏页时绑定，每次操作检查页面，断开清理绑定。保留解析、建议及历史。 | `src/autoplay/`、`src/capture/chromium/cdp.rs`、`src/bridge/majsoul/` | 有解析、自动操作与页面边界测试；用户确认第一局建议可用。真实自动点击及整场历史落盘的专项实机验收仍待完成。 |
| 下载内容可能执行代码 | CfT 只用官方 HTTPS 清单及源，不使用未验证镜像、不自动移除隔离属性；机器人镜像仅有效签名或直接 GitHub 可信摘要匹配时允许安装，存在无效签名始终拒绝。 | `src/capture/chromium/cft.rs`、`src/bot/install.rs`、`src/github/` | 有摘要及签名拒绝测试；本机未安装外部机器人。主动安装的本地机器人以当前用户权限执行，Python venv 不是安全沙箱。 |
| 云上传和上游包覆盖会改变本构建边界 | 云功能默认关闭，实测配置不使用云推理及云复盘；保留版本信息与发布页入口，禁止应用内上游更新包覆盖个人构建。 | `src/config/bot.rs`、`src/ipc/commands.rs`、`frontend/src/components/UpdateDialog.tsx` | 代码检查及回归；“默认关闭”不等于移除所有用户主动启用的云功能。 |
| 依赖公告需结合实际调用路径 | 优先兼容更新，大版本单独评估；剩余公告记录受影响平台、启用条件及待办，不用扫描条数代替风险判断。 | `Cargo.toml`、两个 `Cargo.lock`、`frontend/package*.json`、`scripts/audit_dependencies.py` | 历史审计快照见 `dependency-audit.json`；具体适用条件见安全记录。更新依赖后应重新审计。 |
| 构建范围与上游不同 | dev/目标为 dev 的 PR 检查；Rust 回归仅 macOS/Windows，Linux 只跑通用检查。PR/手动构建仅两种正式目标；无定时协议更新及标签自动发布。 | `.github/workflows/`、`scripts/test_web_only.py` | 不自动发布安装包；验证状态见下文。 |

## 新设备与推送防护

需要 Git 与 Python 3；Windows 使用 Git for Windows（hook 由其 sh 执行），Python 须可运行。
首次默认分支切换完成后，新的克隆会检出 dev；显式命令如下：

```sh
git clone --branch dev git@github.com:Etakind/Akagi.git
cd Akagi
python3 scripts/setup-fork.py
python3 scripts/setup-fork.py --check
```

Windows 的 Python 命令如为 `python`，将上述 `python3` 替换为 `python`。
脚本检查 origin 身份，新增 upstream 或将 upsteam 更名，设置仅本仓库生效的默认推送及拉取策略。
它不读取账号、浏览器会话或私钥，不修改全局 Git 配置，不自动获取、提交或推送代码。

防护包含不可用的 `remote.upstream.pushurl` 和本地 `pre-push` 检查器：后者同时拦截 upstream 名称及原仓库 SSH/HTTPS 地址。
检查器保存在本地 Git 管理目录，切换到 main 仍有效。已有 pre-push 保留为 `pre-push.akagi-original`，允许的推送继续调用它，参数、标准输入及退出状态得到保留。
若配置了外部共享 `core.hooksPath`，脚本拒绝覆盖并报告；先自行安排仓库本地 hooks 后再运行。
新克隆、移动仓库或更换 Python 安装后重新运行配置脚本。Git 配置和 hook 不随普通克隆复制。
这些是防误操作措施，主动修改配置或绕过 hook 可以绕开，不能替代远端权限控制。

## 日常开发与上游同步

先确认 `git status --short` 为空；把正在进行的修改提交到合适的个人分支，不能用 reset/clean 丢弃它们。
日常开发示例：

```sh
git switch dev
git pull --ff-only origin dev
git switch -c feature/example
# 开发、测试，明确选择文件 git add 并提交
git push -u origin feature/example
```

在 origin 发起目标为 dev 的 PR，检查 CI 和差异表后合并。普通 dev 拉取只允许快进；
发生分叉时先检查两侧历史，再显式合并。只可对未发布的个人提交显式 rebase，不改写已发布历史，不强推。

同步上游时逐步执行并检查每一步成功；发生失败立即停止，不把整段当作忽略错误的批处理：

```sh
git fetch --no-tags origin
git fetch --no-tags upstream
git switch main
git merge --ff-only origin/main
git merge-base --is-ancestor main upstream/v3
git merge --ff-only upstream/v3
git rev-parse main upstream/v3
# 上一行必须得到两个相同 SHA；否则停止，不推送
git push origin main:main
git switch dev
git merge --ff-only origin/dev
git merge main
# 检查冲突、安全底线、差异表及测试，通过后才执行：
git push origin dev:dev
```

main 无法快进或 SHA 不一致时停下分析，不 reset 或强推。dev 合并发生冲突时，逐项核对上表，
解决后明确暂存文件并完成 merge；若需要放弃本次合并，工作区原本干净的前提下使用 `git merge --abort`。
修复应在 dev/个人分支完成，不能在 main 留下个人提交。没有定时自动合并任务，也不批量推送标签。

## 验证与文档维护

当前 CI 在 macOS/Windows 运行 Rust 检查与测试；Linux 运行前端、推送防护及裁剪边界检查。
CI 编译通过不能替代本机权限保护、浏览器授权、实际自动点击等实机验证。
开发时按相关变更运行，并在首次整理提交时完整核对：

```sh
python3 scripts/test_setup_fork.py
python3 scripts/setup-fork.py --check
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --manifest-path native_bot/Cargo.toml
cd frontend
npm ci
npm test
npm run lint
npm run build
cd ..
cargo build --locked --release --features custom-protocol
```

凭据、私钥、Cookie/站点会话、日志及本机配置不提交。仅提交审查过的源文件、测试、工具与文档；
临时文件和用途不明文件留在本地。私有仓库也不能当作敏感数据备份。
旧日志不自动删除或重写，其处理范围见安全记录。提交前的检查不得把真实凭据输出到终端、模型或 CI。

每次修改上述行为或合并上游，都同步更新差异表的代码入口及验证状态，并记录新的上游基线。
普通版本和测试证据随提交维护；改变支持目标、安全底线、自动操作边界、同步或发布规则时，仍须用户逐项确认。
不以旧测试结果冒充新版本验证；未运行、失败或受环境阻塞的检查须明确记录。

### 历史：首次仓库整理验收（2026-10-02，裁剪前）

- 本机 Rust 主项目 997 项测试通过、7 项按既有条件忽略；原生机器人 49 项、前端 121 项通过。TypeScript、前端 lint / 生产构建、Rust 格式与严格 Clippy 检查通过。
- 推送防护 9 项离线测试通过，覆盖重复安装、原 hook 参数/输入/退出码、SSH/HTTPS 地址、共享目录及符号链接拒绝、错误 origin，以及本地 bare 仓库实际推送。upstream dry-run 被拒绝，未向上游执行真实推送。
- GitHub 工作流 YAML 及触发条件检查通过。CI 的矩阵维持原状；远端执行结果以 [dev 对应的 Actions 记录](https://github.com/Etakind/Akagi/actions?query=branch%3Adev) 为准，不能将本地结果当作远端或 Windows 实测。
- 待提交清单完成敏感路径与常见令牌格式检查；账号、会话、私钥、日志、本机配置及未知用途的 `command` 均未纳入。该检查不是穷尽所有秘密格式的保证。
- 整理时修复了一个依赖本地 `logs` 是否存在的旧测试，改用独立临时路径；修正严格 Clippy 指出的等价分支、默认值写法及代码排列，没有改变其业务语义。
- 首次远端 CI 在 Rust 1.99 的 Clippy 阶段发现 `async_trait` 0.1.89 生成重复 `must_use` 的告警；仅在 `src/bot/runner.rs` 的 `BotRunner` 与 `src/capture/mod.rs` 的 `CaptureBackend` trait 上允许该宏生成告警，保留其余 `-D warnings` 检查。后续升级宏库时重新评估此兼容标记。
- 正式版构建通过；全新本地克隆切换到 main 后防护仍生效，且不包含本机私密路径。GitHub 已确认仓库私有、默认分支为 dev；main 仍与上述上游基线一致。
- 本次没有重新操作真实游戏、启用自动打牌或执行 Windows 实机验收；远端 CI 的最终结果随交付说明报告。

## 网页维护版裁剪规则（2026-10-02）

| 变更与原因 | 保留规则与入口 | 验证与限制 |
| --- | --- | --- |
| 只使用雀魂网页，不再维护其他协议或客户端 | 删除代理、天凤、一番街解析和自动操作、注入与专属 PT 算法。`src/bridge/majsoul/` 和通用 MJAI/分析/历史继续维护。 | 旧历史标签、房间显示和 Inspector 来源仅为读取兼容保留，不作为运行入口。 |
| 一个采集后端不需要多游戏/代理向导 | `capture.mode` 固定 `chromium`、`platform.kind` 固定 `Majsoul`；设置及向导提供端口 0/非零两种浏览器连接方式。 | 独立和附加模式都只采集官方雀魂页面，自动操作仅绑定唯一页面；不自动刷新对局。 |
| 原启动开关属于已移除的代理配置 | `capture.enabled` 默认 true；缺失时读取旧 `proxy.enabled`，显式新值优先。`src/config/mod.rs` 读取并标记不可用模式，保存由 `merge.rs` 清理已废弃已知字段。 | 旧其他游戏/MITM 停止采集并提示；不重置机器人配置，不自动改写原文件，未知字段继续保留。 |
| 无用的平台代码与资源增加维护负担 | 删除 Linux 浏览器/启动/AppImage 路径、移动图标及 Linux 打包目标。macOS 所需的 Unix 安全文件、进程和 Python 目录搬迁修复保留。 | 打包/运行时下载脚本在任何文件副作用前拒绝不支持目标；不承诺 Intel/ARM Windows 包。 |
| 文档及界面宣传与维护范围不一致 | 根 `README.md` 是唯一中文使用入口；删除重复语言 README、无用产品宣传、旧游戏公告和代理设置翻译。界面多语言保留。 | 必要模块说明保持中文；安全记录保留历史事实并明确版本，许可证和归属不删除。 |

接口变化：运行 IPC 的 CaptureKind/CaptureMode 只输出 Chromium；配置不再包含 proxy，新增 capture.enabled 与只读 unavailable_reason 提示。
`FrameRaw` 的 redacted/text/binary 兼容不变；历史平台/MatchInfo 与旧 Inspector 的 MITM 来源保留为只读兼容数据。
旧配置的代理未知用户字段保留，已废弃的已知代理字段在正常保存时删除；不主动扫描或改写其他配置文件。

清理只针对 Git 跟踪内容。`account`、本机配置、`chrome-profile/`、`edge-game-profile/`、旧 CA、日志和历史目录保持原状。
旧 CA 的文件及已安装信任分别处理：删除代码不会撤销系统信任，也不自动轮换或移除证书。具体遗留项见安全记录。

上游合并时检查上述删除是否被重新引入；只有仍被保留功能引用的代码可以恢复。恢复支持范围或改变安全规则须逐项确认。
旧实现直接查 Git 历史，不放归档源码副本。协议定义继续随上游同步，人工开发工具仍可使用。

### 本次裁剪验证

- 本机 `cargo test --locked --all-targets`：759 项通过、7 项忽略；配置迁移专项 8 项通过。覆盖脱敏、雀魂回放、建议、历史、旧 Inspector 格式、浏览器连接与自动操作边界。
- 原生机器人 49 项通过；前端 16 个文件、113 项测试通过，lint、TypeScript 与生产构建通过。Rust fmt、严格 Clippy 和 `custom-protocol` release 构建通过。
- 仓库防误推 9 项、网页裁剪/双目标脚本 3 项测试通过；工作流 YAML 解析及内部 Markdown 文件链接检查通过。提交内容检查不包含账号、会话、私钥、运行日志或本机配置。
- 本机为 macOS x86_64 开发环境：release 命令行和禁用采集/机器人/自动操作的临时目录进程启动检查通过；正式产物目标仍为 Apple Silicon 与 Windows x64，本次不生成或发布安装包。
- 临时独立 Edge 使用实际 Chromium 后端启动并订阅官方雀魂页面成功，随后关闭并清理临时目录。常用 Edge 的 9222 接口本次不可用，附加实机验收未完成；相关连接、授权失败、发现/重连和唯一页面边界由自动测试覆盖。
- 本次未登录真实账号、开局或执行自动点击；Windows 实机、真实对局和自动点击保持未验收。此前真实使用记录仅见历史安全记录。
- 远端双平台 CI 以本功能分支 PR 的当前提交为准；通过后才合入 dev，结果链接随交付记录。

### 本次依赖审计适用条件

重新查询 OSV 的 676 个 Cargo 包版本；明细保存在 `dependency-audit.json`。删除仅代理使用的 hudsucker、x509-parser 等直接依赖后，保留 rustls/h2 的安全版本约束，不做强制大版本升级。

- `glib 0.18.5` 的两条 ID 是同一迭代器安全问题；来自 Tauri 的 Linux GTK 依赖，在两个正式目标的功能树中不启用。Cargo.lock 保留平台依赖不等于产品支持 Linux。
- `quick-xml 0.38.4` 的 RUSTSEC-2026-0194/0195 分别要求检查大量 XML 属性或使用 NsReader。当前来自 `plist 1.8.0`；已检查它使用普通 Reader，没有调用属性迭代或 NsReader，雀魂帧也不走 XML。修复要求 quick-xml >=0.41，超出当前传递约束，后续随 plist/Tauri 兼容升级评估，不直接替换锁文件版本绕过约束。
- `rand 0.7.3` 的两个 ID 描述同一自定义日志器重入线程 RNG 问题。当前为 HTML/CSS 构建工具链间接依赖，未启用其 log 功能，本项目日志器也不调用该 RNG。
- `fxhash`、`paste`、`proc-macro-error` 为停止维护公告，分别存在于上游解析/构建/数值依赖链；记录维护风险，不等同于已发现的数据外传。
- npm 剩余 `@jimp/core`、`file-type`、`phin` 三项中等风险条目来自 mahgen 的 Node 图像处理依赖。恶意 ASF 文件可能卡住旧解析器，phin 重定向可能携带敏感头；应用前端仅使用麻将牌渲染模块，未导入或调用这些下载/文件识别路径。后续升级 mahgen 时复核，不强制改变牌面渲染依赖的大版本。

审计快照不证明没有其他漏洞；后续输入路径、启用功能或依赖变更都必须重查上述适用条件。
