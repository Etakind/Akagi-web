# Maintaining the local web fork

**English** | [简体中文](FORK_MAINTENANCE.zh-CN.md) · [User guide](../README.md)

This document records the owner's approved scope and merge rules for `Etakind/Akagi`.
The current change is `feature/local-web-platforms`, based on personal `dev` commit
`68b47ada54982f46a1206887193fd8ada1f80df9`. The inspected upstream baseline is
`shinkuan/Akagi` branch `v3`, commit `cd68865f9e93eddcda6451cd18874a6f68c5fb49`.
Comparisons below describe that baseline, not an unverified future upstream release.

## Scope and differences

Only **Majsoul and Tenhou official web clients** are runtime choices. Keep the desktop
UI, local 3p/4p advice, Majsoul overlay, history, Inspector and opt-in automatic play.
Do not restore MITM, certificate management, system proxy, native-game interception,
cloud services, external executable bots or bundled Python/uv during an upstream merge.
Old protocol labels in history/Inspector remain read compatibility, not capabilities.

| Area | Upstream baseline → maintenance behavior | Reason and merge rule | Implementation and migration | Evidence / follow-up |
|---|---|---|---|---|
| Capture | Multiple capture/game backends → Chromium CDP with two official web clients | Reduce privileged interception and unused attack surface. Never install a CA, change system proxy, disable TLS, sandbox or Origin checks. | `src/capture/chromium/`, `src/config/platform.rs`, `src/bridge/{majsoul,tenhou}/`; mode stays `chromium`. | Restored Tenhou code selectively from the stated baseline; no current replay/live-game regression. |
| Browser attachment | HTTP-centric endpoint discovery → HTTP then fixed local locator fallback when no profile is specified | Edge's UI-enabled server may return 404 for `/json/version`. Never inspect session databases to find a browser. | `launch.rs`, `detect.rs`, `connection.rs`, `discovery.rs`; details below. | Local production build passed; locator discovery reached authorization, which timed out. No official-page subscription confirmed in this attempt. |
| Actions | Platform adapters → opt-in, unique selected official page, generation/window guards | A stale action must not reach a different page, game or decision. Failure pauses input, never refreshes the game. | `src/autoplay/`, `cdp.rs`; game/autoplay changes invalidate context. | Source review only for this revision; real input and client-change behavior unverified. |
| Inference | Local / external / remote options → embedded `native` / `native3p` only | Remove remote game uploads and arbitrary Python bot/dependency execution. | `src/bot/{native,manager,supervisor}.rs`; external selections migrate to bundled models and disable autoplay. | Local compilation; no inference consistency regression executed now. Training/conversion tools remain development-only. |
| Review and accounts | Cloud review, sharing, keys, billing and subscriptions → removed | Eliminate application upload paths and remote account/credit state. | Removed API modules, IPC handlers, routes/stores/translations. `src/network.rs` owns remaining HTTP client construction. | Source network inventory, not a packet-capture proof. No local review or model import feature added. |
| Themes | Remote/theme expressions → local JSON and constrained literal colors | Prevent theme CSS from fetching remote resources. Production WebView CSP restricts resources to local assets and Tauri IPC. | `themeStore.ts`, `tauri.conf.json`, `main.tsx`; unsafe cached CSS is not injected at boot. | Production frontend compilation; CSP/WebView behavior on other systems unverified. |
| Logs and files | Raw protocol/config information could persist → metadata redaction and private files | Login frames, raw HTTP bodies and secrets must not reach logs or Inspector. Keep parsing data intact. | `src/privacy.rs`, `logger/`, `inspector/`, `schema/inspector.rs`, `util/private_fs.rs`; legacy `FrameRaw.text/binary` readable, new frames `redacted`. | Existing regression sources retained; not executed this revision. Historical results are separately versioned. |
| Updates/downloads | Upstream auto-install/mirrors and bot installers → personal release metadata, manual installation, explicit official browser downloads | Preserve the reviewed build and avoid untrusted executable additions. Do not reinstate upstream overwrite installation. | `updater/check.rs`, `capture/chromium/cft.rs`, `network.rs`; old mirror configuration is unused. | Static review; no release/download installation performed. |
| Distribution | Upstream packaging → five declared targets, no Python/uv or AppImage | One target inventory avoids divergent workflows and filenames. Installed data must use writable user locations. | `build/targets.json`, `build_targets.rs`, `platform.rs`, `util/mod.rs`, `scripts/package.py`, workflows. | Only host macOS x86_64 binary built. Matrix entries are not build/device acceptance. |

### Network boundary

Akagi does not upload accounts, games, history, logs or inference data to remote services.
Inference runs locally. Update checks and user-initiated downloads remain available.
This is the implemented source boundary; a full traffic audit is still a future check.
The game website still connects to its own servers, and optional actions use its client.
An open browser is not an offline system.

Remaining app requests have explicit purposes:

| Purpose | Destination and constraints | Data |
|---|---|---|
| Release information | Fixed GitHub API endpoint for `Etakind/Akagi`, strict HTTPS, no redirect fallback | Ordinary metadata GET and app version in User-Agent; no game/account payload |
| User-requested browser acquisition | Official Chrome for Testing manifests and exact official HTTPS assets; redirects rejected | Version/platform selection; no gameplay payload |
| Browser control | Loopback-only HTTP/CDP endpoint, no discovery proxy or redirects | Game observation and optional input inside the local browser |

Private GitHub API access may fail anonymously. Offer the personal release page through
the user's browser; do not ask for browser Cookies, borrow its credentials, or substitute
an upstream installation. No inference/capture startup path downloads missing browsers.
The remaining URL-opening command is a user action, not a background uploader.

### Edge discovery and connection rules

- Explicit user-data directory: read its `DevToolsActivePort` with file/owner/link/port
  checks. Reject an unsafe or mismatched locator; do not connect another browser. A
  missing locator can use standard HTTP discovery on the same configured port.
- Empty directory: try standard HTTP discovery first; if unavailable, inspect only known
  browser user-data roots. An explicit executable restricts the browser family. Read only
  the locator, without recursion, Cookie/session reads or profile permission changes.
- Deduplicate valid loopback endpoints matching the selected port. Connect only one;
  ambiguity requires an explicit directory. Rediscover on every transport reconnection.
- Distinguish unreachable port, unavailable discovery, rejected locator, ambiguity,
  authorization rejection/timeout, no official page, and missing initial game state.
  Full endpoints, locator contents and arbitrary handshake responses stay out of logs.
- A 403 is not automatically a user denial: Origin and unknown refusal are separate
  classifications. No repeated automatic 403 retries or Origin bypass flags.
- Attach mode leaves the user's browser open when capture stops. Independent mode uses
  a private Akagi profile and rejects known ordinary browser roots. Do not reset the
  user's session files or navigate an attached page to make attachment succeed.

The allowlist is exact HTTPS origin/path scope: `game.maj-soul.com/1/` and `tenhou.net/4/`,
with no URL credentials or nondefault port. Similar domains are rejected. Multiple
matching pages disable binding instead of arbitrarily choosing one. A matching URL
alone does not prove a hand can be reconstructed; parsing readiness is a separate gate.

### Tenhou adapter and automatic play

Automatic play is off by default. Observe WebSocket messages without script modification
when it is off. When enabled, prepare a response interceptor only for the selected
Tenhou page and recognized official `/4/` client scripts. Require successful status,
bounded size and the expected source structure before exposing the internal discard entry.
The adapter does not relax browser TLS or execute arbitrary downloaded bot code.

Independent launch prepares listeners and the adapter before navigating a new page.
Attachment to an already loaded client may have no adapter: report that input is paused;
the user can safely re-enter later. Do not refresh on their behalf. Unknown client code,
missing adapter, page ambiguity, stale decision window or missing initial hand stops
input while available observation/advice remains. Delay Lua stays in its restricted
runtime, without file/process/network APIs. Tenhou 3p/4p behavior and actual automatic
clicks still require dedicated future regression and human acceptance.

## Configuration and local data

| Setting / old data | Current behavior |
|---|---|
| `capture.enabled` | Explicit value wins; otherwise read old `proxy.enabled`; default true. |
| `capture.mode` | Only `chromium`. A removed mode stops capture and shows an explanation. |
| `platform.kind` | `Majsoul` or `Tenhou`; known old Tenhou Chromium configs work again. Removed games stop capture without resetting unrelated settings. |
| `capture.chromium.attach_port` | Nonzero attaches; zero launches an isolated browser. Empty `user_data_dir` means automatic locator discovery only in attach mode. |
| `bot.active_4p/active_3p` | Fixed bundled models. Old external selections produce a notice and disable autoplay. Old remote API flags cannot restore uploads. |
| Saved obsolete fields | On normal save, remove known proxy/cloud/external-bot/mirror fields; preserve unrelated settings and unknown user fields. Unknown fields do not create executable capabilities. |
| `capture.http.bodies` / `record_all` | Bodies never captured; `record_all` only broadens redacted HTTP metadata. |
| History / Inspector | Existing files are not rewritten. Legacy source labels and raw frame formats remain readable; new recordings are redacted. |

Only tracked obsolete code/resources are removed. The application does not clean old
`account`, local configs, browser profiles, CA keys, logs, histories, external bot folders
or Python environments. They may remain sensitive and unused. Old CA files and OS trust
are separate: deleting implementation does not revoke installed trust. The owner must
review any cleanup or trust removal. Do not commit any of these artifacts, even privately.

Unix private files use 0600 and private directories 0700 with safe-write/link checks.
Existing Windows ACL and reparse-point behavior still needs a dedicated device review;
Unix modes are not a Windows confidentiality guarantee. Writable portable installs use
executable-relative data paths; read-only installed locations fall back to user config/data
roots. Existing configuration search order and explicit absolute paths remain respected.
Do not write application data into `/usr` or silently move prior histories.

## Building, packaging and release rules

`build/targets.json` is the target source for Rust browser mapping, CI and packaging:

| Target | Native runner / baseline | Assets |
|---|---|---|
| `x86_64-pc-windows-msvc` | Windows | ZIP |
| `x86_64-apple-darwin` | Intel macOS | ZIP |
| `aarch64-apple-darwin` | Apple Silicon macOS | ZIP |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | ZIP / DEB / RPM |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 ARM | ZIP / DEB / RPM |

See [README build commands](../README.md#build-from-source). Linux needs GTK3/WebKitGTK
4.1 and the dependencies declared in `tauri.conf.json`. DEB targets Ubuntu 22.04/24.04 and
Debian 12/13; RPM is documented for Fedora; Arch builds from source. These are intended
compatibility targets, not distro acceptance results. Ubuntu 22.04 is the chosen older
[Tauri baseline](https://v2.tauri.app/distribute/appimage/); no AppImage code is restored.

Packaging validates the target before creating/copying/downloading/deleting anything.
It includes the embedded models, licenses, NOTICE, and usage docs, not Python/uv or local
runtime data. Linux native packages come from the matching Tauri build. Each target has
an asset inventory and SHA256 file. Hashes establish consistency, not publisher identity.
Optional minisign uses an already configured key only after a probe verifies against the
repository public key. Missing keys leave assets unsigned; mismatch stops signing. Never
automatically generate/rotate signing identities or claim macOS notarization/Windows signing.

CI covers `dev` pushes and PRs targeting `dev`. The authorized `/build-artifacts` PR
workflow and manual release reuse the same five-target builder. No tag-triggered releases,
scheduled protocol updates or automatic merges. Manual release defaults to artifacts only;
publishing requires an explicit existing tag for that exact build commit. This revision
only changes configurations: no workflows, packages or formal Release were dispatched.

## Remaining risks and improvements

| Risk and trigger | Current boundary | Follow-up |
|---|---|---|
| CDP grants access to a browser session | Loopback, explicit browser authorization, official-page selection; never read session databases | Prefer isolated profiles; close debugging when unused; review changes in browser authorization and locator security. |
| Automatic play and game rules | User opt-in, page/window checks, fail closed | User must assess account/game policy consequences; validate actions manually on each client version before relying on them. |
| Tenhou client changes | Recognized source shape only; missing adapter pauses input | Recorded replay and changed-script fixtures, UI adapter versioning and real input acceptance. |
| Local information | Redacted transport logs, private new files; MJAI/history still include game and player information | Review old logs/backups and Windows permissions; do not distribute local histories or old raw Inspector exports casually. |
| Unsigned desktop binaries | Manual provenance/hash/signature checks; no automatic OS trust modifications | Provision owner's signing/notarization identities separately if desired. |
| Linux glib advisory | Restoring Linux makes GTK/glib a runtime dependency again | `glib 0.18.5` is affected by [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), unsafe `VariantStrIter` iteration. A malicious/invalid variant reaching that iterator is the relevant condition; application reachability is not established here. The earlier “Linux disabled” exclusion no longer applies. Coordinate GTK/Tauri compatible upgrades or a reviewed backport; upstream fixed glib at 0.20.0. |
| Other dependency/system-library defects | Strict TLS retained; no forced major dependency upgrades | Reaudit the changed lockfile and enabled features; maintain supported OS libraries and assess actual parsers/inputs, not just alert counts. |

`dependency-audit.json` is an older snapshot, not a new scan. Historical quick-xml concerns
involved attribute/namespace processing through plist; historical rand findings depended
on reentrant logging; npm mahgen transitives involved Node image/file/download paths.
Those old reachability notes are leads for a fresh audit, not proof for this revision.
Unmaintained dependencies remain maintenance debt. Nothing here establishes malicious
exfiltration; removing unused upload paths reduces exposure without claiming all bugs are gone.

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
Normally review CI before merging. For **this change**, the owner explicitly requested no
CI, PR or merge: push only the feature branch and leave `dev`/`main` untouched.

## Evidence and future validation

Current evidence: source/static review and production frontend plus host release builds.
[Browser acceptance note](validation/2026-10-03-browser-attachment.md) separates endpoint
discovery success from authorization timeout and unconfirmed official-page attachment.
No automated tests, Clippy regression, CI, traffic audit, cross-platform builds, real game,
automatic click, or other-device acceptance was performed for this revision.

Future regression checklist (not marked passed):

- Locator discovery/reconnect, unsafe files, ambiguity, 403/timeouts, unique-page binding.
- Tenhou/Majsoul replay, local advice consistency, late attachment, history and PT results.
- Script changes, disabled autoplay, stale windows and disabled/changed game during a plan.
- Old cloud/external configuration migration, unknown-field preservation, no-upload inventory.
- Fictional credential redaction across all logs and Inspector, legacy record compatibility.
- Linux glib reachability and renewed Cargo/npm audit; CSP on each native WebView.
- Rust/frontend/native-bot regression and five-target native packaging/device acceptance.

Older successful tests belong to their old commits and remain in
[SECURITY_HARDENING.md](../SECURITY_HARDENING.md) and Git history. Never use them as current
results. Update both language versions, code pointers and evidence with related code or
upstream merges. Changes to support scope, safety rules, automation or release policy need
the owner's item-by-item agreement; routine version/evidence updates follow commits.

The [current implementation record](validation/2026-10-03-local-web-platforms.md) lists exact build/static checks and deferred acceptance.
