//! Official release metadata shapes and safe browser archive extraction.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Component, Path};
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Direct,
}
#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReleaseJson {
    #[serde(default)]
    pub tag_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

pub fn find_sig_asset<'a>(assets: &'a [Asset], name: &str) -> Option<&'a Asset> {
    assets
        .iter()
        .find(|asset| asset.name == format!("{name}.minisig"))
}
pub fn extract_zip_safe(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file =
        std::fs::File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("not a valid zip: {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("read zip entry {i}"))?;
        let raw_name = entry.name().to_owned();

        let Some(rel) = entry.enclosed_name() else {
            bail!("zip entry {raw_name:?} has an unsafe path");
        };
        if rel.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("zip entry {raw_name:?} contains `..`");
        }
        if rel.is_absolute() {
            bail!("zip entry {raw_name:?} is absolute");
        }

        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("archive symlink rejected");
        }
        let out_path = dest_dir.join(&rel);
        if entry.is_dir() {
            crate::util::private_fs::directory(&out_path)
                .with_context(|| format!("mkdir {}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            crate::util::private_fs::directory(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("write {}", out_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(
                    &out_path,
                    std::fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o700 } else { 0o600 }),
                );
            }
        }
    }
    Ok(())
}
