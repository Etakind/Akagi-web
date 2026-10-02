# 当前版本说明：仅雀魂网页端

2026-10-02 的网页维护版已删除 MITM、CA 管理、天凤、一番街和 Linux 应用路径。
当前使用方式见 [README](README.md)，裁剪原因和当前验证结果见 [维护指导](docs/FORK_MAINTENANCE.md)。
新配置使用 `capture.enabled`；旧 `proxy.enabled` 仅作读取兼容。TLS 验证继续用于剩余 HTTPS 网络请求，旧 MITM 证书验证链路已不存在。

**下文是裁剪前的安全加固与真实使用记录**，保留排查证据，不表示其中的 MITM、Linux 或旧开关仍可用。
其中测试数量与账号/对局结果仅对应当时版本；本次未重新进行真实账号登录或自动打牌验收。
旧日志、证书私钥、浏览器会话及系统信任不自动清理。Windows 文件 ACL 和实际浏览器行为仍需专项验收。

---

# 本地安全构建说明

长期开发规则及上游同步流程见 [个人仓库维护指导](docs/FORK_MAINTENANCE.md)。下文保留各次验证的时间和范围，不代表所有平台已实测。

本构建保留牌局解析、内置本地机器人、分析建议与历史记录。本次按用户要求使用已安装的官方 Edge、独立配置目录和复用的雀魂站点会话完成第一局人机实测；不会安装根证书或修改系统代理。原先的 Chrome 配置也保留可用。MITM 模式仍可选，必须严格验证上游证书链、域名、有效期和握手签名，不提供失败放行。

## 使用

本机实测配置：`configs/edge-game.toml`（Git 忽略、0600）。启用 Edge / Chromium 和内置 `akagi-native` / `akagi-native3p`，关闭 MITM、云推理、自动操作和外部机器人的自动依赖安装。云复盘只有主动提交时才运行，本次验收不调用。

```sh
cargo build --locked --release --features custom-protocol
./target/release/akagi --config configs/edge-game.toml
```

当前 Edge 实测配置会自动启动采集。`proxy.enabled` 是旧版“自动启动采集”开关；实际后端由 `capture.mode="chromium"` 选择，启动后不会开启 MITM。备用 Chrome 配置 `configs/local-secure.toml` 保持 `proxy.enabled=false`，使用它时需要在界面手动启动采集。

当前 Edge 使用项目下独立的 `edge-game-profile/`；备用 Chrome 使用 `chrome-profile/`，目录为 0700。禁止将调试接口改为非回环监听，禁止关闭证书检查或沙箱。项目不会自动删除该正常使用配置目录；其中包含登录会话，请按个人数据管理。

## 记录与私钥

- `account` 被 Git 忽略且权限为 0600；不复制、不提交。
- 完整配置不再打印；API 密钥、代理认证和下载镜像配置的 Debug 输出脱敏。
- WS 原始帧和完整协议载荷不再写入终端、文本/JSON、流日志、binlog 或 Inspector。新 Inspector 使用 `redacted` 类型及原因，旧版 `text` / `binary` 仍可读。
- HTTP 去掉 URL 的用户信息、查询和片段；只保留经约束的 Content-Type / Content-Length，不保存正文、Cookie、Authorization。遥测仅保留类别/处理标记。`record_all` 只扩大脱敏元数据范围；旧 `bodies=true` 被忽略。
- MJAI 事件、分析建议和历史数据保持原样。这些仍可能包含玩家昵称、对局标识等个人数据，分享日志时应选择范围。
- 私钥两种格式、配置、新日志为 0600，私有目录为 0700；拒绝写入符号链接、硬链接或其他用户拥有的私密文件。已有 CA 私钥在加载时收紧权限，内容和系统信任不变。
- 未删除或重写旧日志。开始加固前检查工作区时没有发现已有的 `logs/`、`ca/` 或用户 Akagi 数据目录中的历史文件。若使用过别的版本/自定义目录，历史 `all.log`、`all.jsonl`、`inspector.jsonl`、`*.binlog`、每连接 `*.log` 仍可能含账号、口令或令牌，需要自行处理；旧浏览器目录和导出的日志副本也不受此次修改追溯保护。

## 下载与更新

Chrome for Testing 只接受 Google 官方 HTTPS 清单中的官方 GCS 地址，拒绝跳转和镜像回退，不自动移除 macOS 隔离属性。本机使用已安装的 Chrome，无需下载。

机器人经镜像下载时，必须有有效 minisign 签名，或与直接从 GitHub HTTPS 元数据取得的 SHA-256 相符。镜像提供的摘要不可信。存在签名但验证失败时，即使摘要相符也拒绝。验证发生在解压和运行依赖安装之前。用户主动选择的本地机器人继续可用，机器人及其依赖以当前用户权限执行；Python 虚拟环境不是安全沙箱。

应用更新仍可检查版本、打开发布页；本地安全构建禁止通过应用内更新覆盖安装上游包。

## 手动登录验收工具

```sh
cargo run --locked --features custom-protocol --example login_acceptance
```

可先在命令末尾添加 `-- --probe`，仅检查登录控件，永不读取 `account`。

当前 Unity 客户端使用辅助模式：

```sh
cargo run --locked --features custom-protocol --example login_acceptance -- --unity-assisted
```

在临时 Chrome 中切换至邮箱密码登录，点击空的邮箱框，等待工具填入后点击空的密码框；密码填入后由用户点击登录。工具不点击 Canvas 坐标。它只接受官方页面中当前聚焦、可编辑、标签明确的输入框，密码还必须使用 `type=password`；聊天、验证码、新密码/确认密码、字段已填值、容量不足和外部表单目标都会被拒绝。`--unity-assisted --probe` 只验证邮箱控件，随后关闭窗口；实际登录需再运行不带 `--probe` 的命令。

仅支持已检查权限的本机 macOS。工具固定使用官方页面 `https://game.maj-soul.com/1/` 和官方 Chrome，创建临时独立浏览器目录；没有截图、录屏、网络转储、控制台订阅或输入值日志。仅输出固定状态码。正常结束、超时、取消和可处理的错误路径都会终止其浏览器并清理临时目录；不发起对局。

工具先确认页面与明确的同源登录表单（或辅助模式下当前聚焦的邮箱控件），再以 O_NOFOLLOW 打开根目录 `account`，检查所有者、0600、普通文件和单一硬链接。两行仅去除换行符，密码其他空白保留。凭据只在进程内存和发往本地 CDP 的输入操作中出现，不通过 argv、环境变量或工具输出来回传。出现验证码时暂停自动输入，供用户在临时窗口完成；超时报告阻塞。默认自动表单模式下，若当前客户端只提供无法确定语义的 Canvas 登录控件，返回 `UNITY_LOGIN_CONTROLS_UNSUPPORTED`；客户端尚未就绪则返回 `UNITY_CLIENT_NOT_READY`，其他无法识别表单返回 `LOGIN_FORM_UNSUPPORTED`。这些状态都在读取凭据之前产生。辅助模式等待点击邮箱框的超时状态为 `USER_FOCUS_EMAIL_REQUIRED`，同样尚未读取凭据；后续等待密码框超时为 `USER_FOCUS_PASSWORD_REQUIRED`。

读取凭据后，即使取消或外层超时，工具也在清理临时目录前用内存中的凭据检查验收日志；缺失/不可读的日志按失败处理。

登录后以成功的 `.lq.Lobby.loginSuccess` 响应确认大厅协议完成，随后在内存中扫描本工具唯一的验收日志，检查原值及常见编码，不输出匹配内容。`CREDENTIAL_LEAK_CHECK_PASSED` 只表示本次日志检查通过；`LOBBY_PROTOCOL_CONFIRMED` 才表示大厅协议验收成功。无此状态不得声称真实账号验收通过。

## 使用已有 Edge 会话进行实战测试

常用 Edge 在 Remote debugging 页面显示 `Server running at: 127.0.0.1:9222` 后，使用：

```sh
./target/release/akagi --config configs/edge-live.toml
```

这个配置固定连接本机 9222，并从常用 Edge 目录只读获取 `DevToolsActivePort`。UI 开启的调试服务可以没有 `/json/version` 接口（返回 404），程序支持直接读取该本地连接定位文件。读取时验证文件类型、所有者、符号链接及端口一致性，拒绝非回环地址；不会修改该目录或其权限。Edge 若显示调试连接授权提示，需要用户允许。可以先打开雀魂并停在大厅，但应等 Akagi 连接并订阅页面后再开始对局。

**若进入对局后才启动 Akagi：** 采集只能观察接入后的消息，缺少 `authGame` 和开局状态时，即使持续收到出牌，也无法确定本人的座位、手牌并建立对局。保持 Akagi 运行，在适合短暂重连时手动刷新雀魂标签页，让游戏自身通过 `authGame` 与 `syncGame` 恢复当前局。重启 Akagi 本身不能补回已错过的消息。程序已有恢复动作重放逻辑，但不会自动刷新正在进行的牌局。2026-09-30 的中途接入日志证实了该情形：有出牌、碰牌记录，却没有 `start_game`、`start_kyoku` 和机器人建议；90 项雀魂解析测试（含当前局重连恢复）通过。

新增可选配置 `[capture.chromium] attach_port = <本地端口>`。默认 0，仍启动专用浏览器目录；非 0 时只连接 `127.0.0.1`，拒绝发现接口的重定向、代理以及返回的非本地 WebSocket 地址。此模式仅选择 `https://game.maj-soul.com/1/` 下的页面进行采集；自动操作默认关闭，用户主动开启并保存后可用（2026-10-02 补齐附加模式链路）；Akagi 停止采集时取消后台任务，不关闭现有浏览器。它不读取、导出或复制 Edge Cookie 数据库。

本机临时配置为 `configs/edge-live.toml`（0600、Git 忽略），关闭云推理和外部机器人，启用内置本地机器人。`proxy.enabled=true` 是旧版“自动启动采集”开关，实际采集后端仍为 Chromium，不启动 MITM。

Edge 必须显式开放本地调试接口。如果启动参数未生效，可按 [Microsoft 官方说明](https://github.com/MicrosoftDocs/edge-developer/blob/main/microsoft-edge/web-platform/devtools-mcp-server.md#step-1-enable-remote-debugging-in-edge)在 `edge://inspect` 的 Remote debugging 页面手动开启本实例调试。调试接口具有访问浏览器会话的能力；应仅在本地测试期间开启，结束后关闭该开关。无需安装根证书、改变系统代理或关闭 TLS 验证。

### Edge 403 连接排查（2026-09-30）

用户后续报告即使点击允许仍出现 HTTP 403。三次失败日志表明握手在 5–36 毫秒内被拒绝；当时没有保留响应正文，无法判断具体的浏览器拒绝分支。本次检查确认 9222 仍由原 Edge 进程监听，定位文件与报错地址一致。同地址的只读握手及 Chromium 控制库探测均成功；仅重启 Akagi 后，实际应用也重新订阅了雀魂并收到双向数据，没有重启 Edge 或重新登录。这确认故障位于本地调试握手阶段，但尚未复现“点击允许后仍拒绝”的确切浏览器条件，不宣称彻底修复 Edge 的授权行为。

[Chromium 的授权握手实现](https://github.com/chromium/chromium/blob/main/content/browser/devtools/devtools_http_handler.cc)会在批准回调拒绝连接时返回 403，Origin 拒绝也可返回 403。新代码仅在内存中分类这些固定响应，提供 `CDP_APPROVAL_REJECTED`、`CDP_ORIGIN_REJECTED`、`CDP_FORBIDDEN` 等诊断；未知响应保持未知，不把所有 403 都解释为用户拒绝。通知区分别提示等待连接、浏览器已连接及游戏页已订阅；握手等待最多 120 秒。错误不再夹带调试地址、响应正文或任意底层错误，日志仅额外保留布尔状态。

出现授权拒绝时，保持 Edge 窗口打开，在 Akagi“设置 → 采集”手动重启一次，并处理这一次请求的浏览器授权。如果仍立即拒绝，可在 Edge 调试页手动关闭再开启远程调试后重试。新尝试会重新读取定位文件。403 不自动循环重试，不添加 `--remote-allow-origins=*`，不修改浏览器授权设置，不放宽 TLS 或沙箱。

验证：56 项 Chromium 回归测试通过，正式版构建成功。新增本地 HTTP 握手测试覆盖授权拒绝、Origin 拒绝、未知 403、404、500 与超时，确认不发送 Cookie/认证头/Origin、不自动重试拒绝的连接，且错误与通知均不泄露模拟响应秘密或端点标识。恢复会话的日志再次通过本地内存凭据检查。已保留恢复后的运行实例，新诊断逻辑在下次启动正式版时生效。

### 页面发现稳定性修复（2026-09-30）

后续“先启动 Akagi 再进入游戏”的失败会话 `20260930-210810` 已记录握手成功，但没有页面订阅记录，Inspector 为空；独立只读查询能发现一个官方游戏页面。这与上一段缺少开局状态的中途接入问题不同。旧实现逐个等待所有标签页的 `Page.url()`，没有超时；chromiumoxide 在标签页初始化未完成时可能不处理该请求，阻塞整个页面发现循环。模拟 CDP 服务已复现页面 URL 请求不返回，同时验证新发现逻辑仍能找到官方游戏页；未直接取得此前运行实例的异步调用栈，因此不把该次卡点视为已证明的唯一根因。

修复后从浏览器 `Target.getTargets` 的元数据筛选官方 HTTPS 游戏页，不查询无关标签页的 URL；发现、页面句柄获取和订阅分别设置超时。暂时无法获取句柄不会丢弃仍有效的订阅，已结束的订阅任务会重建。连续三次发现失败或游戏页无法订阅时结束当前连接，附加模式对传输故障最多重连三次（等待 2/4/8 秒，每次重读定位文件）。授权拒绝及授权超时不重试。仅雀魂的附加模式跳过天凤脚本拦截和执行；事件监听器先安装，再启用网络事件采集。日志只新增页面数量及布尔状态。

另增加对局状态通知：检测到缺少 `start_game` 的牌局动作时提示手动游戏重连；收到 `start_game` 与 `start_kyoku` 后才提示状态就绪，不自动刷新牌桌，不从其他标签页或 Cookie 补取账号资料。

验证：最终 59 项 Chromium 测试通过，正式版构建成功。新版本已替换并启动；同一常用 Edge 的现场日志显示发现 1 个官方游戏页，句柄从暂不可用变为可用，随后完成订阅并收到脱敏数据。当前已开始的对局仍需游戏自身重连补齐状态，不能用页面订阅成功代替牌局恢复验收。

本次连接模式的 53 项 Chromium 测试通过（含本地端点限制、页面范围和停止后取消后台任务），113 项前端测试及生产构建通过。新增本地定位文件支持后，18 项启动模块测试通过，debug 和 release 构建通过。常用 Edge 已监听 `127.0.0.1:9222`；用户允许连接并在该浏览器打开大厅后，正式版已订阅官方雀魂页面。抽查 12 条双向 WebSocket 记录全部为 `redacted`，解析参数只保留类别与编号，所有新日志权限为 0600；本地内存凭据检查输出 `SESSION_LOG_PRIVACY_PASSED`。该模式完成大厅采集验证；下述第一局完整实战结果来自此前的独立 Edge 窗口，本次未要求用户重打一局。

## Edge 第一局人机实测结果

当前可用配置为 `configs/edge-game.toml`（0600、Git 忽略），启动方式：

```sh
./target/release/akagi --config configs/edge-game.toml
```

此配置由 Akagi 启动官方 Edge，使用项目下 `edge-game-profile/`（0700、Git 忽略），本地调试接口只监听回环地址。`scripts/prepare_edge_game_profile.py` 仅复制原 Edge 中 `https://game.maj-soul.com` 独立分区的 IndexedDB 到新目录；不读取 `account`，不输出站点数据，不复制其他网站、密码库、浏览器历史或整份用户配置。这个游戏的 Cookie 查询计数为 0，实际复用的是站点保存的会话。新窗口已自动恢复登录和人机局，整个复制过程没有解密或输出会话值。该目录包含登录状态，需要按私有会话数据保管。已有目录不会被脚本覆盖。

用户在独立 Edge 中确认建议正常，并按内置模型建议完成东一局，随后明确选择“本次只验收已完成的第一局”。本次范围据此止于东一局结算、进入东二局，不再要求完成整场东风局。

- 采集到 `authGame` / `syncGame` 恢复过程；记录 94 条 MJAI 事件，包含 44 次摸牌、44 次出牌、一次碰牌、一次和牌及一次局结算；已产生东二局开始事件。
- 仅使用 `akagi-native`，产生 16 条本地建议。用户已在界面确认建议可用；未安装外部机器人，未启用云推理或云复盘。
- 同一批 94 条真实牌局事件，在流日志与 Inspector 中逐项一致。使用 `replay_session_analysis` 重放这些事件，94 次分析均完成，向听范围与风险数值检查通过。
- 所有已检查的 WS 帧均为 `redacted`，解析副本仅含消息类型/编号；HTTP 未保存正文、Cookie、Authorization 或 URL 查询参数。新日志文件权限全部为 0600。
- 本地检查器 `audit_session_logs` 在内存中读取凭据，对本次所有会话日志及运行终端日志检查原值和常见编码，结果为 `SESSION_LOG_PRIVACY_PASSED`。未将匹配内容或凭据输出给模型。该结果对应检查时已产生的日志，不保证其他软件或历史副本中没有凭据。
- 存在一次仅 7 字节的未解析下行帧；已按元数据记录，原文未保存。后续恢复、出牌和结算正常，不把它认作整条数据链路失败，也不声称支持所有未知协议消息。
- **整场历史列表写入未实测**：日志尚无 `end_game`，因此历史索引尚未生成，符合“整场确认结束后保存”的实现。第一局 MJAI 记录已在本地落盘；历史功能的既有回归测试通过，但不能替代本次未做的整场实机验收。

当前 Akagi 和独立 Edge 窗口保留供继续使用。如果继续使用独立 Edge 配置，原 Edge 的远程调试开关可以关闭；如果改用 `edge-live.toml` 接入常用 Edge，则需保持该开关开启。未再次重启或关闭用户正在进行的游戏。

补充本地检查命令（仅输出状态或计数）：

```sh
cargo run --locked --features custom-protocol --example audit_session_logs -- logs/edge-game
cargo run --locked --features custom-protocol --example replay_session_analysis -- logs/edge-game/20260930-173652/inspector.jsonl
```

## 依赖复查

已升级至 Tauri 2.12.0、rustls 0.23.45、h2 0.4.19，更新兼容的直接/传递依赖及两个 Cargo.lock；前端执行兼容范围的 `npm audit fix --ignore-scripts`，没有强制批量大版本升级。

`python3.13 scripts/audit_dependencies.py` 仅发送公开的包名和版本至 OSV，结果保存在 `dependency-audit.json`。本次扫描 706 个 Rust 包版本；同一问题可能有 GHSA/RUSTSEC 两个编号，不能按条数当作独立风险。

剩余条件：

| 依赖/公告 | 本使用方式的适用条件 |
| --- | --- |
| quick-xml 0.38.4 / RUSTSEC-2026-0194、0195 | 经 plist/Tauri 传递依赖引入；恶意 XML 属性/命名空间可消耗 CPU/内存，修复要求 0.41。不接受游戏 HTTP 原文作为 plist；本路径未发现网络 XML 解析入口。将来加载不可信 plist 时须重新评估。没有强行替换 Tauri 的不兼容传递依赖。 |
| glib 0.18.5 / RUSTSEC-2024-0429 | GTK/Linux 依赖的 VariantStrIter 不安全；本次 macOS 不编译此路径。Linux 仍须修复或验证上游 GTK 依赖组合。 |
| rand 0.7.3 / RUSTSEC-2026-0097 | 仅 phf 代码生成构建链；要求 log+线程 RNG、自定义日志器重入 RNG 等条件。本锁定功能未启用 log，日志器也不调用该 RNG。 |
| fxhash、paste、proc-macro-error | 停止维护公告；保留维护风险记录，不等同于已确认的远程利用。 |
| npm file-type / GHSA-5v7r-6r5c-r473 | mahgen→旧 Jimp 的 ASF 解析拒绝服务。mahgen 发布的浏览器模块无外部导入，不含 Jimp/file-type；本项目运行时使用已生成的浏览器模块。不要用这条旧构建工具链处理不可信图像。 |
| npm phin / GHSA-x565-32qp-m3vf | 旧 Jimp HTTP 下载重定向可携带敏感头；同样未被导入浏览器运行模块。本次没有调用 Jimp 下载工具。 |

## 本机验证结果

2026-09-30，本机 macOS x86_64，Rust 1.98.1、protoc 36.2、Google Chrome 154.0.8037.58。Chrome 通过 codesign 完整性验证，签名为 Google LLC (EQHXZ8M8AV)。本次为构建安装了 protobuf，并更新了 stable Rust 工具链。

- Rust 主项目：最终 990 项测试通过；保留原有忽略的云服务实测、性能基准及示例文档测试，未调用真实云 API。
- 前端：原有 99 项测试通过；新增 Unity 输入边界 14 项虚构凭据测试通过（合计 113 项），TypeScript 与 Vite 生产构建通过，覆盖官方地址、错误控件、密码原样填入、事件传递、容量和验证挑战边界。
- 原生机器人：49 项独立测试通过，包括 3p/4p 引擎回放和一致性测试。
- 通过的安全测试包括真实 protobuf 虚构登录帧经过解析器/日志出口、HTTP 请求/响应、未知帧、原始/编码密钥扫描、终端和广播、正常 TLS 以及不可信根/域名不匹配/过期/错误握手签名、下载摘要/签名拒绝、私钥权限及链接拒绝。
- release 优化构建通过，`target/release/akagi --help` 运行成功。
- 无凭据登录探测：`UNITY_LOGIN_CONTROLS_UNSUPPORTED`。页面已完成 Unity 初始化，但没有可安全定位的邮箱/密码/提交 DOM 控件。公开 WASM 和 data 资源的 HTTPS HEAD 检查均为 200；这不是放宽 TLS 可以解决的输入定位问题。
- 探测完成后已关闭其 Chrome，检查临时 `akagi-login-*` 会话目录剩余数量为 0。`account` 内容未被读取，权限和 Git 忽略状态复查通过。
- Unity 辅助无凭据探测：用户聚焦的官方 `電郵/賬號登錄` 输入框已被识别，返回 `FIELD_READY`；该探测窗口随后取消并清理。新增 Rust 验收工具日志检查测试通过，覆盖原值、Base64、正常脱敏日志及日志缺失。
- **真实账号大厅验收已完成**：辅助工具依次输出 `EMAIL_FILLED_USER_FOCUS_PASSWORD`、`PASSWORD_FILLED_USER_CLICK_LOGIN`、`CREDENTIAL_LEAK_CHECK_PASSED`、`LOBBY_PROTOCOL_CONFIRMED`。凭据仅由本地进程读取，未通过工具输出传给模型，没有发起对局。
- 上述实际验收结束后，临时 `akagi-login-*` 目录和验收 Chrome 进程均为 0；`account` 仍为当前用户拥有的 0600 普通文件、单一硬链接，Git 忽略有效。
- 用户随后将范围扩展为现有 Edge 中的人机实战测试；这与已完成的大厅验收分开记录。

仅针对本机 macOS，不声称 Windows/Linux 实机验证。


## 常用 Edge 的自动打牌修复（2026-10-02）

用户确认识别与建议正常，但自动打牌不执行。检查发现附加模式原先向 CDP 传入 `autoplay=None`，且未传递时间预算和输入确认计数器，因此自动打牌模块没有游戏页面可操作；这是此前只读采集路径的实现限制，不是模型未输出建议。检查时本机 `configs/edge-live.toml` 中 `autoplay.enabled=false`，未自动改为开启。

现在附加模式与自动打牌模块共享页面、时间预算及输入确认。页面订阅成功后即可绑定，不依赖重新建立 WebSocket；停止或断开时清理绑定，重连时重新绑定。仅存在一个官方游戏标签页时才绑定输入目标，多个游戏标签页时暂停操作。每次初始点击、重试和自动恢复刷新前检查当前页面仍处于官方 HTTPS 游戏路径，检查失败或超时则停止该操作。页面检查只返回布尔值，不输出网址、Cookie 或页面内容。

使用时先确认牌局和建议正常，再在“设置 → 自动打牌”开启并保存。保留一个官方雀魂标签页。代码修复不代表完成真实点击验收；下述测试与现场结果分别记录。

验证：152 项自动打牌相关 Rust 测试、59 项 Chromium 测试、90 项雀魂解析测试、8 项官方页面边界测试和 TypeScript 检查全部通过；正式版构建成功。当前运行实例未重启，新版在下次启动时生效。本次未执行真实牌局自动点击，当前配置中的自动打牌开关保持关闭。
