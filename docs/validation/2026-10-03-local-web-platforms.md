# Local web / platform change — 2026-10-03

[English maintenance guide](../FORK_MAINTENANCE.md) · [简中维护指南](../FORK_MAINTENANCE.zh-CN.md)

## Scope of this record / 本记录范围

Only the current macOS x86_64 development host was used. The owner explicitly excluded
automated tests, CI, real games, automatic input and other-device acceptance. No PR,
merge or release was created. Source-level checks are not network-traffic certification.

本次仅使用 macOS x86_64 本机；按用户要求不执行自动测试、CI、真实对局、自动输入或其他设备验收。
不创建 PR、不合并、不发布。源码检查不能当作实际流量认证。

## Checks / 已执行检查

- Production frontend: `npm run build --prefix frontend` passed (TypeScript + Vite).
  Existing Vite configuration/deprecation and large-chunk warnings remain; no test runner invoked.
- Rust: dependency-pruned offline release build passed; final locked production build also passed (3m 30s), embedding the final frontend. This builds the app, not `cargo test` or Clippy.
- Static parsing: workflow YAML (Ruby YAML), target/Tauri JSON and packaging Python syntax parsed.
  This does not run the workflows, packaging or regression scripts.
- Links: relative file links in tracked module/root Markdown and both new bilingual guides resolved.
- Git guard: `python3 scripts/setup-fork.py --check` returned `AKAGI_FORK_CONFIG_OK`.
  No push to upstream, including no upstream dry-run, was attempted.
- Submission review: selected tracked changes and explicitly listed new source paths only;
  no account/config/session/key/log/history/runtime data staged. Common private-key/GitHub-token
  pattern inspection found no matches in added source; not an exhaustive secret detector.
- GitHub metadata: personal repository verified private, default branch `dev`, push/admin permission
  present. `main` and `upstream/v3` remain at `cd68865f9e93eddcda6451cd18874a6f68c5fb49`.
- Runtime network inventory: HTTP clients constructed in `src/network.rs`; callers are local CDP
  discovery, personal release metadata and explicit official CfT download. Cloud/bot installer
  implementations, related IPC/UI, remote theme fetch and binary-replacement command removed.
- Dependency version inventory: Tauri 2.12.0, rustls 0.23.45, h2 0.4.19, glib 0.18.5.
  No fresh advisory scan performed. Linux restoration invalidates the old glib platform exclusion;
  conditions and follow-up are in the maintenance guide.

前端生产构建通过；静态检查包括 YAML/JSON/Python 语法、文档链接、上游推送防护、待提交路径与常见密钥格式。
个人仓库仍为私有、默认 dev，main/upstream 基线未动。版本清单不等于重新依赖审计；glib 的 Linux 排除理由已失效。

## Browser acceptance / 浏览器验收

See [the separate connection record](2026-10-03-browser-attachment.md). With the first-stage
release, standard Edge locator discovery succeeded and reached the authorization handshake.
It timed out after 120 seconds; official-page capture was not confirmed. The check process
was stopped without restarting Edge, refreshing a page, logging in, or clicking game controls.
This is not recorded as an end-to-end connection pass for the expanded second-stage build.

第一阶段只确认定位文件发现成功，授权 120 秒超时，未确认官方页面订阅。没有重启 Edge、刷新、登录或游戏操作。
不能将这次结果写成第二阶段完整接入通过。

## Deferred / 未执行

- Rust/frontend/native-bot tests and Clippy; simulated credential-leak/TLS/CDP regressions.
- Tenhou replays, adapter failure handling, 3p/4p advice consistency and live automatic input.
- Config migration behavior, new/old history and Inspector regression, CSP on native WebViews.
- Packet/traffic audit, renewed Cargo/npm audit and Linux glib call-site reachability.
- All five native package builds and OS/distribution/device acceptance; signature identity acceptance.
- CI dispatch/result monitoring, PR, merge, release publication.

以上均列为后续事项，不引用以前版本通过结果替代本次验证。

## Final local artifact / 最终本机产物

`cargo build --locked --release --features custom-protocol` passed on macOS x86_64.
Binary: `target/release/akagi` (44545824 bytes), Mach-O x86_64.
SHA256: `e21d9b34dfc0935879dd152ea588739bcbfef0ab06560f360c9c33aac46e8ab0`.

最终锁文件生产构建通过，包含最终前端。产物只生成于本机，未纳入 Git、未发布。
第二阶段没有再启动实际浏览器或发起游戏；编译成功不代表完整浏览器/对局验收。

Commits before this documentation update: `51a936e` (discovery), `e9aceb9` (games/local runtime),
`c111082` (packaging/workflows). The final documentation commit records this evidence.
