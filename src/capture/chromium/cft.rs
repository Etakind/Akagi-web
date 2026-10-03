//! Chrome-for-Testing (CfT) downloader.
//!
//! Lets users without a system Chrome install one on demand from
//! Google's official CfT distribution. Avoids bundling a ~150MB browser
//! with the Akagi binary; downloads are runtime-only into a
//! `chrome-for-testing/` directory next to the binary (or the user
//! config dir as a fallback — see [`install_root`]).
//!
//! Manifest URLs (Google):
//! - All versions: <https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json>
//! - Channel pins: <https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json>
//!
//! Install layout under `<install_root>/<version>/`:
//! - macOS: `chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing`
//! - Windows: `chrome-win64\chrome.exe`
//!
//! Triggers (per design):
//! - Settings UI button (explicit)
//! - First-run wizard button (explicit)
//! - **Never** automatic on supervisor start — if no system browser AND
//!   no CfT installed, the supervisor surfaces a notification and
//!   refuses to start. The user clicks Download themselves.

use crate::event_bus::NotifyBus;
use crate::schema::Notification;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

/// CfT release channel + literal-version pin format used by
/// `ChromiumConfig.cft_channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Dev,
    Canary,
    Literal(String),
}

impl Channel {
    pub fn parse(input: &str) -> Self {
        match input.to_ascii_lowercase().as_str() {
            "stable" | "" => Channel::Stable,
            "beta" => Channel::Beta,
            "dev" => Channel::Dev,
            "canary" => Channel::Canary,
            _ => Channel::Literal(input.to_string()),
        }
    }
}

/// CfT platform identifier expected by the manifest (e.g. `win64`,
/// `mac-arm64`). Returns `None` on unsupported platforms.
pub fn cft_platform() -> Option<&'static str> {
    crate::build_targets::current().map(|t| t.cft.as_str())
}

pub fn install_root() -> Result<PathBuf> {
    Ok(crate::util::resolve_dir(Path::new("./chrome-for-testing")))
}

/// Per-version install dir: `<install_root>/<version>/`.
pub fn install_dir_for(version: &str) -> Result<PathBuf> {
    if cft_platform().is_none() {
        bail!("unsupported browser download target");
    }
    if version.split('.').count() != 4
        || !version
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!("invalid browser version");
    }
    Ok(install_root()?.join(version))
}

/// Map an install dir + platform to the chrome executable inside.
pub fn executable_path(install_dir: &Path, platform: &str) -> PathBuf {
    match platform {
        "mac-arm64" | "mac-x64" => install_dir
            .join(format!("chrome-{platform}"))
            .join("Google Chrome for Testing.app")
            .join("Contents/MacOS/Google Chrome for Testing"),
        "win64" => install_dir.join("chrome-win64").join("chrome.exe"),
        _ => install_dir
            .join(format!("chrome-{platform}"))
            .join("chrome"),
    }
}

/// List installed CfT versions, newest first (lex-sort descending —
/// `131.0.6778.85 > 130.0.6723.92` because the dotted segments are
/// fixed-width-ish for the same major). Skips entries whose chrome
/// executable for the current platform is missing.
pub fn list_installed() -> Vec<String> {
    let Ok(root) = install_root() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return vec![];
    };
    let Some(platform) = cft_platform() else {
        return vec![];
    };
    let mut versions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let exe = executable_path(&path, platform);
        if !exe.exists() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            versions.push(name.to_string());
        }
    }
    versions.sort_by_key(|v| std::cmp::Reverse(version_sort_key(v)));
    versions
}

/// Best-effort `127.0.6778.85` → `[127, 0, 6778, 85]` for comparison.
/// Non-numeric components sort as `i64::MIN` so malformed names trail.
fn version_sort_key(v: &str) -> Vec<i64> {
    v.split('.')
        .map(|seg| seg.parse::<i64>().unwrap_or(i64::MIN))
        .collect()
}

/// Find an installed CfT executable. `pinned` (from
/// `ChromiumConfig.cft_channel`) is preferred when it parses as a
/// literal version and that version is installed; otherwise returns
/// the newest installed version's executable. `None` if nothing is
/// installed for the current platform.
pub fn installed_executable(pinned: &Channel) -> Option<PathBuf> {
    let platform = cft_platform()?;
    let installed = list_installed();
    if installed.is_empty() {
        return None;
    }
    let pick = match pinned {
        Channel::Literal(v) if installed.iter().any(|x| x == v) => v.clone(),
        _ => installed[0].clone(),
    };
    let dir = install_dir_for(&pick).ok()?;
    Some(executable_path(&dir, platform))
}

/// Remove a single installed CfT version. Idempotent — missing dir
/// returns Ok.
pub fn remove(version: &str) -> Result<()> {
    let dir = install_dir_for(version)?;
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))
}

// ---------- Manifest fetching ----------

#[derive(Debug, Clone, Deserialize)]
struct ManifestDownload {
    platform: String,
    url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ManifestDownloads {
    #[serde(default)]
    chrome: Vec<ManifestDownload>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelEntry {
    version: String,
    /// Missing downloads are parsed, but resolution fails closed.
    #[serde(default)]
    downloads: ManifestDownloads,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelsManifest {
    channels: ChannelsBag,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelsBag {
    #[serde(rename = "Stable")]
    stable: Option<ChannelEntry>,
    #[serde(rename = "Beta")]
    beta: Option<ChannelEntry>,
    #[serde(rename = "Dev")]
    dev: Option<ChannelEntry>,
    #[serde(rename = "Canary")]
    canary: Option<ChannelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionEntry {
    version: String,
    downloads: ManifestDownloads,
}

#[derive(Debug, Clone, Deserialize)]
struct AllVersionsManifest {
    versions: Vec<VersionEntry>,
}

const CHANNELS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
const ALL_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json";

/// Official binary bucket — every `downloads.chrome[].url` in the
/// manifests points here. Hard-blocked in mainland China.
const GCS_BASE: &str = "https://storage.googleapis.com/chrome-for-testing-public";
const MANIFEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

fn official_asset(url: &str, version: &str, platform: &str) -> bool {
    version.split('.').count() == 4
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && url == format!("{GCS_BASE}/{version}/{platform}/chrome-{platform}.zip")
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    what: &str,
) -> Result<T> {
    client
        .get(url)
        .timeout(MANIFEST_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("fetch {what}"))?
        .error_for_status()
        .with_context(|| format!("{what} endpoint returned an error"))?
        .json()
        .await
        .with_context(|| format!("parse {what}"))
}

fn pick_channel(bag: ChannelsBag, channel: &Channel) -> Result<ChannelEntry> {
    match channel {
        Channel::Stable => bag.stable,
        Channel::Beta => bag.beta,
        Channel::Dev => bag.dev,
        Channel::Canary => bag.canary,
        Channel::Literal(_) => None,
    }
    .ok_or_else(|| anyhow!("channel not present in CfT manifest"))
}

/// Resolve only against Google's HTTPS manifests. No unverified mirror fallback.
async fn resolve_asset(
    client: &reqwest::Client,
    channel: &Channel,
) -> Result<(String, Vec<String>)> {
    let platform =
        cft_platform().ok_or_else(|| anyhow!("unsupported Chrome for Testing platform"))?;
    let (version, downloads) = match channel {
        Channel::Literal(v) => {
            let m: AllVersionsManifest =
                fetch_json(client, ALL_VERSIONS_URL, "official CfT manifest").await?;
            let e = m
                .versions
                .into_iter()
                .find(|e| &e.version == v)
                .ok_or_else(|| anyhow!("requested browser version not found"))?;
            (e.version, e.downloads)
        }
        _ => {
            let m: ChannelsManifest =
                fetch_json(client, CHANNELS_URL, "official CfT channels").await?;
            let e = pick_channel(m.channels, channel)?;
            (e.version, e.downloads)
        }
    };
    let url = downloads
        .chrome
        .into_iter()
        .find(|d| d.platform == platform)
        .ok_or_else(|| anyhow!("official manifest has no browser for this platform"))?
        .url;
    if !official_asset(&url, &version, platform) {
        bail!("non-official browser download refused");
    }
    Ok((version, vec![url]))
}

// ---------- Download + extract ----------

/// Download + extract the requested CfT into the install dir. Reports
/// progress via NotifyBus with sticky id `capture-cft-download` so the
/// frontend can show a single live toast. Idempotent: if the version
/// is already installed, returns early.
pub async fn install(channel: &Channel, notify: &NotifyBus) -> Result<String> {
    if cft_platform().is_none() {
        bail!("unsupported browser download target");
    }
    let client = crate::network::client(crate::network::Purpose::BrowserDownload)?;
    let toast = "capture-cft-download";

    let _ = notify.send(
        Notification::info("Resolving Chrome for Testing")
            .body(format!("Channel: {channel:?}"))
            .sticky()
            .id(toast),
    );
    let (version, url_candidates) = resolve_asset(&client, channel).await?;
    let install_dir = install_dir_for(&version)?;
    if install_dir.exists() && installed_chrome_exists(&install_dir) {
        let _ = notify.send(
            Notification::success(format!("Chrome for Testing {version} already installed"))
                .id(toast),
        );
        return Ok(version);
    }

    let _ = notify.send(
        Notification::info(format!("Downloading Chrome for Testing {version}"))
            .body("0%")
            .sticky()
            .id(toast),
    );

    // Stage download under <root>/.downloads/ so a partial transfer
    // doesn't pollute the version dir.
    let staging_root = install_root()?.join(".downloads");
    std::fs::create_dir_all(&staging_root)
        .with_context(|| format!("create {}", staging_root.display()))?;
    let zip_path = staging_root.join(format!("cft-{version}.zip"));
    let mut last_err: Option<anyhow::Error> = None;
    let mut downloaded = false;
    for url in &url_candidates {
        info!("downloading CfT {version} from {url}");
        match download_with_progress(&client, url, &zip_path, notify, toast, &version).await {
            Ok(()) => {
                downloaded = true;
                break;
            }
            Err(e) => {
                warn!("CfT download from {url} failed: {e:#}");
                let _ = std::fs::remove_file(&zip_path);
                last_err = Some(e.context(format!("download {url}")));
            }
        }
    }
    if !downloaded {
        return Err(last_err.unwrap_or_else(|| anyhow!("no CfT download candidates")));
    }

    let _ = notify.send(
        Notification::info(format!("Installing Chrome for Testing {version}"))
            .body("Extracting…")
            .sticky()
            .id(toast),
    );
    std::fs::create_dir_all(&install_dir)
        .with_context(|| format!("create {}", install_dir.display()))?;
    crate::github::extract_zip_safe(&zip_path, &install_dir).context("extract CfT zip")?;
    let _ = std::fs::remove_file(&zip_path);

    if let Some(platform) = cft_platform() {
        let exe = executable_path(&install_dir, platform);
        if !exe.exists() {
            // Cleanup the half-installed dir to avoid `list_installed`
            // returning a broken version.
            let _ = std::fs::remove_dir_all(&install_dir);
            bail!(
                "CfT extract finished but expected executable {} is missing",
                exe.display()
            );
        }
        post_extract_fixup(&exe, &install_dir);
    }

    let _ = notify
        .send(Notification::success(format!("Chrome for Testing {version} installed")).id(toast));
    Ok(version)
}

fn installed_chrome_exists(install_dir: &Path) -> bool {
    let Some(platform) = cft_platform() else {
        return false;
    };
    executable_path(install_dir, platform).exists()
}

/// Restore executable permission if absent. Never remove macOS quarantine.
#[cfg_attr(not(unix), allow(unused_variables))]
fn post_extract_fixup(exe: &Path, _install_dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(exe) {
            let mut perms = meta.permissions();
            // Add user/group/other execute bits without dropping read/write.
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(exe, perms);
        }
    }
}

async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    notify: &NotifyBus,
    toast: &str,
    version: &str,
) -> Result<()> {
    let mut response = client
        .get(url)
        .send()
        .await
        .context("send CfT download request")?
        .error_for_status()
        .context("CfT download endpoint returned error")?;
    let total = response.content_length();

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    // Bound the quiet time between chunks (not total transfer time) so a
    // stalled connection fails over to the next URL candidate.
    while let Some(chunk) =
        tokio::time::timeout(std::time::Duration::from_secs(60), response.chunk())
            .await
            .context("download stalled")?
            .context("read body chunk")?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", dest.display()))?;
        downloaded += chunk.len() as u64;
        // Cap UI updates to ~4Hz; otherwise we drown the broadcast bus.
        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            last_emit = std::time::Instant::now();
            let body = match total {
                Some(t) if t > 0 => format!(
                    "{:.0}% / {:.1} MB",
                    (downloaded as f64 / t as f64) * 100.0,
                    t as f64 / 1_048_576.0,
                ),
                _ => format!("{:.1} MB", downloaded as f64 / 1_048_576.0),
            };
            let _ = notify.send(
                Notification::info(format!("Downloading Chrome for Testing {version}"))
                    .body(body)
                    .sticky()
                    .id(toast),
            );
        }
    }
    file.flush().await.ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_parses() {
        assert_eq!(Channel::parse("stable"), Channel::Stable);
        assert_eq!(Channel::parse("STABLE"), Channel::Stable);
        assert_eq!(Channel::parse(""), Channel::Stable);
        assert_eq!(Channel::parse("beta"), Channel::Beta);
        assert_eq!(Channel::parse("dev"), Channel::Dev);
        assert_eq!(Channel::parse("canary"), Channel::Canary);
        assert_eq!(
            Channel::parse("131.0.6778.85"),
            Channel::Literal("131.0.6778.85".into())
        );
    }

    #[test]
    fn cft_platform_returns_known_string_on_supported_targets() {
        // Just exercise the cfg branches — actual value depends on host.
        let p = cft_platform();
        if cfg!(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64")
        )) {
            assert!(p.is_some());
            let s = p.unwrap();
            assert!(["mac-arm64", "win64"].contains(&s));
        }
    }

    #[test]
    fn executable_path_per_platform() {
        let dir = PathBuf::from("/tmp/cft/131.0.6778.85");
        assert_eq!(
            executable_path(&dir, "win64"),
            PathBuf::from("/tmp/cft/131.0.6778.85/chrome-win64/chrome.exe")
        );
        let mac = executable_path(&dir, "mac-arm64");
        assert!(
            mac.ends_with("Contents/MacOS/Google Chrome for Testing"),
            "got {}",
            mac.display()
        );
    }

    #[test]
    fn version_sort_orders_numerically() {
        let mut v = vec![
            "129.0.6668.58".to_string(),
            "131.0.6778.85".into(),
            "130.0.6723.92".into(),
        ];
        v.sort_by_key(|x| std::cmp::Reverse(version_sort_key(x)));
        assert_eq!(
            v,
            vec![
                "131.0.6778.85".to_string(),
                "130.0.6723.92".into(),
                "129.0.6668.58".into(),
            ]
        );
    }

    #[test]
    fn version_sort_handles_garbage_gracefully() {
        let key_good = version_sort_key("131.0.6778.85");
        let key_garbage = version_sort_key("not-a-version");
        // garbage should sort *less* than well-formed (i64::MIN component)
        assert!(key_garbage < key_good);
    }

    #[test]
    fn download_origin_and_version_are_strict() {
        assert!(official_asset("https://storage.googleapis.com/chrome-for-testing-public/1.2.3.4/mac-arm64/chrome-mac-arm64.zip", "1.2.3.4", "mac-arm64"));
        assert!(!official_asset(
            "https://evil.test/browser.zip",
            "1.2.3.4",
            "mac-arm64"
        ));
        assert!(!official_asset(
            "https://storage.googleapis.com/chrome-for-testing-public/../chrome-mac-arm64.zip",
            "..",
            "mac-arm64"
        ));
    }

    #[test]
    fn pick_channel_missing_channel_errors() {
        let body = r#"{ "channels": { "Stable": { "version": "1.2.3.4", "downloads": {} } } }"#;
        let m: ChannelsManifest = serde_json::from_str(body).unwrap();
        assert!(pick_channel(m.channels, &Channel::Canary).is_err());
    }

    #[test]
    fn channels_manifest_parses_real_shape() {
        // Real-world shape sample (truncated). Keep the test offline.
        let body = r#"{
            "timestamp": "2026-01-01",
            "channels": {
                "Stable": {
                    "channel": "Stable",
                    "version": "131.0.6778.85",
                    "revision": "1",
                    "downloads": {
                        "chrome": [
                            { "platform": "win64", "url": "https://example.test/chrome-win64.zip" },
                            { "platform": "mac-arm64", "url": "https://example.test/chrome-mac-arm64.zip" }
                        ]
                    }
                }
            }
        }"#;
        let m: ChannelsManifest = serde_json::from_str(body).unwrap();
        let stable = m.channels.stable.unwrap();
        assert_eq!(stable.version, "131.0.6778.85");
        assert_eq!(stable.downloads.chrome.len(), 2);
    }
}
