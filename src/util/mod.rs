use std::path::{Path, PathBuf};
pub mod credentials;
pub mod private_fs;

pub fn user_config_root() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("akagi"))
}

/// Strip a leading `./` so `./logs` joins cleanly under a user dir.
pub fn strip_leading_dot(p: &Path) -> &Path {
    p.strip_prefix("./").unwrap_or(p)
}

pub fn user_subdir(name: &str) -> Option<PathBuf> {
    user_config_root().map(|r| r.join(name))
}

pub fn resolve_dir(configured: &Path) -> PathBuf {
    resolve_dir_inner(configured)
}

fn resolve_dir_inner(configured: &Path) -> PathBuf {
    // System package locations are read-only application resources. Runtime
    // data belongs to the user even when launched from /usr or /opt.
    if !configured.is_absolute()
        && std::env::current_exe()
            .ok()
            .is_some_and(|exe| exe.parent().is_some_and(|p| !directory_writable(p)))
    {
        if let Some(root) = dirs::data_local_dir() {
            return root.join("akagi").join(strip_leading_dot(configured));
        }
    }
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

pub fn directory_writable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        path.metadata().is_ok_and(|m| !m.permissions().readonly())
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
    fn relative_dot_path_does_not_leak_into_absolute() {
        // Regression: `./logs` joined with absolute exe-parent used to keep
        // the literal `./` component, producing paths like
        // `/home/.../akagi/./logs/...` that surfaced in the UI as broken.
        let resolved = resolve_dir_inner(Path::new("./logs"));
        let s = resolved.to_string_lossy();
        assert!(!s.contains("/./"), "got: {s}");
        assert!(!s.contains("\\.\\"), "got: {s}");
    }
}
