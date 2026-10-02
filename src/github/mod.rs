//! Shared primitives for talking to the GitHub Releases API.
//!
//! Used by `bot::install` (download + extract a bot from a release zip)
//! and `updater` (download + extract Akagi's own binary from a release
//! zip). Both clients hit `api.github.com/repos/<repo>/releases/latest`
//! anonymously, with a `User-Agent: akagi/<version>` header.
//!
//! Submodules:
//! - [`mirror`] — gh-proxy-style accelerator fallback for users whose
//!   network black-holes GitHub. [`fetch_latest_release_mirrored`] and
//!   [`download_with_fallback`] here iterate its candidate URLs.
//! - [`signing`] — minisign verification of release assets, the trust
//!   anchor that makes mirror-sourced bytes safe to use.

pub mod mirror;
pub mod signing;

use crate::config::NetworkConfig;
use anyhow::{bail, Context, Result};
use mirror::Source;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::warn;

pub const GITHUB_API: &str = "https://api.github.com";
pub const USER_AGENT: &str = concat!("akagi/", env!("CARGO_PKG_VERSION"));

/// Per-attempt ceiling for metadata requests (small JSON bodies). Kept
/// short because a blocked host usually black-holes instead of
/// resetting — without this, "try the next mirror" never happens.
const METADATA_TIMEOUT: Duration = Duration::from_secs(15);
/// Max quiet time between two chunks of a streaming download. Bounds
/// stalls without capping total transfer time (release zips are large
/// and links can be slow).
const CHUNK_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// One asset entry from `releases/latest`.
///
/// `size` and `digest` are optional because the older bot-install path
/// doesn't need them, and the digest field only started appearing in
/// 2024 — old releases lack it. The updater verifies SHA-256 when
/// present and warns (but doesn't fail) when absent.
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

/// Build a reqwest client preconfigured with the Akagi `User-Agent`.
/// The connect timeout is deliberately short: on filtered networks a
/// blocked host black-holes the SYN, and the fallback logic can only
/// move on once the attempt fails.
pub fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("build http client")
}

/// Try `op` against each candidate URL in order; return the first
/// success tagged with its [`Source`], or an error joining every
/// attempt's failure.
async fn try_each<T, F, Fut>(candidates: &[(String, Source)], mut op: F) -> Result<(T, Source)>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut errors: Vec<String> = Vec::new();
    for (candidate, source) in candidates {
        match op(candidate.clone()).await {
            Ok(v) => return Ok((v, *source)),
            Err(e) => {
                warn!("fetch via {candidate} failed: {e:#}");
                errors.push(format!(
                    "{}: download failed",
                    crate::privacy::url(candidate)
                ));
            }
        }
    }
    bail!("all sources failed:\n  {}", errors.join("\n  "))
}

/// Fetch `releases/latest` for `<repo>` (pre-validated `owner/name`),
/// trying candidates from [`mirror::candidates`] in order.
/// `Source::Mirror` metadata is attacker-suppliable — callers must not
/// treat its digests as integrity.
pub async fn fetch_latest_release_mirrored(
    client: &reqwest::Client,
    repo: &str,
    net: &NetworkConfig,
) -> Result<(ReleaseJson, Source)> {
    let url = format!("{GITHUB_API}/repos/{repo}/releases/latest");
    try_each(&mirror::candidates(net, &url), |u| async move {
        client
            .get(&u)
            .header("Accept", "application/vnd.github+json")
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .context("fetch release metadata")?
            .error_for_status()
            .context("github release endpoint returned an error")?
            .json()
            .await
            .context("parse release JSON")
    })
    .await
}

/// Cap for [`fetch_text_with_fallback`] bodies. A `.minisig` is ~330
/// bytes; anything past this is a hostile or broken mirror trying to
/// fill memory.
const MAX_TEXT_BYTES: usize = 64 * 1024;

/// Fetch a small text document (e.g. a `.minisig`) trying candidates in
/// order. Returns the body and its source. Bodies over
/// [`MAX_TEXT_BYTES`] are rejected.
pub async fn fetch_text_with_fallback(
    client: &reqwest::Client,
    candidates: &[(String, Source)],
) -> Result<(String, Source)> {
    try_each(candidates, |u| async move {
        let mut response = client
            .get(&u)
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .context("send request")?
            .error_for_status()
            .context("endpoint returned an error")?;
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.context("read body")? {
            if body.len() + chunk.len() > MAX_TEXT_BYTES {
                bail!("response body exceeds {MAX_TEXT_BYTES} bytes");
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).context("response is not UTF-8")
    })
    .await
}

/// Stream one of `candidates` into `dest`, trying each in order.
/// Returns the winning source and the hex SHA-256 of the bytes written.
/// A partial file from a failed attempt is removed before the next
/// attempt; each chunk read is bounded by [`CHUNK_STALL_TIMEOUT`].
pub async fn download_with_fallback(
    client: &reqwest::Client,
    candidates: &[(String, Source)],
    dest: &Path,
) -> Result<(Source, String)> {
    let (digest, source) = try_each(candidates, |u| async move {
        let result = stream_to_file(client, &u, dest).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(dest).await;
        }
        result
    })
    .await?;
    Ok((source, digest))
}

/// Stream `url` to `path`, returning the hex SHA-256 of the bytes
/// written. Hashing happens on the fly so a multi-hundred-MB zip never
/// has to be re-read just to digest it.
async fn stream_to_file(client: &reqwest::Client, url: &str, path: &Path) -> Result<String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .context("send download request")?
        .error_for_status()
        .context("download endpoint returned error")?;

    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("create {}", path.display()))?;
    let mut hasher = Sha256::new();
    loop {
        let chunk = tokio::time::timeout(CHUNK_STALL_TIMEOUT, response.chunk())
            .await
            .context("download stalled")?
            .context("read body chunk")?;
        let Some(chunk) = chunk else { break };
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", path.display()))?;
    }
    file.flush().await.ok();
    Ok(hex_encode(hasher.finalize().as_slice()))
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The `.minisig` companion asset for `asset_name`, if the release
/// ships one.
pub fn find_sig_asset<'a>(assets: &'a [Asset], asset_name: &str) -> Option<&'a Asset> {
    let want = format!("{asset_name}.minisig");
    assets.iter().find(|a| a.name == want)
}

/// Extract `zip_path` into `dest_dir`. Rejects entries whose normalised
/// path escapes the destination root (`..`, absolute paths) — standard
/// "zip slip" defence. Preserves unix mode bits.
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

        let out_path = dest_dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("mkdir {}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("write {}", out_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut z = ZipWriter::new(f);
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(name.to_string(), opts).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn extract_zip_safe_writes_files() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(
            &zip,
            &[("bot.py", b"print('hi')\n"), ("README.md", b"# hi\n")],
        );

        let out = TempDir::new().unwrap();
        extract_zip_safe(&zip, out.path()).unwrap();
        assert!(out.path().join("bot.py").is_file());
        assert!(out.path().join("README.md").is_file());
    }

    #[test]
    fn extract_zip_safe_preserves_directory_layout() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(
            &zip,
            &[
                ("mortal-v1/bot.py", b"print('hi')\n"),
                ("mortal-v1/sub/x.txt", b"x"),
            ],
        );

        let out = TempDir::new().unwrap();
        extract_zip_safe(&zip, out.path()).unwrap();
        assert!(out.path().join("mortal-v1/bot.py").is_file());
        assert!(out.path().join("mortal-v1/sub/x.txt").is_file());
    }

    #[test]
    fn extract_zip_safe_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let zip = tmp.path().join("a.zip");
        make_zip(&zip, &[("../escape.txt", b"nope")]);

        let out = TempDir::new().unwrap();
        let err = extract_zip_safe(&zip, out.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unsafe path") || msg.contains("contains `..`"),
            "got: {msg}"
        );
    }

    #[test]
    fn asset_parses_with_optional_fields() {
        let raw = r#"{
            "name": "akagi-3.0.12-linux-x64.zip",
            "browser_download_url": "https://example.com/x.zip",
            "size": 12345,
            "digest": "sha256:abc"
        }"#;
        let asset: Asset = serde_json::from_str(raw).unwrap();
        assert_eq!(asset.size, Some(12345));
        assert_eq!(asset.digest.as_deref(), Some("sha256:abc"));
    }

    #[test]
    fn asset_parses_without_optional_fields() {
        let raw = r#"{
            "name": "mortal.zip",
            "browser_download_url": "https://example.com/m.zip"
        }"#;
        let asset: Asset = serde_json::from_str(raw).unwrap();
        assert!(asset.size.is_none());
        assert!(asset.digest.is_none());
    }
}
