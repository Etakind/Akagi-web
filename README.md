# Akagi Web

**English** | [简体中文](README.zh-CN.md)

Akagi Web is a mahjong assistant for the Mahjong Soul and Tenhou web clients. Bundled models analyze games locally.

- Get discard and call suggestions for three- and four-player games.
- View suggestions beside the game in the default-enabled overlay, with adjustable text size and opacity.
- Enable automatic play in Settings.
- Browse saved games in History after a match ends.

## Changes from upstream

Based on [Akagi v3](https://github.com/shinkuan/Akagi/tree/v3), this fork focuses on local web games.

| Area | Changes in this fork |
|---|---|
| Games and capture | Focuses on the official Mahjong Soul and Tenhou web clients through Edge/Chrome, without proxy or certificate setup. |
| AI and services | Bundled local three- and four-player models; cloud inference, cloud review, subscriptions and external bots have been removed. |
| Diagnostics and privacy | Redacted diagnostic logs, with local analysis, history and log viewing. |
| Installation and updates | Windows, macOS and Linux packages, with updates from this repository's releases. |

## Security and privacy

AI inference and history stay on your computer. Akagi does not upload accounts, games or logs; diagnostic logs redact sensitive information.
Network access serves the game connection, update checks and browser downloads you request. Browser debugging uses local addresses and respects browser authorization.
Check your game's rules before enabling automatic play. See the [maintenance guide](docs/FORK_MAINTENANCE.md) for details.

## Download and install

Choose a package for your system and processor from the [releases page](https://github.com/Etakind/Akagi-web/releases).

| System | Processor | Download |
|---|---|---|
| Windows | x64 | `windows-x64.zip` |
| macOS | Intel | `macos-x64.zip` |
| macOS | Apple Silicon | `macos-arm64.zip` |
| Linux | x64 / ARM64 | Matching ZIP, DEB or RPM |

Extract a ZIP and open `akagi.exe` on Windows, or run `./akagi` on macOS/Linux.
Windows requires [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).
If macOS blocks the first launch, allow it in **System Settings → Privacy & Security**.

On Ubuntu/Debian, install the DEB; on Fedora, install the RPM. These commands also install runtime dependencies:

```sh
sudo apt install ./akagi-*.deb
```

```sh
sudo dnf install ./akagi-*.rpm
```

For Linux ZIPs, install your system's GTK/WebKitGTK dependencies listed under source installation below.

## First use

1. Open Akagi, choose your game, browser and capture options in the setup wizard, then click **Save**.
2. Log in to Mahjong Soul or Tenhou in the game browser, keeping one tab for the selected game open.
3. Start a game and view suggestions in **Game** or the overlay. Enable automatic play in **Settings** if desired.

The independent browser can reuse your login session; sign in again when it expires. Keep the `chrome-profile` directory when moving the application.
Adjust the overlay and browser options, or rerun the setup wizard, from **Settings**.

If you start Akagi during a Mahjong Soul game, start capture, click **Recover and continue**, and follow the recovery panel's instructions.

## Browser setup

**Independent browser (default):** set **Existing browser debug port** to `0`, select an installed Edge or Chrome, and leave **User data dir** blank.
You can also download Chrome for Testing through Settings. Akagi opens a separate game window.

**Existing browser:** enable local remote debugging in Edge/Chrome and note its port.
Enter the port in **Existing browser debug port** in Akagi Settings (for example, `9222`), restart capture, and approve the browser's debugging prompt.
For a custom browser profile, enter its user-data root directory.

## Install from source

Building requires Git, current stable Rust, Node.js/npm, `protoc`, and a C/C++ toolchain.
Use Node.js **24 LTS**; the minimum is **22.12**. Expand your system, check the existing environment, then add missing tools. A version means the tool is available; check PATH first if a command is missing.
After launching, follow **First use** above; browser options are in **Settings**.

<details>
<summary>macOS (Intel / Apple Silicon)</summary>

Use a native terminal on Intel or Apple Silicon. Check each group first and install only missing tools.

**1. Compiler tools:** check the Xcode tool directory and compiler version. If both work, continue; otherwise run `xcode-select --install` and wait for installation.

```sh
xcode-select -p
clang --version
```

**2. Homebrew:** these commands check PATH and both standard installation locations. A version means it is installed: skip the installer. An existing standard installation is also loaded into this terminal.

```sh
if command -v brew >/dev/null 2>&1; then
  brew --version
elif [ -x /opt/homebrew/bin/brew ]; then
  eval "$(/opt/homebrew/bin/brew shellenv)"
  brew --version
elif [ -x /usr/local/bin/brew ]; then
  eval "$(/usr/local/bin/brew shellenv)"
  brew --version
else
  echo "Homebrew not found in PATH or standard locations"
fi
```

If you see `Homebrew not found` and previously used a custom location, load `brew shellenv` from that path first. If Homebrew is absent, run its [installer](https://docs.brew.sh/Installation):

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

Follow the installer's **Next steps** to configure your shell, then repeat the check above.

**3. Git, Node.js/npm, protoc and CMake:** check versions. An existing Node.js 22.12+ with working npm can be reused; 24 LTS is recommended.

```sh
git --version
node -v
npm -v
protoc --version
cmake --version
```

If `node` is missing, first check whether `node@24` is installed. If it reports a version, load its PATH and repeat `node -v` and `npm -v`:

```sh
brew list --versions node@24
export PATH="$(brew --prefix node@24)/bin:$PATH"
```

Install only missing packages: `git`, `node@24` (includes npm), `protobuf` (provides protoc), and `cmake`. The command below lists all packages; keep the names you need:

```sh
brew install git node@24 protobuf cmake
```

Add Homebrew's `shellenv` and the required Node.js PATH setting to `~/.zprofile` (`~/.bash_profile` for bash) for new terminals.

**4. Rust:** load an existing environment and check it first. Reuse working `rustc` and `cargo`; for an older rustup-managed toolchain, run `rustup update stable` and `rustup default stable`.

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

If Rust is still missing after loading the environment, run the [Rust installer](https://rust-lang.org/tools/install/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

Once the tools are ready, download, build and run Akagi:

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Windows x64 (PowerShell)</summary>

**1. Check existing tools.** If Visual Studio/Build Tools is installed, open **Developer PowerShell** from the Start menu and run:

```powershell
git --version
node -v
npm.cmd -v
rustup --version
rustc -vV
cargo --version
protoc --version
cmake --version
nasm -v
where.exe cl
```

Reuse tools that report a version or path. Node.js must be at least 22.12 (24 LTS recommended); Rust's host should be `x86_64-pc-windows-msvc`. If `cl` is missing, first confirm you are using Developer PowerShell.

If only Rust commands are missing, check its default installation location. If this returns `True`, add it to this terminal's PATH and retry the Rust checks:

```powershell
Test-Path "$env:USERPROFILE\.cargo\bin\rustc.exe"
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

**2. Add missing tools.** Keep PATH integration enabled. After installation or PATH changes, reopen the terminal and repeat the checks above.

- [Git](https://git-scm.com/install/windows) and [Node.js 24 LTS](https://nodejs.org/en/download), with npm and PATH integration.
- [C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/): select **Desktop development with C++**, MSVC x64/x86 tools, and Windows SDK.
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) and [CMake](https://cmake.org/download/).
- [NASM](https://www.nasm.us/pub/nasm/releasebuilds/); add its executable folder to PATH. Extract the win64 ZIP from [protobuf releases](https://github.com/protocolbuffers/protobuf/releases) to `C:\Tools\protobuf` and add `C:\Tools\protobuf\bin` to PATH.
- [Rust](https://rust-lang.org/tools/install/): run `rustup-init.exe` and select stable MSVC.

Check WebView2 in Windows **Installed apps** and skip installation if available. For existing Visual Studio installations, check the C++ workload and Windows SDK in its Installer. With rustup already installed, use `rustup update stable-msvc` and `rustup default stable-msvc` to update the toolchain.

Once the tools are ready, download, build and run Akagi in the same PowerShell:

```powershell
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm.cmd ci --prefix frontend
npm.cmd run build --prefix frontend
cargo build --locked --release --features custom-protocol
.\target\release\akagi.exe
```

</details>

<details>
<summary>Ubuntu / Debian</summary>

Open a terminal in your graphical desktop.

**1. Check system tools and graphical libraries:**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

Use the package command below to complete the system dependencies. The package manager checks existing packages; there is no need to uninstall and reinstall them:

```sh
sudo apt update
sudo apt install -y git curl ca-certificates xz-utils file build-essential cmake pkg-config \
  libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev protobuf-compiler
```

**2. Check Rust:** load an existing installation first. Skip the installer if `rustc` and `cargo` work. Update an existing rustup toolchain with `rustup update stable` and `rustup default stable`.

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

Install only if Rust is still missing after loading its environment:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. Check Node.js/npm and nvm:** if Node.js is already 22.12+ and npm works, skip this group's installation commands. This also loads an existing nvm:

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

For missing or older Node.js, use [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) to install 24 LTS. If `command -v nvm` prints `nvm`, skip the first line below:

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

Repeat `node -v` and `npm -v` to confirm the environment, then download, build and run Akagi:

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Fedora</summary>

Open a terminal in your graphical desktop.

**1. Check system tools and graphical libraries:**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

Use the package command below to complete the system dependencies. The package manager checks existing packages; there is no need to uninstall and reinstall them:

```sh
sudo dnf install -y git curl ca-certificates xz file gcc gcc-c++ make cmake \
  pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel gtk3-devel \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel protobuf-compiler
```

**2. Check Rust:** load an existing installation first. Skip the installer if `rustc` and `cargo` work. Update an existing rustup toolchain with `rustup update stable` and `rustup default stable`.

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

Install only if Rust is still missing after loading its environment:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. Check Node.js/npm and nvm:** if Node.js is already 22.12+ and npm works, skip this group's installation commands. This also loads an existing nvm:

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

For missing or older Node.js, use [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) to install 24 LTS. If `command -v nvm` prints `nvm`, skip the first line below:

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

Repeat `node -v` and `npm -v` to confirm the environment, then download, build and run Akagi:

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

<details>
<summary>Arch Linux</summary>

Open a terminal in your graphical desktop.

**1. Check system tools and graphical libraries:**

```sh
git --version
curl --version
cc --version
c++ --version
make --version
cmake --version
pkg-config --version
protoc --version
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 openssl
```

Use the package command below to complete the system dependencies. The package manager checks existing packages; there is no need to uninstall and reinstall them:

```sh
sudo pacman -Syu --needed git curl ca-certificates xz file base-devel cmake pkgconf \
  openssl webkit2gtk-4.1 gtk3 appmenu-gtk-module libappindicator-gtk3 \
  librsvg xdotool protobuf
```

**2. Check Rust:** load an existing installation first. Skip the installer if `rustc` and `cargo` work. Update an existing rustup toolchain with `rustup update stable` and `rustup default stable`.

```sh
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
rustup --version
rustc --version
cargo --version
```

Install only if Rust is still missing after loading its environment:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
```

**3. Check Node.js/npm and nvm:** if Node.js is already 22.12+ and npm works, skip this group's installation commands. This also loads an existing nvm:

```sh
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh"; fi
node -v
npm -v
command -v nvm
```

For missing or older Node.js, use [nvm](https://github.com/nvm-sh/nvm#installing-and-updating) to install 24 LTS. If `command -v nvm` prints `nvm`, skip the first line below:

```sh
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
```

Repeat `node -v` and `npm -v` to confirm the environment, then download, build and run Akagi:

```sh
git clone https://github.com/Etakind/Akagi-web.git
cd Akagi-web
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --locked --release --features custom-protocol
./target/release/akagi
```

</details>

For development mode and packaging, see the [development guide](docs/BUILDING.md).

Based on [shinkuan/Akagi](https://github.com/shinkuan/Akagi/tree/v3). License and third-party notices: [LICENSE.txt](LICENSE.txt), [NOTICE](NOTICE).
