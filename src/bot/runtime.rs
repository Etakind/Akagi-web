//! Python interpreter + `uv` locator and per-bot venv sync.
//!
//! Two modes:
//!
//! - **Bundled**: `runtime/python/<triple>/...` and `runtime/uv/<triple>/uv`
//!   ship next to the binary in the portable zip distribution. The locator
//!   checks the exe-adjacent layout first; the Tauri-managed
//!   `app.path().resource_dir()` is checked as a secondary fallback so
//!   `cargo run` from a checkout (and any future Tauri-bundled target) keeps
//!   working. Zero Python install required for end users.
//! - **System**: `python3` and `uv` are looked up on `PATH` via the `which`
//!   crate. Used during development (`cargo run` from a checkout without a
//!   populated `runtime/`) and as a graceful fallback if the bundled
//!   binaries are missing.
//!
//! Per-bot venvs live under `<bot_dir>/.akagi/venv` so they don't clash with
//! a developer's own `.venv` if they happen to keep one in the bot folder.
//! `uv sync` is run on demand and skipped via a stamp file when neither
//! `pyproject.toml` nor `uv.lock` have changed since the last successful
//! sync.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::process::Command;
use tracing::{info, warn};

const STAMP_FILE: &str = "synced.stamp";
const VENV_DIR: &str = "venv";
const AKAGI_DIR: &str = ".akagi";

/// Origin of the python + uv binaries this runtime points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// Bundled python-build-standalone + uv from the Tauri resource dir.
    Bundled,
    /// `python3` + `uv` discovered on `PATH`. Dev-mode fallback.
    System,
}

#[derive(Debug, Clone)]
pub struct PythonRuntime {
    /// Interpreter that uv uses to seed venvs (`UV_PYTHON`).
    python: PathBuf,
    /// `uv` binary.
    uv: PathBuf,
    mode: RuntimeMode,
}

impl PythonRuntime {
    /// Direct construction. Tests use this; production code uses `locate`.
    pub fn from_paths(python: PathBuf, uv: PathBuf, mode: RuntimeMode) -> Self {
        Self { python, uv, mode }
    }

    /// Locate bundled binaries first, then fall back to system PATH.
    ///
    /// Lookup order:
    /// 1. **Exe-adjacent**: `<exe_parent>/runtime/{python,uv}/<triple>/...` —
    ///    this is the portable zip layout users get from Releases.
    /// 2. **Resource dir**: the Tauri-managed `app.path().resource_dir()` —
    ///    secondary fallback so `cargo run` and any future Tauri-bundled
    ///    install (`/usr/lib/akagi/`, `.app/Contents/Resources/`) keep
    ///    working.
    /// 3. **System PATH**: `python3` and `uv` resolved via the `which` crate.
    ///
    /// Pass `None` for `resource_dir` outside Tauri (tests, CLI tools).
    pub fn locate(resource_dir: Option<&Path>) -> Result<Self> {
        if let Some(rt) = try_bundled_exe_adjacent() {
            return Ok(rt);
        }
        if let Some(rd) = resource_dir {
            if let Some(rt) = try_bundled(rd) {
                return Ok(rt);
            }
        }
        try_system().context("no bundled runtime found and neither `python3` nor `uv` is on PATH")
    }

    pub fn python(&self) -> &Path {
        &self.python
    }

    pub fn uv(&self) -> &Path {
        &self.uv
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    /// Run `uv sync` against the bot's `pyproject.toml` if the on-disk
    /// signature has changed since the last successful sync. Idempotent.
    pub async fn ensure_synced(&self, bot_dir: &Path) -> Result<()> {
        let pyproject = bot_dir.join("pyproject.toml");
        if !pyproject.is_file() {
            bail!(
                "pyproject.toml missing in {} — every Akagi bot must declare its deps",
                bot_dir.display()
            );
        }
        let lock = bot_dir.join("uv.lock");
        let venv = bot_dir.join(AKAGI_DIR).join(VENV_DIR);
        let stamp_path = bot_dir.join(AKAGI_DIR).join(STAMP_FILE);

        let current = current_signature(&pyproject, &lock)?;
        if venv.is_dir() && stamp_matches(&stamp_path, &current).await? {
            if venv_python_alive(&venv) && venv_home_exists(&venv) {
                return Ok(());
            }
            // Stamp says the deps are in sync, but the venv's baked-in
            // absolute pointers to the base interpreter are stale. Two
            // ways this happens:
            //   1. AppImage: each launch creates a fresh
            //      `/tmp/.mount_Akagi_<rand>/` mount, so the `bin/python`
            //      symlink built under a previous mount now dangles
            //      (`venv_python_alive` is false).
            //   2. The user moved/renamed the whole Akagi folder, so the
            //      `pyvenv.cfg.home` directory uv baked in at sync time is
            //      gone (`venv_home_exists` is false). On Unix this also
            //      dangles the symlink, but on Windows `Scripts/python.exe`
            //      is a real copy that survives the move, so the missing
            //      `home` dir is the only on-disk tell — without it we'd
            //      hand a dead interpreter to the runner and the first
            //      stdin write dies with `os error 232` ("The pipe is
            //      being closed").
            // The standalone python + installed wheels are binary-identical
            // regardless of location, so we repoint the venv to the current
            // python without paying for a full re-sync (which would
            // otherwise re-run on every single launch under AppImage).
            match repoint_venv(&venv, &self.python).await {
                Ok(()) => {
                    info!(
                        bot = %bot_dir.display(),
                        python = %self.python.display(),
                        "repointed venv to current python (AppImage mount changed)"
                    );
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        bot = %bot_dir.display(),
                        "venv repoint failed ({e:#}); wiping for full re-sync"
                    );
                    reset_sync_state(bot_dir).await;
                }
            }
        }

        if let Some(parent) = stamp_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create {}", parent.display()))?;
        }

        let mut sync_cmd = Command::new(&self.uv);
        sync_cmd
            .arg("sync")
            .arg("--project")
            .arg(bot_dir)
            .env("UV_PYTHON", &self.python)
            .env("UV_PROJECT_ENVIRONMENT", &venv);
        scrub_python_env(&mut sync_cmd);
        let status = sync_cmd
            .status()
            .await
            .with_context(|| format!("spawn `uv` at {}", self.uv.display()))?;
        if !status.success() {
            bail!("uv sync failed in {} ({status})", bot_dir.display());
        }

        // `uv sync` creates (or refreshes) `uv.lock`, so the `current`
        // signature captured before the sync is stale the moment the bot
        // shipped no lockfile — the common case for a hand-written bot. Writing
        // that stale signature would make the very next `is_synced` check fail
        // (it re-stats the now-present `uv.lock` and sees a mismatch), leaving a
        // freshly-synced bot reading as not-ready and its activation toggle
        // disabled. Re-snapshot the on-disk pyproject + lock so the stamp
        // matches what `is_synced` computes next.
        let synced = current_signature(&pyproject, &lock)?;
        write_stamp(&stamp_path, &synced).await?;
        Ok(())
    }

    /// Build a `tokio::process::Command` that runs the venv's python with
    /// the given args, with `current_dir` set to the bot directory so
    /// `bot.py` can resolve relative paths.
    pub fn command_for(&self, bot_dir: &Path, args: &[&str]) -> Command {
        let py = venv_python(&bot_dir.join(AKAGI_DIR).join(VENV_DIR));
        let mut cmd = Command::new(py);
        cmd.current_dir(bot_dir).args(args);
        scrub_python_env(&mut cmd);
        cmd
    }
}

/// Drop Python env vars that the AppImage runtime (and some AUR
/// wrappers) export for *Akagi's* host process. Inherited as-is they
/// override the bot venv's `pyvenv.cfg`, so the venv python looks for
/// its stdlib under the AppImage mount and dies with
/// `Fatal Python error: init_fs_encoding: failed to get the Python
/// codec of the filesystem encoding / No module named 'encodings'`
/// before the bot ever reads stdin — the next `react()` then surfaces
/// as `Broken pipe (os error 32)`. Bundled python-build-standalone
/// (used both for `uv sync` and for the venv it seeds) is relocatable
/// and resolves its stdlib via `sys._base_executable`, so removing
/// these is strictly safer than inheriting them.
fn scrub_python_env(cmd: &mut Command) {
    cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
}

/// True when `bot_dir`'s Python environment is already installed — i.e.
/// `ensure_synced` would short-circuit instead of running `uv sync`. A bot
/// with no `pyproject.toml` has nothing to install and is always ready.
///
/// Inspects only the on-disk stamp + venv (no `uv`, no async), so it's cheap
/// enough to call while listing bots or gating activation. Deliberately does
/// NOT require `venv_python_alive`: a dangling venv symlink (the AppImage
/// mount-changed case) is repaired by a cheap repoint inside `ensure_synced`,
/// not a slow full re-sync, so it shouldn't block a bot from being activated.
///
/// It DOES, however, report not-installed for a venv that survived a
/// folder move but can only be repaired by a full re-sync (see
/// [`needs_out_of_band_resync`]). That keeps the readiness signal honest:
/// the UI re-offers "Install environment", `set_active_bot` won't (re)gate
/// it active, and game-start skips it instead of stalling a live game on an
/// inline `uv sync`.
pub fn is_synced(bot_dir: &Path) -> bool {
    let pyproject = bot_dir.join("pyproject.toml");
    if !pyproject.is_file() {
        return true;
    }
    let lock = bot_dir.join("uv.lock");
    let venv = bot_dir.join(AKAGI_DIR).join(VENV_DIR);
    let stamp_path = bot_dir.join(AKAGI_DIR).join(STAMP_FILE);
    let Ok(current) = current_signature(&pyproject, &lock) else {
        return false;
    };
    if !venv.is_dir() {
        return false;
    }
    if needs_out_of_band_resync(bot_dir) {
        return false;
    }
    match std::fs::read_to_string(&stamp_path) {
        Ok(saved) => saved.trim() == current,
        Err(_) => false,
    }
}

/// True when the bot's venv is present but in a state that game-start
/// **cannot cheaply repair**, so letting `ensure_synced` run inline there
/// would do a slow `uv sync` that stalls the live game while the bot misses
/// its turns — the historical game-start-timeout hazard that `set_active_bot`
/// guards against by requiring a pre-installed env.
///
/// The one production trigger is a moved/renamed Akagi folder whose venv
/// interpreter file *survived* the move but whose baked base-python `home`
/// is gone. That happens on **Windows**, where the venv's
/// `Scripts/python.exe` is a real trampoline copy with the base path
/// embedded in the binary — `ensure_synced` can't repoint it in place and
/// must re-sync from scratch. On **Unix** the same move dangles the
/// `bin/python` symlink instead (interpreter not "alive"), which
/// `ensure_synced` repoints cheaply, so this returns false and game-start
/// proceeds as before. A genuinely absent venv also returns false — the
/// cold first install is a separate, out-of-band concern.
///
/// The check is the *semantic* condition (interpreter alive AND base `home`
/// gone), not an OS `cfg`, so it's testable on any platform and naturally
/// fires only for the move shape that actually needs a re-sync.
pub fn needs_out_of_band_resync(bot_dir: &Path) -> bool {
    let venv = bot_dir.join(AKAGI_DIR).join(VENV_DIR);
    venv.is_dir() && venv_python_alive(&venv) && !venv_home_exists(&venv)
}

/// Wipe the stamp file and the venv so the next `ensure_synced` runs from
/// scratch. Used by the user-triggered "Reinstall environment" path —
/// stamp-only invalidation lets `uv sync` re-run, but uv's sync is
/// incremental against an existing venv, so a corrupted venv (the actual
/// failure mode) can survive a stamp-only retry. Wiping the venv forces a
/// clean seed. Errors are swallowed — missing files are the expected case.
pub async fn reset_sync_state(bot_dir: &Path) {
    let akagi = bot_dir.join(AKAGI_DIR);
    let _ = tokio::fs::remove_file(akagi.join(STAMP_FILE)).await;
    let _ = tokio::fs::remove_dir_all(akagi.join(VENV_DIR)).await;
}

/// Look for the bundled runtime in the directory containing the running
/// executable. This is the layout shipped by the portable zip
/// distribution: `<exe_parent>/runtime/{python,uv}/<triple>/...`.
///
/// On Linux/macOS, `tauri::path::resource_dir()` does not return
/// exe-adjacent paths in a portable layout — it tries Tauri-bundled
/// install locations like `/usr/lib/akagi/` and returns `Err` or a
/// non-existent path otherwise. Checking exe-adjacent here ensures the
/// portable zip works without depending on Tauri's resource resolution.
fn try_bundled_exe_adjacent() -> Option<PythonRuntime> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    try_bundled(exe_dir)
}

fn try_bundled(resource_dir: &Path) -> Option<PythonRuntime> {
    let triple = host_triple();
    let py = resource_dir
        .join("runtime")
        .join("python")
        .join(triple)
        .join(if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3"
        });
    let uv = resource_dir
        .join("runtime")
        .join("uv")
        .join(triple)
        .join(if cfg!(windows) { "uv.exe" } else { "uv" });
    if py.is_file() && uv.is_file() {
        Some(PythonRuntime::from_paths(py, uv, RuntimeMode::Bundled))
    } else {
        None
    }
}

fn try_system() -> Result<PythonRuntime> {
    let python = which::which("python3")
        .or_else(|_| which::which("python"))
        .context("locate python3/python on PATH")?;
    let uv = which::which("uv").context("locate uv on PATH")?;
    Ok(PythonRuntime::from_paths(python, uv, RuntimeMode::System))
}

/// The venv's interpreter path for this platform's layout.
pub(crate) fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// True when the venv's python interpreter resolves to an existing
/// file. `metadata` follows symlinks, so a dangling symlink (the
/// AppImage mount-changed case) returns Err and we report dead.
fn venv_python_alive(venv: &Path) -> bool {
    std::fs::metadata(venv_python(venv))
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// Pull the `home = …` value out of a `pyvenv.cfg` body, if present.
/// Matches the `home` key exactly (not `homepage` etc.) and tolerates
/// the surrounding whitespace uv writes (`home = /path`).
fn home_dir_from_cfg(cfg: &str) -> Option<String> {
    for line in cfg.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("home") {
            if let Some(value) = rest.trim_start().strip_prefix('=') {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// True when the venv's recorded base-python `home` directory still
/// exists on disk. `uv` bakes the *absolute* path of the base
/// interpreter into `pyvenv.cfg` (`home = …`) at sync time. When the
/// Akagi folder is moved or renamed, that absolute path is gone: the
/// venv python can no longer find its base interpreter/stdlib and dies
/// at startup, which surfaces downstream as a broken pipe on the first
/// stdin write — `os error 232` ("The pipe is being closed") on Windows,
/// `Broken pipe (os error 32)` on Unix.
///
/// On Unix a moved venv is already caught by `venv_python_alive` (the
/// `bin/python` symlink dangles), but on Windows `Scripts/python.exe` is
/// a real copy that survives the move, so the missing `home` directory
/// is the only on-disk tell. Checking it lets `ensure_synced` fall
/// through to repair (repoint, or re-sync) instead of handing a dead
/// interpreter to the runner.
///
/// Returns `true` (assume current) when there is no `pyvenv.cfg` or it
/// carries no `home` key — there is nothing to invalidate on, and
/// `venv_python_alive` stays the backstop. Note a *copy* of the folder
/// (original left in place) keeps the old `home` dir alive, so the venv
/// still works and we correctly leave it untouched; only a true
/// move/rename trips this.
fn venv_home_exists(venv: &Path) -> bool {
    let Ok(cfg) = std::fs::read_to_string(venv.join("pyvenv.cfg")) else {
        return true;
    };
    match home_dir_from_cfg(&cfg) {
        Some(home) => Path::new(&home).is_dir(),
        None => true,
    }
}

/// Repoint a venv at `new_python` without re-running `uv sync`. Used
/// when the venv was sync'd under a previous AppImage mount whose
/// `/tmp/.mount_Akagi_<rand>/` path is gone. Rewrites the `bin/python`
/// symlink and the `home = …` line in `pyvenv.cfg`; everything else in
/// the venv (site-packages, .pyc) stays valid because
/// python-build-standalone is binary-identical across launches.
///
/// Unix-only — the AppImage failure mode doesn't exist on Windows
/// (resource dir is stable there).
#[cfg(unix)]
async fn repoint_venv(venv: &Path, new_python: &Path) -> Result<()> {
    let target = tokio::fs::canonicalize(new_python)
        .await
        .with_context(|| format!("canonicalize {}", new_python.display()))?;
    let new_home = target
        .parent()
        .with_context(|| format!("python {} has no parent dir", target.display()))?
        .to_path_buf();

    let py_link = venv_python(venv);
    // Use symlink_metadata so a dangling symlink is still detected and
    // removed (plain metadata would error and skip the unlink).
    if std::fs::symlink_metadata(&py_link).is_ok() {
        tokio::fs::remove_file(&py_link)
            .await
            .with_context(|| format!("remove stale {}", py_link.display()))?;
    }
    tokio::fs::symlink(&target, &py_link)
        .await
        .with_context(|| format!("symlink {} -> {}", py_link.display(), target.display()))?;

    let cfg_path = venv.join("pyvenv.cfg");
    if cfg_path.is_file() {
        let cfg = tokio::fs::read_to_string(&cfg_path)
            .await
            .with_context(|| format!("read {}", cfg_path.display()))?;
        let new_home_str = new_home.display().to_string();
        let mut rewrote = false;
        let mut out = String::with_capacity(cfg.len());
        for line in cfg.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("home") && trimmed.split_once('=').is_some() {
                out.push_str(&format!("home = {new_home_str}"));
                rewrote = true;
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        if rewrote {
            tokio::fs::write(&cfg_path, out)
                .await
                .with_context(|| format!("write {}", cfg_path.display()))?;
        }
    }

    if !venv_python_alive(venv) {
        bail!("venv python still dead after repoint");
    }
    Ok(())
}

#[cfg(not(unix))]
async fn repoint_venv(_venv: &Path, _new_python: &Path) -> Result<()> {
    bail!("venv repoint not supported on this platform")
}

/// Build target triple — used to pick the right bundled runtime.
fn host_triple() -> &'static str {
    // `cargo` doesn't expose the runtime target triple, only the build-time
    // one — which is exactly what we want here, since the bundled binary
    // matches what we compiled for.
    env!(
        "TARGET_TRIPLE",
        "TARGET_TRIPLE not set; build.rs should pass it"
    )
}

/// `mtime:size` for `pyproject.toml` plus the same for `uv.lock` if it
/// exists. Cheap to compute (no file read) and stable across reboots
/// (mtime is filesystem-persistent). Granularity is 1 s, which is fine —
/// `uv sync` writes its lockfile with the current second.
fn current_signature(pyproject: &Path, lock: &Path) -> Result<String> {
    let proj = file_meta(pyproject)?;
    let lock_part = if lock.exists() {
        let l = file_meta(lock)?;
        format!("{}:{}", l.0, l.1)
    } else {
        "0:0".into()
    };
    Ok(format!("v1|{}:{}|{}", proj.0, proj.1, lock_part))
}

fn file_meta(p: &Path) -> Result<(u64, u64)> {
    let m = std::fs::metadata(p).with_context(|| format!("stat {}", p.display()))?;
    let mtime = m
        .modified()
        .with_context(|| format!("mtime {}", p.display()))?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok((mtime, m.len()))
}

async fn stamp_matches(stamp_path: &Path, current: &str) -> Result<bool> {
    match tokio::fs::read_to_string(stamp_path).await {
        Ok(saved) => Ok(saved.trim() == current),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("read {}", stamp_path.display())),
    }
}

async fn write_stamp(stamp_path: &Path, sig: &str) -> Result<()> {
    tokio::fs::write(stamp_path, sig)
        .await
        .with_context(|| format!("write {}", stamp_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn dummy_runtime() -> PythonRuntime {
        PythonRuntime::from_paths(
            PathBuf::from("/dev/null/python"),
            PathBuf::from("/dev/null/uv"),
            RuntimeMode::System,
        )
    }

    #[tokio::test]
    async fn ensure_synced_bails_when_pyproject_missing() {
        let tmp = TempDir::new().unwrap();
        let rt = dummy_runtime();
        let err = rt.ensure_synced(tmp.path()).await.unwrap_err();
        assert!(
            err.to_string().contains("pyproject.toml missing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn signature_changes_when_pyproject_changes() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        let lock = tmp.path().join("uv.lock");
        write(&py, "[project]\nname='a'\n");
        let s1 = current_signature(&py, &lock).unwrap();

        // Bump mtime by sleeping 1.1s + writing different content. mtime
        // granularity on most filesystems is 1s, so we need to clear at
        // least one whole second.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&py, "[project]\nname='b'\n");
        let s2 = current_signature(&py, &lock).unwrap();
        assert_ne!(s1, s2, "signature must change after pyproject edit");
    }

    #[test]
    fn signature_includes_lock_when_present() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        let lock = tmp.path().join("uv.lock");
        write(&py, "[project]\nname='a'\n");

        let no_lock = current_signature(&py, &lock).unwrap();
        write(&lock, "version = 1\n");
        let with_lock = current_signature(&py, &lock).unwrap();
        assert_ne!(no_lock, with_lock);
    }

    #[tokio::test]
    async fn stamp_round_trip() {
        let tmp = TempDir::new().unwrap();
        let stamp = tmp.path().join(AKAGI_DIR).join(STAMP_FILE);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();

        assert!(!stamp_matches(&stamp, "v1|abc").await.unwrap());
        write_stamp(&stamp, "v1|abc").await.unwrap();
        assert!(stamp_matches(&stamp, "v1|abc").await.unwrap());
        assert!(!stamp_matches(&stamp, "v1|xyz").await.unwrap());
    }

    #[test]
    fn venv_python_path_per_platform() {
        let venv = Path::new("/foo/.akagi/venv");
        let p = venv_python(venv);
        if cfg!(windows) {
            assert!(p.ends_with("Scripts/python.exe") || p.ends_with("Scripts\\python.exe"));
        } else {
            assert!(p.ends_with("bin/python"));
        }
    }

    #[test]
    fn try_bundled_returns_none_when_runtime_dir_empty() {
        let tmp = TempDir::new().unwrap();
        assert!(try_bundled(tmp.path()).is_none());
    }

    /// Regression: portable zip relies on `try_bundled_exe_adjacent` to
    /// find `<exe_parent>/runtime/...` because Tauri's `resource_dir()`
    /// doesn't return exe-adjacent on Linux/macOS in a portable layout.
    /// In the test runner the binary lives in `target/<profile>/deps/`
    /// with no `runtime/` next to it, so this must return `None` (and
    /// must not panic on the optional chain).
    #[test]
    fn try_bundled_exe_adjacent_returns_none_when_runtime_missing() {
        assert!(try_bundled_exe_adjacent().is_none());
    }

    /// Regression: AppImage runtimes export `PYTHONHOME` / `PYTHONPATH`
    /// for Akagi's host process. If we let those leak into the bot
    /// venv's python, the venv crashes at startup with
    /// `init_fs_encoding ... No module named 'encodings'` and the next
    /// `react()` writes hit a broken pipe (manager.rs surfaces this as
    /// `bot react failed: write events to bot stdin: Broken pipe`). The
    /// `command_for` builder must explicitly remove them so the venv
    /// python falls back to its `pyvenv.cfg`-based stdlib resolution.
    #[test]
    fn command_for_strips_pythonhome_and_pythonpath() {
        use std::ffi::OsStr;
        let rt = dummy_runtime();
        let tmp = TempDir::new().unwrap();
        let cmd = rt.command_for(tmp.path(), &["bot.py"]);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.as_std().get_envs().collect();
        assert!(
            envs.iter()
                .any(|(k, v)| *k == OsStr::new("PYTHONHOME") && v.is_none()),
            "PYTHONHOME must be removed (got envs={envs:?})"
        );
        assert!(
            envs.iter()
                .any(|(k, v)| *k == OsStr::new("PYTHONPATH") && v.is_none()),
            "PYTHONPATH must be removed (got envs={envs:?})"
        );
    }

    /// Regression: under AppImage, every launch creates a new
    /// `/tmp/.mount_Akagi_<rand>/` mount, and uv bakes that absolute
    /// path into the venv at sync time. On the next launch the venv's
    /// `bin/python` symlink target is gone and `cmd.spawn()` returns
    /// ENOENT, surfacing as `spawn bot mortal: No such file or
    /// directory` from `runner.rs`. `repoint_venv` must rewrite the
    /// symlink and `pyvenv.cfg` `home =` line so the venv works again
    /// without re-running uv sync (which would otherwise re-run on
    /// every launch and cost minutes).
    #[cfg(unix)]
    #[tokio::test]
    async fn repoint_venv_rewrites_symlink_and_pyvenv_cfg() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join(AKAGI_DIR).join(VENV_DIR);
        let bin = venv.join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        // Stale mount path — neither file exists. This mirrors what an
        // AppImage second-launch venv looks like on disk.
        let stale = tmp.path().join("mount_OLD/python3");
        std::os::unix::fs::symlink(&stale, bin.join("python")).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            format!(
                "home = {}\nimplementation = CPython\nversion_info = 3.12.13\n",
                stale.parent().unwrap().display()
            ),
        )
        .unwrap();
        assert!(!venv_python_alive(&venv), "stale venv must read as dead");

        // Fresh mount: real python that does exist.
        let fresh_dir = tmp.path().join("mount_NEW/bin");
        std::fs::create_dir_all(&fresh_dir).unwrap();
        let fresh_python = fresh_dir.join("python3");
        std::fs::write(&fresh_python, b"#!/bin/sh\nexit 0\n").unwrap();

        repoint_venv(&venv, &fresh_python).await.unwrap();

        assert!(
            venv_python_alive(&venv),
            "venv python must resolve after repoint"
        );
        let new_link = std::fs::read_link(bin.join("python")).unwrap();
        assert_eq!(
            new_link,
            std::fs::canonicalize(&fresh_python).unwrap(),
            "symlink must point at the canonical fresh python"
        );
        let cfg = std::fs::read_to_string(venv.join("pyvenv.cfg")).unwrap();
        assert!(
            cfg.contains(&format!(
                "home = {}",
                std::fs::canonicalize(&fresh_dir).unwrap().display()
            )),
            "pyvenv.cfg `home` must be rewritten to the fresh bin dir, got:\n{cfg}"
        );
        assert!(
            !cfg.contains("mount_OLD"),
            "pyvenv.cfg must not retain the stale mount path, got:\n{cfg}"
        );
    }

    #[test]
    fn home_dir_from_cfg_extracts_home() {
        let cfg = "home = /opt/py/bin\nimplementation = CPython\nversion_info = 3.12.13\n";
        assert_eq!(home_dir_from_cfg(cfg).as_deref(), Some("/opt/py/bin"));
        // Tolerate the no-space form uv never writes but is still legal.
        assert_eq!(home_dir_from_cfg("home=/x").as_deref(), Some("/x"));
    }

    #[test]
    fn home_dir_from_cfg_ignores_lookalike_keys_and_missing_home() {
        // `homepage`/`home_dir` must not be mistaken for the `home` key.
        assert_eq!(home_dir_from_cfg("homepage = /x\nhome_dir = /y\n"), None);
        // No home line at all.
        assert_eq!(home_dir_from_cfg("implementation = CPython\n"), None);
    }

    #[test]
    fn venv_home_exists_tracks_the_baked_home_dir() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join(AKAGI_DIR).join(VENV_DIR);
        std::fs::create_dir_all(&venv).unwrap();

        // No pyvenv.cfg → nothing to invalidate on → assume current.
        assert!(venv_home_exists(&venv));

        // home points at a directory that exists → current.
        let live_home = tmp.path().join("runtime/python/bin");
        std::fs::create_dir_all(&live_home).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            format!("home = {}\nversion_info = 3.12.13\n", live_home.display()),
        )
        .unwrap();
        assert!(venv_home_exists(&venv));

        // home points at a vanished directory (folder was moved) → stale.
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = /no/such/old/runtime/python/bin\n",
        )
        .unwrap();
        assert!(!venv_home_exists(&venv));

        // pyvenv.cfg with no home key → assume current (alive check backstops).
        std::fs::write(venv.join("pyvenv.cfg"), "implementation = CPython\n").unwrap();
        assert!(venv_home_exists(&venv));
    }

    /// Regression (the Windows folder-move bug → `write newline: The pipe is
    /// being closed. (os error 232)`): a venv whose interpreter file still
    /// exists — on Windows `Scripts/python.exe` is a real copy that survives a
    /// move, unlike the Unix symlink which dangles — but whose pyvenv.cfg
    /// `home` directory is gone must NOT short-circuit as ready. The
    /// `ensure_synced` fast path now requires BOTH `venv_python_alive` AND
    /// `venv_home_exists`; before this fix only the former was checked, so the
    /// dead venv was handed to the runner and its first stdin write hit
    /// `os error 232`.
    #[test]
    fn venv_with_live_python_but_missing_home_reads_as_stale() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join(AKAGI_DIR).join(VENV_DIR);
        // Create the per-platform interpreter file so the test is correct on
        // both Unix (`bin/python`) and Windows (`Scripts/python.exe`).
        let py = venv_python(&venv);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, "").unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = /vanished/old/akagi/runtime/python/bin\n",
        )
        .unwrap();

        assert!(
            venv_python_alive(&venv),
            "interpreter file is present (Windows copy survives the move)"
        );
        assert!(
            !venv_home_exists(&venv),
            "but the baked `home` dir is gone → the venv is stale and must be repaired"
        );
    }

    /// Regression: end-to-end, `ensure_synced` must repair (not silently
    /// accept) a moved venv whose python is alive but whose `home` is stale.
    /// On Unix the repair is an in-place repoint; this asserts the observable
    /// outcome — afterwards the venv's `home` resolves again. (On Windows the
    /// same detection routes to a full re-sync, since the base path is baked
    /// into the trampoline `.exe` and can't be edited in place.)
    #[cfg(unix)]
    #[tokio::test]
    async fn ensure_synced_repairs_venv_with_live_python_but_stale_home() {
        let tmp = TempDir::new().unwrap();
        let bot = tmp.path();
        write(&bot.join("pyproject.toml"), "[project]\nname='a'\n");

        // The current (post-move) bundled interpreter, at a valid path.
        let py_dir = tmp.path().join("runtime/bin");
        std::fs::create_dir_all(&py_dir).unwrap();
        let python = py_dir.join("python3");
        std::fs::write(&python, "").unwrap();

        // Pre-seed the "moved" venv: bin/python present, but home vanished,
        // and a stamp that matches so the short-circuit branch is reached.
        let akagi = bot.join(AKAGI_DIR);
        let venv = akagi.join(VENV_DIR);
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::fs::write(venv.join("bin").join("python"), "").unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = /no/such/old/runtime/bin\nversion_info = 3.12.13\n",
        )
        .unwrap();
        let sig = current_signature(&bot.join("pyproject.toml"), &bot.join("uv.lock")).unwrap();
        std::fs::write(akagi.join(STAMP_FILE), &sig).unwrap();
        assert!(
            venv_python_alive(&venv) && !venv_home_exists(&venv),
            "precondition: the os-error-232 state — live python, dead home"
        );

        // uv is never invoked: the Unix repoint succeeds in place. A bogus
        // path proves we didn't fall through to a real sync.
        let rt =
            PythonRuntime::from_paths(python, PathBuf::from("/dev/null/uv"), RuntimeMode::System);
        rt.ensure_synced(bot).await.unwrap();

        assert!(
            venv_home_exists(&venv),
            "ensure_synced must repair the stale home in place"
        );
        assert!(venv_python_alive(&venv));
    }

    /// `needs_out_of_band_resync` must fire only for the move shape that
    /// truly can't be repaired cheaply at game-start: the interpreter file
    /// survived (Windows copy) but its baked base `home` is gone. A dangling
    /// symlink (the Unix move/AppImage shape) is cheaply repointable, and an
    /// absent or healthy venv needs nothing — all three must return false so
    /// game-start isn't needlessly downgraded to analysis-only.
    #[test]
    fn needs_out_of_band_resync_only_for_alive_interp_with_dead_home() {
        let tmp = TempDir::new().unwrap();
        let bot = tmp.path();
        let venv = bot.join(AKAGI_DIR).join(VENV_DIR);

        // No venv at all (cold install) → not a re-sync-needed state.
        assert!(!needs_out_of_band_resync(bot));

        // Interpreter present + home dir present (healthy / warm) → false.
        let py = venv_python(&venv);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, "").unwrap();
        let live_home = tmp.path().join("runtime/python/bin");
        std::fs::create_dir_all(&live_home).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            format!("home = {}\n", live_home.display()),
        )
        .unwrap();
        assert!(!needs_out_of_band_resync(bot));

        // Interpreter present but home dir vanished (the Windows folder-move
        // shape) → the one true case.
        std::fs::write(venv.join("pyvenv.cfg"), "home = /no/such/old/bin\n").unwrap();
        assert!(needs_out_of_band_resync(bot));
    }

    /// Unix move shape: the interpreter is a dangling symlink, so it's NOT
    /// "alive" — `ensure_synced` repoints it cheaply and `needs_out_of_band_resync`
    /// must stay false (no analysis-only downgrade, no out-of-band reinstall).
    #[cfg(unix)]
    #[test]
    fn needs_out_of_band_resync_false_for_dangling_symlink() {
        let tmp = TempDir::new().unwrap();
        let bot = tmp.path();
        let venv = bot.join(AKAGI_DIR).join(VENV_DIR);
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::os::unix::fs::symlink("/no/such/old/bin/python3", venv.join("bin").join("python"))
            .unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /no/such/old/bin\n").unwrap();
        assert!(!venv_python_alive(&venv), "dangling symlink is not alive");
        assert!(!needs_out_of_band_resync(bot));
    }

    /// Regression: a moved venv that needs a full re-sync must read as
    /// not-installed so the UI re-offers "Install environment" and game-start
    /// skips it — even though its stamp still matches (a move preserves the
    /// pyproject/lock mtimes the stamp is keyed on).
    #[test]
    fn is_synced_false_for_moved_venv_needing_resync() {
        let tmp = TempDir::new().unwrap();
        let bot = tmp.path();
        let py = bot.join("pyproject.toml");
        write(&py, "[project]\nname='a'\n");
        let venv = bot.join(AKAGI_DIR).join(VENV_DIR);
        // Alive interpreter (survived the move) + vanished home.
        let interp = venv_python(&venv);
        std::fs::create_dir_all(interp.parent().unwrap()).unwrap();
        std::fs::write(&interp, "").unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /vanished/old/bin\n").unwrap();
        // Stamp matches current signature — the move kept the mtimes.
        let sig = current_signature(&py, &bot.join("uv.lock")).unwrap();
        std::fs::write(bot.join(AKAGI_DIR).join(STAMP_FILE), &sig).unwrap();

        assert!(
            needs_out_of_band_resync(bot),
            "precondition: this venv needs a re-sync"
        );
        assert!(
            !is_synced(bot),
            "a moved venv needing a re-sync must not read as installed"
        );
    }

    #[test]
    fn is_synced_true_without_pyproject() {
        let tmp = TempDir::new().unwrap();
        // No pyproject.toml → nothing to install → always ready.
        assert!(is_synced(tmp.path()));
    }

    #[test]
    fn is_synced_false_when_venv_and_stamp_missing() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join("pyproject.toml"), "[project]\nname='a'\n");
        // Has deps but no venv / stamp → not installed yet.
        assert!(!is_synced(tmp.path()));
    }

    #[test]
    fn is_synced_tracks_stamp_signature() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        write(&py, "[project]\nname='a'\n");
        let lock = tmp.path().join("uv.lock");
        let sig = current_signature(&py, &lock).unwrap();

        let akagi = tmp.path().join(AKAGI_DIR);
        std::fs::create_dir_all(akagi.join(VENV_DIR)).unwrap();
        std::fs::write(akagi.join(STAMP_FILE), &sig).unwrap();
        assert!(is_synced(tmp.path()), "matching stamp + venv → ready");

        // Stale stamp (e.g. pyproject changed since last sync) → not ready.
        std::fs::write(akagi.join(STAMP_FILE), "v1|stale").unwrap();
        assert!(!is_synced(tmp.path()));
    }

    #[test]
    fn is_synced_false_when_stamp_present_but_venv_missing() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("pyproject.toml");
        write(&py, "[project]\nname='a'\n");
        let sig = current_signature(&py, &tmp.path().join("uv.lock")).unwrap();
        let akagi = tmp.path().join(AKAGI_DIR);
        std::fs::create_dir_all(&akagi).unwrap();
        std::fs::write(akagi.join(STAMP_FILE), &sig).unwrap();
        // Stamp matches but the venv dir is gone → must re-sync.
        assert!(!is_synced(tmp.path()));
    }

    #[tokio::test]
    async fn reset_sync_state_removes_stamp_and_venv() {
        let tmp = TempDir::new().unwrap();
        let akagi = tmp.path().join(AKAGI_DIR);
        let venv = akagi.join(VENV_DIR);
        let stamp = akagi.join(STAMP_FILE);
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::fs::write(stamp, "v1|abc").unwrap();
        std::fs::write(venv.join("bin").join("python"), "").unwrap();

        reset_sync_state(tmp.path()).await;

        assert!(!akagi.join(STAMP_FILE).exists(), "stamp should be removed");
        assert!(!akagi.join(VENV_DIR).exists(), "venv should be removed");
        // .akagi/ dir itself may stay — only the wipe targets are stamp + venv.
    }

    #[tokio::test]
    async fn reset_sync_state_is_silent_when_paths_missing() {
        let tmp = TempDir::new().unwrap();
        // No `.akagi/` dir at all — must not panic.
        reset_sync_state(tmp.path()).await;
    }

    /// Regression (issue #143): a bot dropped straight into the bots dir with a
    /// `pyproject.toml` but **no** `manifest.toml` must still go through the
    /// normal env-readiness transition — readiness/sync never depends on a
    /// manifest. This is the invariant the Bots-tab "Install environment"
    /// button relies on: it runs the same sync path for manifest-less bots,
    /// breaking the chicken-and-egg where activation needs a ready env but the
    /// only manual sync trigger (the drawer's "Reinstall environment" button)
    /// was reachable only via the manifest-gated Configure button.
    #[test]
    fn is_synced_is_manifest_independent() {
        let tmp = TempDir::new().unwrap();
        // A minimal locally-developed bot: entry point + deps, no manifest.
        write(&tmp.path().join("bot.py"), "print()\n");
        let py = tmp.path().join("pyproject.toml");
        write(&py, "[project]\nname='a'\n\n[tool.uv]\npackage = false\n");
        assert!(
            !tmp.path().join("manifest.toml").exists(),
            "this fixture deliberately has no manifest"
        );

        // Before sync: deps declared but no venv/stamp → not ready (toggle
        // blocked, install button shown).
        assert!(
            !is_synced(tmp.path()),
            "fresh dropped-in bot must read as not ready"
        );

        // After a sync seeds the venv + matching stamp → ready, with no
        // manifest anywhere in the picture.
        let sig = current_signature(&py, &tmp.path().join("uv.lock")).unwrap();
        let akagi = tmp.path().join(AKAGI_DIR);
        std::fs::create_dir_all(akagi.join(VENV_DIR)).unwrap();
        std::fs::write(akagi.join(STAMP_FILE), &sig).unwrap();
        assert!(
            is_synced(tmp.path()),
            "manifest-less bot must become ready after sync"
        );
    }

    /// Regression (issue #143): `uv sync` creates/refreshes `uv.lock`, so the
    /// stamp must be written from the POST-sync on-disk signature, not the
    /// pre-sync one. A bot that ships no lockfile (the common hand-written
    /// case) otherwise reads as not-ready immediately after a successful sync —
    /// its activation toggle stays disabled until a second sync. Uses a fake
    /// `uv` that reproduces the real side effects: it seeds the venv and writes
    /// a `uv.lock` into the project dir.
    #[cfg(unix)]
    #[tokio::test]
    async fn ensure_synced_marks_ready_when_uv_creates_lockfile() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let bot = tmp.path();
        write(&bot.join("pyproject.toml"), "[project]\nname='a'\n");
        // No uv.lock shipped — the (fake) uv will create it during sync.
        assert!(!bot.join("uv.lock").exists());

        let fake_uv = tmp.path().join("uv");
        std::fs::write(
            &fake_uv,
            "#!/bin/sh\n\
             mkdir -p \"$UV_PROJECT_ENVIRONMENT/bin\"\n\
             : > \"$UV_PROJECT_ENVIRONMENT/bin/python\"\n\
             proj=\"$(dirname \"$(dirname \"$UV_PROJECT_ENVIRONMENT\")\")\"\n\
             printf 'version = 1\\n' > \"$proj/uv.lock\"\n\
             exit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_uv, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fake_python = tmp.path().join("python");
        std::fs::write(&fake_python, "").unwrap();

        let rt = PythonRuntime::from_paths(fake_python, fake_uv, RuntimeMode::System);
        rt.ensure_synced(bot).await.unwrap();

        assert!(
            bot.join("uv.lock").exists(),
            "fake uv should have created the lockfile"
        );
        assert!(
            is_synced(bot),
            "a freshly-synced bot must read as ready even when uv created the lockfile"
        );
    }
}
