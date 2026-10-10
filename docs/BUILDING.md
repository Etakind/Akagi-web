# Development and packaging

For installation and everyday use, see the [README](../README.md).

The Cargo commands in the README embed the frontend into a release executable. Running it needs neither Node.js, Rust, protoc nor a development server. Keep custom-protocol: a plain Cargo build otherwise points to the development server. With build dependencies installed and from the repository root, development mode uses:

```sh
npm exec --prefix frontend -- tauri dev
```
In Windows PowerShell use npm.cmd. tauri dev starts Vite; it is separate from running the release executable directly.

Python 3.11+ is needed only for Python packaging/check scripts. Use brew install python on macOS, the [official Python installer](https://www.python.org/downloads/windows/) with PATH on Windows, or a distribution-provided Python 3.11+ on Linux. Check python3 --version first: Ubuntu 22.04’s default 3.10 is too old for these scripts. Install 3.11+ following the [official Unix instructions](https://docs.python.org/3/using/unix.html), and replace python3 below with python3.12 or your installed interpreter if needed. Linux DEB/RPM packaging also needs patchelf and rpm: apt install patchelf rpm on Ubuntu/Debian, dnf install patchelf rpm on Fedora, or pacman -S patchelf rpm-tools on Arch.

Exact triples and formats are in [build/targets.json](../build/targets.json). This is an Apple Silicon native packaging example; use x86_64-apple-darwin on Intel Mac and x86_64-pc-windows-msvc on Windows:

```sh
rustup target add aarch64-apple-darwin
npm exec --prefix frontend -- tauri build --no-bundle --target aarch64-apple-darwin
python3 scripts/package.py --target aarch64-apple-darwin
```
On Windows run scripts with py -3. On a matching Linux x86_64/ARM64 host, use its GNU triple, replace --no-bundle with --bundles deb,rpm, then run the same packaging script. Adding a Rust target does not install another OS’s SDK/linker or guarantee cross-compilation. Instructions for other platforms are checked against documentation and CI, rather than treated as installation tests on this machine.

System dependency references: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), [AWS-LC requirements](https://aws.github.io/aws-lc-rs/requirements/), [nvm](https://github.com/nvm-sh/nvm#installing-and-updating).
