//! Private application files. Never follow a final symlink or truncate before
//! checking ownership/type. Existing private files are tightened in place.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

pub fn directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(io::Error::other("private directory must not be a symlink"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(path)?;
        if dir.metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other("private directory has another owner"));
        }
        dir.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn checked_open(path: &Path, append: bool, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::other("private file must not be a symlink"));
    }
    let f = options.open(path)?;
    let meta = f.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("not a regular private file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != unsafe { libc::geteuid() } || meta.nlink() != 1 {
            return Err(io::Error::other("unsafe private file owner or hard link"));
        }
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(f)
}

pub fn append(path: &Path) -> io::Result<File> {
    checked_open(path, true, true)
}

pub fn create(path: &Path) -> io::Result<File> {
    let f = checked_open(path, false, true)?;
    f.set_len(0)?;
    Ok(f)
}

pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let mut f = checked_open(path, false, true)?;
    f.set_len(0)?;
    f.write_all(bytes.as_ref())
}

pub fn read_and_protect(path: &Path) -> io::Result<String> {
    let mut f = checked_open(path, false, false)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn creates_and_tightens_without_changing_contents() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        directory(&dir).unwrap();
        let p = dir.join("key");
        fs::write(&p, "fixture").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_and_protect(&p).unwrap(), "fixture");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    #[test]
    fn refuses_links_before_truncation() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::write(&real, "unchanged").unwrap();
        let link = root.path().join("link");
        symlink(&real, &link).unwrap();
        assert!(write(&link, "replacement").is_err());
        fs::remove_file(&link).unwrap();
        fs::hard_link(&real, &link).unwrap();
        assert!(write(&link, "replacement").is_err());
        assert_eq!(fs::read_to_string(real).unwrap(), "unchanged");
    }
}
