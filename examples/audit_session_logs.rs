//! Local-only scan. Arguments are log paths, never credentials.
use akagi::util::credentials::Credentials;
use std::path::Path;
fn scan(credentials: &Credentials, path: &Path) -> std::io::Result<bool> {
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        return Ok(false);
    }
    if metadata.is_dir() {
        for entry in path.read_dir()? {
            if !scan(credentials, &entry?.path())? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if !metadata.is_file() {
        return Ok(false);
    }
    Ok(credentials.absent_from(&std::fs::read(path)?))
}
fn main() {
    std::panic::set_hook(Box::new(|_| eprintln!("SESSION_LOG_PRIVACY_FAILED")));
    let status = (|| {
        let credentials =
            Credentials::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("account")).ok()?;
        let paths: Vec<_> = std::env::args_os().skip(1).collect();
        if paths.is_empty() {
            return None;
        }
        for path in paths {
            if !scan(&credentials, Path::new(&path)).ok()? {
                return None;
            }
        }
        Some(())
    })();
    if status.is_some() {
        println!("SESSION_LOG_PRIVACY_PASSED");
    } else {
        println!("SESSION_LOG_PRIVACY_FAILED");
        std::process::exit(1);
    }
}
