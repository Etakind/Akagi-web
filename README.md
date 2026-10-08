# Akagi Web — local web maintenance fork

**English** | [简体中文](README.zh-CN.md)

This maintenance fork of [shinkuan/Akagi v3](https://github.com/shinkuan/Akagi/tree/v3)
uses **Majsoul and Tenhou official web clients** with bundled local inference.

Akagi Web uses independent version numbers starting at **0.1.0**; upstream releases use a separate version line.

## What differs from upstream

| Area | This maintenance version |
|---|---|
| Capture | Chromium CDP only: attach to an existing Edge/Chrome or launch an isolated browser. No MITM, CA installation or system proxy. |
| Inference | Embedded four-player and three-player models only. No cloud inference, cloud review/sharing, API keys, subscriptions or external Python bots. |
| Diagnostics | Redacted protocol/HTTP metadata, local analysis, history and Inspector. Raw login frames, headers and bodies are not stored. |
| Automatic play | Opt-in; requires a unique official page and a current decision state. Failures never refresh a game automatically. |
| Updates | Personal maintenance releases; manual installation. Upstream packages cannot replace this build. |
| Distribution | Five build targets below; no bundled Python/uv, AppImage or mobile client. |

These changes reduce credential persistence, remove remote inference/upload paths and
avoid trusting a local interception CA or downloaded executable bots. CDP still grants
powerful browser access. See the [maintenance guide](docs/FORK_MAINTENANCE.md) for
implementation boundaries, migration, remaining risks and upstream merge rules.

Akagi does not upload accounts, games, history, logs or inference data to remote services.
Inference runs locally. Update checks and user-initiated downloads remain available.
The game website itself still communicates with its game server; Akagi's optional actions
use that client.

## Games and features

| Official web client | Features |
|---|---|
| [Majsoul](https://game.maj-soul.com/1/) | 3p/4p parsing and local advice, overlay, history, Inspector, optional autoplay |
| [Tenhou](https://tenhou.net/4/) | 3p/4p parsing and local advice, history and PT statistics, Inspector, optional autoplay |

Tenhou autoplay needs its client adapter. It is prepared only when autoplay is enabled.

For Majsoul, automatic-action countdowns, steps and results appear in **Game → Events → Automatic actions**.
The overlay shows the latest actions only while Majsoul autoplay is effectively enabled (default 4 rows, configurable from 1 to 10).
Countdowns observe the existing plan's next mouse press, including waiting and hover; uncertain page/safety preparation is shown as “Preparing”.
Success requires matching server feedback, or a correlated error-free response for passing. Sending an input request alone is not success.
Insufficient feedback 5 seconds after the last input is “Unconfirmed”; this does not add retries, and late matching feedback on the same connection and round may confirm it.
All text in an unconfirmed record is red; “Awaiting feedback” is not unconfirmed. The overlay base font size is configurable from 12–24px (default 14).
All overlay text and row heights scale together without affecting the main window. Larger fonts and more records require more screen height; adjust either for your screen.
In-memory records survive rounds, same-match reconnects and match completion, and clear when a new match is confirmed. Remaining turn time is not displayed.
The protocol cannot fully distinguish simultaneous identical manual and automatic actions; records are not independent proof of input attribution.
If attaching after the client script has loaded, re-enter or refresh **yourself when safe**.
If the client changes or the current hand cannot be reconstructed, actions stop; wait
for a complete next hand.
History is finalized when the complete game ends, not after each individual hand.

## Packages and running

| OS | CPU | Package formats |
|---|---|---|
| Windows | x86_64 | `windows-x64.zip` |
| macOS | x86_64 | `macos-x64.zip` |
| macOS | ARM64 | `macos-arm64.zip` |
| Linux | x86_64 | `linux-x64.zip`, DEB, RPM |
| Linux | ARM64 | `linux-arm64.zip`, DEB, RPM |

Get builds from [maintenance releases](https://github.com/Etakind/Akagi-web/releases)
Verify the asset's SHA256; verify
its minisign signature when one is supplied.

Unzip into a user-owned directory. Run `akagi.exe` on Windows (WebView2 required),
`./akagi` on macOS/Linux. Linux also needs its GTK/WebKitGTK runtime libraries; ZIPs do
not bundle a Linux system. DEB targets Ubuntu 22.04/24.04 and Debian 12/13; RPM instructions
target Fedora. Install with `sudo apt install ./akagi-*.deb` or `sudo dnf install ./akagi-*.rpm`.
Arch uses source builds. Package compatibility depends on the installed system libraries.

Programs may be unsigned/not notarized: verify provenance before authorizing OS execution.
Do not disable browser TLS/sandboxing or globally remove OS quarantine protection.
In a writable portable directory, data is normally next to the executable; read-only
system installations use user configuration/data directories. Existing explicit paths
remain respected.

### First run: configure and save

The initial configuration screen is **not a stripped-down application**. Select the game platform, browser and capture options,
then click **Save** to enter the main interface with Overview, Game, Bots, History, Logs and Settings. A failed save leaves the setup screen open.
Source builds and release packages use the same initialization flow.

With capture enabled by default, Akagi automatically opens its controlled browser and the Majsoul page. Logging in there is useful:
independent browser mode uses a persistent isolated profile (normally `chrome-profile` next to the executable), so reusing the **same directory**
can preserve cookies and login state. The game server may expire or revoke the session; automatic login is not guaranteed.
Changing the directory, moving only the executable without its profile, or attaching to another browser may not retain that login.
Akagi does not add account/password storage or copy login databases from your regular browser.

## Connect a browser

**Existing Edge:** enable its local remote-debugging service, note the loopback port
(e.g. `127.0.0.1:9222`), select the game in Akagi Settings, set Attach port to `9222`,
and restart capture. Approve the current browser prompt. Keep exactly one official
page for the selected game open. Start Akagi before entering a hand when possible.

Leave User data directory blank for automatic discovery. If `/json/version` is unavailable
(as with Edge's UI-enabled debugging), Akagi checks only known `DevToolsActivePort`
locations. An explicit executable limits discovery to that browser family. Port mismatch,
unsafe locators or multiple candidates do not cause arbitrary browser selection. For a
custom profile, enter its **user-data root**, not `Default` or an individual profile folder.
No Cookie or session database is read/copied. Every reconnect rediscovers the endpoint.

**Independent browser:** set Attach port to `0`. Leave the directory blank for Akagi's
isolated profile, or select a separate directory. Never select your regular browser's
profile. Choose an installed browser; if absent, explicitly download official Chrome for
Testing from Settings. Downloads never occur as part of inference/capture startup.

A missing port/locator, authorization rejection/timeout, missing game page and missing
hand state are different failures. A previous approval may not authorize the next
connection. Akagi does not repeatedly retry a 403 or bypass Origin checks. It never
refreshes an in-progress game to recover.

## Build from source

Use Git, stable Rust, Node.js **22+**, npm, Python **3.11+** for packaging/configuration
tools, and Protocol Buffers `protoc`. Python is not an application runtime.
Follow [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for OS toolchains.

macOS (run native builds on the intended Intel/Apple Silicon host):

```sh
xcode-select --install
brew install node protobuf
```

Windows: install Visual Studio Build Tools with **Desktop development with C++** and a
Windows SDK, WebView2, Rust's MSVC toolchain, Node.js and `protoc` on PATH. In PowerShell:

```powershell
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
.\target\release\akagi.exe
```

Ubuntu/Debian build dependencies:

```sh
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev protobuf-compiler patchelf
```

Fedora: install GCC/C++, `pkgconf-pkg-config`, `openssl-devel`, `webkit2gtk4.1-devel`,
`gtk3-devel`, `libappindicator-gtk3-devel`, `librsvg2-devel`, `libxdo-devel`, `protobuf-compiler`.
Arch: install `base-devel`, `pkgconf`, `openssl`, `webkit2gtk-4.1`, `gtk3`,
`libappindicator-gtk3`, `librsvg`, `xdotool`, `protobuf`, plus Rust and Node.js/npm.
Then on macOS/Linux:

```sh
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

`custom-protocol` embeds the production frontend. A plain Cargo release build without it
uses the development-server URL. Use [build/targets.json](build/targets.json) for exact
triples. On a matching host, build packages (substitute one listed triple):

```sh
npm exec --prefix frontend -- tauri build --no-bundle --target aarch64-apple-darwin
python3 scripts/package.py --target aarch64-apple-darwin
```

For Linux use `tauri build --target <triple> --bundles deb,rpm`, then the same packaging
script. Linux workflows use Ubuntu 22.04 as the older
[Tauri build baseline](https://v2.tauri.app/distribute/appimage/), without producing AppImage.
Cross-compilation is not promised by these native build commands.

## Risks

CDP can access your browser session; keep it loopback-only and disable it when no longer
needed. Automated play may violate game rules and cause account penalties. Client changes
can invalidate automation. Local history/logs remain sensitive; unsigned binaries and
system/library vulnerabilities also need review. Linux's existing glib advisory requires
follow-up; see the [detailed risk register](docs/FORK_MAINTENANCE.md#remaining-risks-and-improvements).

Licensing and third-party attribution: [LICENSE.txt](LICENSE.txt), [NOTICE](NOTICE).
