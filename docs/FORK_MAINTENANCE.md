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

长期支持目标是 **macOS / Windows 双平台**。本次只建立仓库维护流程，不补齐平台安全实现或实机验证。
Linux 不列为本分支的支持承诺；保留上游 Linux CI 和代码不代表新增支持承诺。
当前主要实机证据来自 macOS 官方 Edge 与内置本地机器人，Chrome 独立目录模式继续保留。

## 必须保留的差异

以下规则已由用户批准。上游合并不能以“恢复上游行为”为由撤销它们。

| 差异与原因 | 保留规则 | 代码入口 | 验证状态与限制 |
| --- | --- | --- | --- |
| 日志可能包含登录凭据及会话数据 | WS 原帧、完整协议载荷和 HTTP 正文不落盘、不广播；URL 去用户信息、查询与片段；敏感头及配置 Debug 脱敏。仅改变记录副本，保留 MJAI、分析与历史。旧 `bodies=true` 不恢复正文；兼容旧 Inspector 记录。 | `src/privacy.rs`、`src/logger/`、`src/inspector/`、`src/schema/inspector.rs` | 有虚构凭据泄露回归及本机日志检查；不追溯保证旧日志、副本或其他软件。 |
| 私钥与账号属于本地敏感文件 | 凭据不提交、不通过参数或环境变量传递；本地读取须验证文件；Unix 私有文件 0600、目录 0700，拒绝不安全链接及所有者；不自动轮换 CA 或修改系统信任。 | `src/util/private_fs.rs`、`src/util/credentials.rs`、`src/proxy/ca.rs` | Unix 边界已有测试；Windows ACL 及等效链接/所有者保护尚待补齐和验证，不能将 Unix 模式位视为 Windows 已保护。 |
| MITM 上游验证不能被绕过 | HTTP/WSS 严格验证证书链、域名、有效期及握手签名，仅缓存验证成功的证书；不因验证失败放行。Chromium 模式不安装根证书或修改系统代理。 | `src/proxy/upstream.rs`、`src/proxy/certstore.rs` | 有有效证书、错误证书及签名拒绝回归；保留既有雀魂 IP:443 透传规则。 |
| 常用 Edge 接入和页面发现不稳定 | 仅回环 CDP；只采集官方雀魂页面；发现与订阅有超时、传输重连有预算；403 与授权超时不循环重试；中途接入缺状态时提示游戏自身重连，不自动刷新牌桌。 | `src/capture/chromium/{launch,connection,discovery,cdp,mod}.rs` | 有模拟 CDP 回归和本机订阅证据；未证明彻底解决浏览器所有授权拒绝条件。 |
| 附加模式原先没有自动操作页面 | 建议为基础；自动打牌须用户主动开启；只有唯一官方游戏页时绑定，每次操作检查页面，断开清理绑定。保留解析、建议及历史。 | `src/autoplay/`、`src/capture/chromium/cdp.rs`、`src/bridge/majsoul/` | 有解析、自动操作与页面边界测试；用户确认第一局建议可用。真实自动点击及整场历史落盘的专项实机验收仍待完成。 |
| 下载内容可能执行代码 | CfT 只用官方 HTTPS 清单及源，不使用未验证镜像、不自动移除隔离属性；机器人镜像仅有效签名或直接 GitHub 可信摘要匹配时允许安装，存在无效签名始终拒绝。 | `src/capture/chromium/cft.rs`、`src/bot/install.rs`、`src/github/` | 有摘要及签名拒绝测试；本机未安装外部机器人。主动安装的本地机器人以当前用户权限执行，Python venv 不是安全沙箱。 |
| 云上传和上游包覆盖会改变本构建边界 | 云功能默认关闭，实测配置不使用云推理及云复盘；保留版本信息与发布页入口，禁止应用内上游更新包覆盖个人构建。 | `src/config/bot.rs`、`src/ipc/commands.rs`、`frontend/src/components/UpdateDialog.tsx` | 代码检查及回归；“默认关闭”不等于移除所有用户主动启用的云功能。 |
| 依赖公告需结合实际调用路径 | 优先兼容更新，大版本单独评估；剩余公告记录受影响平台、启用条件及待办，不用扫描条数代替风险判断。 | `Cargo.toml`、两个 `Cargo.lock`、`frontend/package*.json`、`scripts/audit_dependencies.py` | 历史审计快照见 `dependency-audit.json`；具体适用条件见安全记录。更新依赖后应重新审计。 |
| 上游工作流依赖 v3，不能直接服务 dev | dev 推送及 PR 自动检查；协议随上游同步；个人仓库不定时生成协议更新、不按标签自动发布，保留手动构建。 | `.github/workflows/`、`scripts/setup-fork.py` | 本次验收结果见本文末尾；main 上工作流保持上游原样，个人开发及手动构建选择 dev。 |

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

本次不改变现有平台矩阵：Linux 跑 Rust 检查/测试，macOS/Windows 编译；前端另行测试和构建。
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

### 本次仓库整理验收（2026-10-02）

- 本机 Rust 主项目 997 项测试通过、7 项按既有条件忽略；原生机器人 49 项、前端 121 项通过。TypeScript、前端 lint / 生产构建、Rust 格式与严格 Clippy 检查通过。
- 推送防护 9 项离线测试通过，覆盖重复安装、原 hook 参数/输入/退出码、SSH/HTTPS 地址、共享目录及符号链接拒绝、错误 origin，以及本地 bare 仓库实际推送。upstream dry-run 被拒绝，未向上游执行真实推送。
- GitHub 工作流 YAML 及触发条件检查通过。CI 的矩阵维持原状；远端执行结果以 [dev 对应的 Actions 记录](https://github.com/Etakind/Akagi/actions?query=branch%3Adev) 为准，不能将本地结果当作远端或 Windows 实测。
- 待提交清单完成敏感路径与常见令牌格式检查；账号、会话、私钥、日志、本机配置及未知用途的 `command` 均未纳入。该检查不是穷尽所有秘密格式的保证。
- 整理时修复了一个依赖本地 `logs` 是否存在的旧测试，改用独立临时路径；修正严格 Clippy 指出的等价分支、默认值写法及代码排列，没有改变其业务语义。
- 首次远端 CI 在 Rust 1.99 的 Clippy 阶段发现 `async_trait` 0.1.89 生成重复 `must_use` 的告警；仅在 `src/bot/runner.rs` 的 `BotRunner` 与 `src/capture/mod.rs` 的 `CaptureBackend` trait 上允许该宏生成告警，保留其余 `-D warnings` 检查。后续升级宏库时重新评估此兼容标记。
- 正式版构建通过；全新本地克隆切换到 main 后防护仍生效，且不包含本机私密路径。GitHub 已确认仓库私有、默认分支为 dev；main 仍与上述上游基线一致。
- 本次没有重新操作真实游戏、启用自动打牌或执行 Windows 实机验收；远端 CI 的最终结果随交付说明报告。
