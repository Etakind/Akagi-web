# Maintaining the local web fork

**English** | [简体中文](FORK_MAINTENANCE.zh-CN.md)

This guide explains the fork's design, security boundaries and contribution rules.
For installation and everyday use, start with the [README](../README.md).
The upstream comparison uses [Akagi v3 at cd68865](https://github.com/shinkuan/Akagi/tree/cd68865f9e93eddcda6451cd18874a6f68c5fb49).

## Function differences and reasons

| Area | Upstream → maintenance fork | Reason and implementation |
|---|---|---|
| Capture | Multiple games/backends → Majsoul and Tenhou official websites over Chromium CDP | Removes interception certificates and system proxy management. `src/capture/chromium/`, `src/bridge/{majsoul,tenhou}/`. Preserve strict TLS, loopback attachment and exact official-page matching. |
| Inference | Local, external and remote options → bundled four-player/three-player models | Removes remote inference uploads and arbitrary bot/dependency execution. `src/bot/{native,manager,supervisor}.rs`. Developer training/conversion tools are not application runtimes. |
| Online services | Cloud review/sharing, API keys, subscriptions and billing → removed | There is no account, payment or upload service in this build. `src/network.rs` centralizes remaining HTTP clients. |
| Automatic play | Platform adapters → opt-in actions on a unique official page and valid decision window | Prevents input reaching unrelated pages or stale game states. `src/autoplay/`; failures pause actions without refreshing the game. |
| Diagnostics | Raw protocol/config output → redacted transport metadata | Reduces credential persistence while retaining MJAI, local analysis, history and Inspector. `src/privacy.rs`, `src/logger/`, `src/inspector/`, `src/util/private_fs.rs`. |
| Themes and UI | Remote themes / CSS expressions → local JSON with literal colors | Prevents remote resource requests through theme content. `frontend/src/stores/themeStore.ts`, `tauri.conf.json`. Local Blob Workers generate tile images; remote scripts and workers remain blocked. The build adapter `frontend/scripts/mahgen-csp.mjs` replaces the known legacy runtime initialization; dependency changes fail the build until reviewed. |
| Updates and downloads | Upstream replacement packages and mirrors → maintenance releases and explicit official browser downloads | Keeps upstream installers from overwriting fork changes. `src/updater/check.rs`, `src/capture/chromium/cft.rs`. Installation remains manual. |
| Distribution | Upstream packaging → five native targets without Python/uv or AppImage | Keeps installed dependencies aligned with the local inference design. `build/targets.json`, `scripts/package.py`, build workflows. |

## Browser and automatic-operation boundaries

Attachment first uses the configured loopback port. With an explicit user-data root,
its locator is authoritative; unsafe or mismatched files are rejected. With no directory,
HTTP discovery falls back to standard browser `DevToolsActivePort` files. Only one distinct,
port-matching endpoint is accepted. An explicit browser executable limits the search to
that browser family. Discovery never reads Cookie/session databases, scans directories
recursively, or changes an existing browser profile's permissions.

Every reconnect discovers the endpoint again. HTTP discovery failures are distinct from
browser authorization failures. A 403 stops retries; it must not be bypassed by weakening
Origin checks. Independent mode prepares capture on a blank page before navigating to the
official client. It uses an isolated profile and keeps the browser sandbox and TLS checks.

Automatic input requires the selected official HTTPS host/path, a unique page and current
hand state. Both adapters parse `location.href` as a URL before validating its fields.
Changing the autoplay switch cancels queued/in-flight actions without restarting capture
or discarding the hand. New actions require inference begun after the switch was enabled.
Page or game changes invalidate pending actions. Pauses are reflected in the UI.

Tenhou script adaptation is active only with autoplay enabled. Interception is limited to
recognized official client script URLs and supported source structures. Enabling it on an
already-loaded client cannot retrofit its entry point: re-enter the page yourself when safe.
Missing adapters, ambiguous pages and incomplete state stop input while observation remains
available. Akagi never reloads a game to repair automation. The Lua delay environment has
no filesystem, process or network access.

## Data, network and configuration

Akagi does not upload accounts, games, history, logs or inference data. Inference is local.
Remaining requests are maintenance release metadata, user-requested official Chrome for
Testing assets, and loopback CDP discovery/control. The game website communicates normally
with its own server. Private release API access failures leave a release-page link; Akagi
never asks for browser cookies or falls back to upstream installation packages.

New WebSocket records omit raw frames. HTTP records omit URL credentials/query/fragment,
sensitive headers and bodies. Inspector broadcasts use the same redaction. MJAI events,
analysis and history contain game/player information and should be shared deliberately.
New private Unix files use 0600 and directories 0700 with owner/link checks. Windows file
privacy depends on the user directory's ACL; Unix mode bits do not provide Windows access
control. Portable installs use writable application directories; system installs use user
data/configuration roots. Explicit configured paths remain supported.

| Configuration | Behavior |
|---|---|
| `capture.enabled` | Explicit value wins over the legacy `proxy.enabled`; defaults to enabled. |
| `capture.mode` | Only `chromium`; unsupported old modes stop capture with a message. |
| `platform.kind` | `Majsoul` or `Tenhou`; unsupported games do not reset unrelated settings. |
| `capture.chromium.attach_port` | Nonzero attaches; zero launches an isolated browser. An empty profile path has different meanings in these two modes. |
| `bot.active_4p/active_3p` | Bundled models only. Migrating an external selection disables autoplay and displays a notice. |
| Obsolete cloud/proxy/bot fields | Known obsolete fields are removed on save; unknown user fields survive without enabling removed capabilities. |
| `capture.http.bodies` / `record_all` | Raw bodies remain disabled; `record_all` broadens only redacted metadata. |
| History / Inspector | Legacy labels and frame formats remain readable. New raw-frame records use `redacted`. |

## Building and releasing

[README build instructions](../README.md#build-from-source) cover system prerequisites.
The shared inventory `build/targets.json` declares Windows x86_64, macOS x86_64/ARM64 and
Linux x86_64/ARM64. All produce portable ZIPs; Linux also produces DEB/RPM. Linux builds use
Ubuntu 22.04 with GTK3/WebKitGTK 4.1. DEB installation guidance covers Ubuntu 22.04/24.04 and
Debian 12/13; RPM guidance covers Fedora. Arch uses source builds. System library versions
must satisfy the package dependencies; these are not universal Linux binaries.

Packaging rejects unsupported targets before modifying files. Packages contain the models,
licenses, NOTICE and usage documentation, not user data or Python/uv. Each build produces
an asset inventory and SHA256. Optional minisign signing requires a configured key that
matches the repository public key; identities are never generated or rotated automatically.
SHA256 checks consistency, while a trusted signature verifies publisher identity.

CI covers `dev` pushes and PRs targeting `dev`. Administrator-requested `/build-artifacts`
and manual release share the native builder. Artifacts are available from the workflow run.
Manual release defaults to building artifacts; publishing requires an explicit existing tag
for that commit. There are no tag-triggered releases, scheduled protocol changes or automatic
merges.

The application and standalone `native_bot` manifests optimize `gemm-common` and
`gemm-f16` in development/test profiles. This works around their [AArch64 debug-build
issue](https://github.com/sarah-quinones/gemm/issues/31) while preserving runtime CPU
feature dispatch; do not replace it with a global `+fp16` or `target-cpu=native` setting.
Revisit these package overrides when the dependency fixes its helper annotations.

## Remaining risks and improvements

| Risk / trigger | Current protection | Improvement direction |
|---|---|---|
| CDP exposes browser session access | Loopback, browser authorization, exact official-page binding | Prefer isolated profiles; disable debugging when unused; follow browser authorization changes. |
| Autoplay conflicts with game rules or client UI changes | Explicit opt-in, stale-action cancellation, failure pauses | Check game rules before use; maintain client fixtures and adapter compatibility. |
| Tenhou script structure changes | Only recognized scripts are adapted | Version adapter fixtures and expand supported-client regression coverage. |
| Local game/player information | Redacted transport logs and private file creation | Improve Windows ACL/reparse-point handling and export privacy controls. |
| Unsigned/not-notarized binaries | Manual installation and provenance/signature checking | Add platform signing with maintained publisher identities. |
| Linux GTK/glib dependency | Linux includes `glib 0.18.5`, affected by [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) | The issue concerns unsafe `VariantStrIter` iteration; application reachability requires analysis. Coordinate a compatible upgrade or reviewed backport; glib fixed it in 0.20.0. |
| Other dependency and OS vulnerabilities | Locked dependencies and strict TLS | Assess enabled features and reachable inputs; upgrade compatible fixes and maintain system libraries. |

## Contribution checks

Run `cargo test --locked --all-targets`, the frontend tests and production build before
submitting changes. `node frontend/scripts/test-tiles-browser.mjs` checks real tile generation
with production CSP in a disposable Chromium profile; `AKAGI_TEST_BROWSER` selects the local
executable. The check opens only a local fixture, not a game or existing profile.

Keep both language versions consistent. Upstream merges must preserve local-only inference,
redaction, strict TLS, safe private writes, official-page binding and opt-in automation.
Do not reintroduce interception CAs, system proxies, remote themes, cloud uploads or external
bot execution. Test attachment/reconnection, config migration, replay/history compatibility,
autoplay cancellation and package target validation when their implementation changes.

## Repository synchronization and push protection

| Ref | Role |
|---|---|
| `origin` | Private `git@github.com:Etakind/Akagi.git`; personal code and multi-device synchronization |
| `upstream` | `https://github.com/shinkuan/Akagi`; fetch only, actual baseline branch `v3` |
| `main` | Exact upstream mirror, tracks `origin/main`; fast-forward only, no personal commits |
| `dev` | Personal default/integration branch, tracks `origin/dev` |
| `feature/*`, `fix/*`, `experiment/*` | Start from `dev`; review before integration |

New clone:

```sh
git clone --branch dev git@github.com:Etakind/Akagi.git
cd Akagi
python3 scripts/setup-fork.py
python3 scripts/setup-fork.py --check
```

Use `python` if that is Python 3's Windows command. The standard-library/Git script sets
repository-local `remote.pushDefault=origin`, `push.default=simple`, `pull.ff=only`, an
unusable upstream push URL and a local `pre-push` guard. The guard blocks upstream by
remote name and SSH/HTTPS URL. It lives under Git's local management directory, so it
survives checkout of `main`; existing hooks are preserved/chained. External shared hooks
are not overwritten. Reinstall after cloning/moving or changing Python installations.
This prevents accidents and is deliberately bypassable; it is not a remote permission wall.

Require a clean working tree; never discard user changes to synchronize. Daily work:

```sh
git switch dev
git pull --ff-only origin dev
git switch -c feature/example
# Stage reviewed files explicitly, commit, then:
git push -u origin feature/example
```

Run each upstream synchronization step separately and stop on failure:

```sh
git fetch --no-tags origin
git fetch --no-tags upstream
git switch main
git merge --ff-only origin/main
git merge-base --is-ancestor main upstream/v3
git merge --ff-only upstream/v3
git rev-parse main upstream/v3
# Both hashes must match before pushing:
git push origin main:main
git switch dev
git merge --ff-only origin/dev
git merge main
# Review personal differences, resolve conflicts, perform the agreed validation:
git push origin dev:dev
```

Stop if main diverges; no force push/reset to hide it. Resolve dev conflicts against this
document; `git merge --abort` returns to the clean pre-merge state if abandoning the merge.
Only unpublished personal commits may be explicitly rebased; never rewrite published history.
