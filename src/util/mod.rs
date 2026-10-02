use std::path::{Path, PathBuf};
pub mod credentials;
pub mod private_fs;

/// True when running inside an AppImage runtime (read-only squashfs mount).
/// AppImage sets `$APPIMAGE` to the .AppImage path before exec.
pub fn is_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

/// `~/.config/akagi` (Linux) or platform equivalent. None if unresolvable.
/// All AppImage runtime paths (config, logs, bot dir, CA dir) anchor here.
pub fn user_config_root() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("akagi"))
}

/// Strip a leading `./` so `./logs` joins cleanly under a user dir.
pub fn strip_leading_dot(p: &Path) -> &Path {
    p.strip_prefix("./").unwrap_or(p)
}

/// `<user_config_root>/<name>` if available. Use this for runtime data
/// that must always live in a writable XDG-style location regardless of
/// AppImage / system-install / cwd. Unlike [`resolve_dir`], there is no
/// exe-dir-first fallback — callers like the Chromium profile and CfT
/// install dir explicitly want the user dir every time.
pub fn user_subdir(name: &str) -> Option<PathBuf> {
    user_config_root().map(|r| r.join(name))
}

/// Resolve a configured directory path with the standard fallback chain:
///
/// 1. Absolute path → used as-is
/// 2. `<exe_dir>/<configured>` if it exists
/// 3. `<cwd>/<configured>` if it exists
/// 4. Under AppImage: `<user_data_root>/<configured>` (writable XDG location).
/// 5. Otherwise return `<exe_dir>/<configured>` (preferred), falling back to
///    `<cwd>/<configured>` if the executable path is unavailable. The caller
///    is responsible for creating the directory.
pub fn resolve_dir(configured: &Path) -> PathBuf {
    resolve_dir_inner(configured, is_appimage(), user_config_root())
}

fn resolve_dir_inner(configured: &Path, appimage: bool, user_root: Option<PathBuf>) -> PathBuf {
    if configured.is_absolute() {
        return configured.to_path_buf();
    }

    let exe_candidate = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join(strip_leading_dot(configured))));

    if let Some(p) = &exe_candidate {
        if p.exists() {
            return p.clone();
        }
    }

    let cwd_candidate = configured.to_path_buf();
    if cwd_candidate.exists() {
        return cwd_candidate;
    }

    if appimage {
        if let Some(root) = user_root {
            return root.join(strip_leading_dot(configured));
        }
    }

    exe_candidate.unwrap_or(cwd_candidate)
}

/// Extension for [`std::process::Command`] that stops Windows from briefly
/// flashing a console window when the GUI app spawns a console helper
/// (`tasklist`, `taskkill`, `reg`, …). Chainable and a **no-op off Windows**:
///
/// ```ignore
/// Command::new("tasklist").args(["/FI", filter]).no_console_window().output()
/// ```
pub trait NoConsoleWindow {
    fn no_console_window(&mut self) -> &mut Self;
}

impl NoConsoleWindow for std::process::Command {
    fn no_console_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the child gets no console, so no window flashes.
            self.creation_flags(0x0800_0000);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_leading_dot_removes_dotslash() {
        assert_eq!(strip_leading_dot(Path::new("./logs")), Path::new("logs"));
        assert_eq!(strip_leading_dot(Path::new("logs")), Path::new("logs"));
        assert_eq!(strip_leading_dot(Path::new("./a/b")), Path::new("a/b"));
    }

    #[test]
    fn appimage_routes_relative_path_to_user_config() {
        // An existing ./logs intentionally wins over the AppImage fallback.
        // Use a unique absent relative path so a real local session cannot
        // change this test's premise. Do not mutate process-wide cwd.
        let fixture = tempfile::tempdir().unwrap();
        let relative = PathBuf::from(fixture.path().file_name().unwrap()).join("logs");
        assert!(!relative.exists());
        let user_root = fixture.path().join("config");
        let resolved = resolve_dir_inner(&relative, true, Some(user_root.clone()));
        assert_eq!(resolved, user_root.join(relative));
    }

    #[test]
    fn appimage_preserves_absolute_path() {
        let resolved = resolve_dir_inner(
            Path::new("/var/log/akagi"),
            true,
            Some(PathBuf::from("/home/u/.config/akagi")),
        );
        assert_eq!(resolved, PathBuf::from("/var/log/akagi"));
    }

    #[test]
    fn non_appimage_does_not_use_user_config_for_dirs() {
        // Non-appimage with a path that doesn't exist anywhere should NOT
        // fall back to user_root; it returns the exe-relative candidate.
        let user_root = PathBuf::from("/home/u/.config/akagi");
        let resolved = resolve_dir_inner(
            Path::new("./nonexistent-xyz"),
            false,
            Some(user_root.clone()),
        );
        assert!(!resolved.starts_with(&user_root));
    }

    #[test]
    fn relative_dot_path_does_not_leak_into_absolute() {
        // Regression: `./logs` joined with absolute exe-parent used to keep
        // the literal `./` component, producing paths like
        // `/home/.../akagi/./logs/...` that surfaced in the UI as broken.
        let resolved = resolve_dir_inner(Path::new("./logs"), false, None);
        let s = resolved.to_string_lossy();
        assert!(!s.contains("/./"), "got: {s}");
        assert!(!s.contains("\\.\\"), "got: {s}");
    }
}
