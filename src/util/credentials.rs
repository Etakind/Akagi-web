//! Local manual acceptance only. No Debug implementation or data-bearing errors.
use std::path::Path;
#[cfg(unix)]
use std::{fs::OpenOptions, io::Read};
pub struct Credentials {
    pub email: String,
    pub password: String,
}
#[derive(Debug)]
pub struct Rejected;
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CREDENTIAL_FILE_REJECTED")
    }
}
impl std::error::Error for Rejected {}

impl Credentials {
    pub fn read(path: &Path) -> Result<Self, Rejected> {
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Rejected)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            let mut f = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| Rejected)?;
            let m = f.metadata().map_err(|_| Rejected)?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o777 != 0o600
                || m.nlink() != 1
                || m.len() > 65536
            {
                return Err(Rejected);
            }
            let mut s = String::new();
            f.read_to_string(&mut s).map_err(|_| Rejected)?;
            Self::parse(&s)
        }
    }
    #[cfg(any(unix, test))]
    fn parse(s: &str) -> Result<Self, Rejected> {
        let mut lines = s.split_inclusive('\n');
        fn trim_newline(s: &str) -> &str {
            s.strip_suffix("\r\n")
                .or_else(|| s.strip_suffix('\n'))
                .unwrap_or(s)
        }
        let email = trim_newline(lines.next().ok_or(Rejected)?);
        let password = trim_newline(lines.next().ok_or(Rejected)?);
        if lines.next().is_some() || email.is_empty() || password.is_empty() {
            return Err(Rejected);
        }
        Ok(Self {
            email: email.into(),
            password: password.into(),
        })
    }
    pub fn absent_from(&self, bytes: &[u8]) -> bool {
        use base64::Engine;
        for secret in [&self.email, &self.password] {
            let b64 = base64::engine::general_purpose::STANDARD.encode(secret);
            let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
            let json = serde_json::to_string(secret).unwrap_or_default();
            let percent: String = secret.bytes().map(|b| format!("%{b:02X}")).collect();
            for needle in [
                secret.as_str(),
                &b64,
                &hex,
                json.trim_matches('"'),
                &percent,
            ] {
                if !needle.is_empty() && bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
                {
                    return false;
                }
            }
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_password_whitespace() {
        let c = Credentials::parse("fake@example.test\r\n  password \t\r\n").unwrap();
        assert_eq!(c.password, "  password \t");
        assert!(Credentials::parse("a\nb\nc").is_err());
        assert_eq!(Credentials::parse("a\nb\r").unwrap().password, "b\r");
    }
    #[cfg(unix)]
    #[test]
    fn rejects_permissions_and_links() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("account");
        std::fs::write(&p, "fake@example.test\nfake-password\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Credentials::read(&p).is_err());
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Credentials::read(&p).is_ok());
        let link = dir.path().join("link");
        symlink(&p, &link).unwrap();
        assert!(Credentials::read(&link).is_err());
    }
}
