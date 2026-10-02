//! Install a Mjai bot from a GitHub release or a local `.zip` file.
//!
//! GitHub flow ([`install_from_github_release`]):
//!
//! 1. Fetch `https://api.github.com/repos/<repo>/releases/latest` (anonymous).
//! 2. Pick one asset — by glob if `asset_glob` is set, else the first `.zip`.
//! 3. Stream the asset into a tempfile under `<dest_root>/.downloads/`.
//! 4. Hand off to the shared tail (steps below).
//!
//! Local-zip flow ([`install_from_zip`]): skip steps 1-3 — the caller supplies
//! a `.zip` already on disk — then run the same shared tail. The user's zip is
//! never modified or deleted.
//!
//! Shared tail ([`extract_place_and_finalize`]):
//!
//! 1. Open the zip, validate every entry's path is enclosed inside the
//!    destination (no `..`, no absolute paths).
//! 2. Extract to a sibling tempdir.
//! 3. If the archive has a single top-level directory, strip it.
//! 4. Validate `bot.py` is present in the extracted layout.
//! 5. Atomic-move the tempdir into `<dest_root>/<name>/`.
//! 6. Run `uv sync` when a runtime is available and a `pyproject.toml` exists.
//!
//! Existing `<dest_root>/<name>/` is treated as an error — frontend can
//! offer an explicit "remove and reinstall" toggle later. Authenticated
//! installs (private repos / token), tarballs, and source-tree clones are
//! out of scope for v1.

use crate::bot::manifest::Manifest;
use crate::bot::registry::BotEntry;
use crate::bot::runtime::PythonRuntime;
use crate::config::NetworkConfig;
use crate::event_bus::NotifyBus;
use crate::github::mirror::{self, Source};
use crate::github::{
    build_client, download_with_fallback, extract_zip_safe, fetch_latest_release_mirrored,
    fetch_text_with_fallback, find_sig_asset, signing, Asset,
};
use crate::schema::Notification;
use anyhow::{bail, Context, Result};
use globset::Glob;
use std::path::{Path, PathBuf};

const DOWNLOADS_DIR: &str = ".downloads";

/// Pointer to a GitHub release-zip install.
#[derive(Debug, Clone)]
pub struct GithubInstallSpec {
    /// `owner/name`.
    pub repo: String,
    /// Glob to pick one asset out of the release. `None` → first `.zip`.
    pub asset_glob: Option<String>,
    /// Override target subdir name. `None` → second segment of `repo`.
    pub name: Option<String>,
}

impl GithubInstallSpec {
    /// Resolve the target subdir name. Repo must already be validated.
    fn target_name(&self) -> String {
        if let Some(n) = self.name.as_deref() {
            return n.to_owned();
        }
        // `repo` is already validated to be `owner/name`; second segment
        // is the default install name.
        self.repo.split('/').nth(1).unwrap_or(&self.repo).to_owned()
    }
}

/// End-to-end install. Returns the registry entry for the freshly
/// extracted bot.
///
/// When `runtime` is `Some(_)` and the extracted bot has a `pyproject.toml`,
/// `uv sync` is run as the final step. A sync failure aborts the install and
/// rolls back the freshly-extracted bot dir (the install is atomic — a failed
/// install leaves nothing in the registry); the user re-runs the installer to
/// retry. When `runtime` is `None`, sync is skipped with a warning
/// notification — the install is considered successful.
pub async fn install_from_github_release(
    spec: GithubInstallSpec,
    dest_root: &Path,
    notify: &NotifyBus,
    runtime: Option<&PythonRuntime>,
    net: &NetworkConfig,
) -> Result<BotEntry> {
    let mut spec = spec;
    spec.repo = normalize_repo(&spec.repo);
    validate_repo(&spec.repo)?;
    let target_name = spec.target_name();
    validate_target_name(&target_name)?;
    let dest_dir = dest_root.join(&target_name);
    if dest_dir.exists() {
        bail!(
            "{} already exists — remove it before reinstalling",
            dest_dir.display()
        );
    }

    let notify_id = format!("bot-install-{target_name}");
    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body(format!("Fetching release metadata for {}", spec.repo))
            .sticky()
            .id(notify_id.clone()),
    );

    let client = build_client()?;
    let (release, meta_source) = fetch_latest_release_mirrored(&client, &spec.repo, net).await?;

    let asset = pick_asset(&release.assets, spec.asset_glob.as_deref())?;

    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body(format!(
                "Downloading {} ({})",
                asset.name,
                release.tag_name.as_deref().unwrap_or("latest"),
            ))
            .sticky()
            .id(notify_id.clone()),
    );

    let downloads_dir = dest_root.join(DOWNLOADS_DIR);
    tokio::fs::create_dir_all(&downloads_dir)
        .await
        .with_context(|| format!("mkdir {}", downloads_dir.display()))?;

    let tempfile_path = downloads_dir.join(format!(
        "akagi-bot-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let zip_candidates = mirror::candidates(net, &asset.browser_download_url);
    let (zip_source, zip_digest) = download_with_fallback(&client, &zip_candidates, &tempfile_path)
        .await
        .with_context(|| format!("download {}", asset.browser_download_url))?;

    // When the metadata came straight from GitHub its digest field is
    // trustworthy — check the downloaded bytes against it regardless of
    // which route they took.
    let trusted_digest =
        match authenticated_digest(meta_source, asset.digest.as_deref(), &zip_digest) {
            Ok(trusted) => trusted,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tempfile_path).await;
                return Err(e);
            }
        };

    // An existing signature must verify, even when a digest matches.
    // Mirrored metadata/archive without a signature requires a digest
    // authenticated by a direct GitHub metadata response.
    match find_sig_asset(&release.assets, &asset.name) {
        Some(sig_asset) => {
            let sig_candidates = mirror::candidates(net, &sig_asset.browser_download_url);
            let (sig_text, _) = fetch_text_with_fallback(&client, &sig_candidates)
                .await
                .with_context(|| format!("download signature {}", sig_asset.name))?;
            if let Err(e) = signing::verify_release_asset(&tempfile_path, &sig_text, &asset.name) {
                let _ = tokio::fs::remove_file(&tempfile_path).await;
                return Err(e)
                    .with_context(|| format!("signature verification failed for {}", asset.name));
            }
        }
        None if !unsigned_transport_allowed(meta_source, zip_source, trusted_digest) => {
            let _ = tokio::fs::remove_file(&tempfile_path).await;
            bail!("Unverified mirrored bot refused before extraction or dependency installation. Use a signed release or a digest obtained directly from GitHub.");
        }
        None => {}
    }

    let tag = release.tag_name.as_deref().unwrap_or("latest").to_owned();
    // Hand off to the shared tail. The download tempfile is deleted after
    // extraction (`delete_zip_after = true`).
    extract_place_and_finalize(
        &tempfile_path,
        dest_root,
        &target_name,
        &spec.repo,
        format!("{} ({tag})", spec.repo),
        notify,
        &notify_id,
        runtime,
        true,
    )
    .await
}

fn authenticated_digest(source: Source, expected: Option<&str>, actual: &str) -> Result<bool> {
    if source != Source::Direct {
        return Ok(false);
    }
    let Some(expected) = expected.and_then(crate::updater::check::parse_sha256_digest) else {
        return Ok(false);
    };
    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("download does not match the SHA-256 obtained directly from GitHub");
    }
    Ok(true)
}

fn unsigned_transport_allowed(metadata: Source, archive: Source, trusted_digest: bool) -> bool {
    (metadata == Source::Direct && archive == Source::Direct)
        || (metadata == Source::Direct && trusted_digest)
}

/// Pointer to a local zip-file install.
#[derive(Debug, Clone)]
pub struct LocalZipInstallSpec {
    /// Path to a `.zip` file already on disk.
    pub zip_path: PathBuf,
    /// Override target subdir name. `None` → the zip file's stem.
    pub name: Option<String>,
}

impl LocalZipInstallSpec {
    /// Resolve the target subdir name from the override, else the zip stem.
    fn target_name(&self) -> String {
        if let Some(n) = self.name.as_deref() {
            return n.to_owned();
        }
        self.zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("bot")
            .to_owned()
    }
}

/// Install a bot from a `.zip` already on disk. Same pipeline as
/// [`install_from_github_release`] minus the GitHub release lookup and
/// download — the caller supplies the zip directly. The source zip is never
/// modified or deleted.
///
/// As with the GitHub path, an existing `<dest_root>/<name>/` is an error and
/// `uv sync` runs as the final step when `runtime` is `Some(_)` and the bot
/// ships a `pyproject.toml`.
pub async fn install_from_zip(
    spec: LocalZipInstallSpec,
    dest_root: &Path,
    notify: &NotifyBus,
    runtime: Option<&PythonRuntime>,
) -> Result<BotEntry> {
    if !spec.zip_path.is_file() {
        bail!("{} is not a file", spec.zip_path.display());
    }
    let target_name = spec.target_name();
    validate_target_name(&target_name)?;
    let dest_dir = dest_root.join(&target_name);
    if dest_dir.exists() {
        bail!(
            "{} already exists — remove it before reinstalling",
            dest_dir.display()
        );
    }

    // Name the source by its filename in user-facing messages.
    let source_label = spec
        .zip_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("zip")
        .to_owned();

    let notify_id = format!("bot-install-{target_name}");
    extract_place_and_finalize(
        &spec.zip_path,
        dest_root,
        &target_name,
        &source_label,
        source_label.clone(),
        notify,
        &notify_id,
        runtime,
        false,
    )
    .await
}

/// Shared install tail for both the GitHub-release and local-zip installers.
/// Given a `.zip` already on disk, extract it into `<dest_root>/<target_name>`,
/// validate the layout, run `uv sync` when a runtime is available, and return
/// the registry entry.
///
/// `source_label` names the install source in the "may not be a valid bot"
/// warning; `success_body` is the body of the final success notification. When
/// `delete_zip_after` is true the input zip is removed after extraction (the
/// GitHub download tempfile); user-supplied zips are left intact.
#[allow(clippy::too_many_arguments)]
async fn extract_place_and_finalize(
    zip_path: &Path,
    dest_root: &Path,
    target_name: &str,
    source_label: &str,
    success_body: String,
    notify: &NotifyBus,
    notify_id: &str,
    runtime: Option<&PythonRuntime>,
    delete_zip_after: bool,
) -> Result<BotEntry> {
    let dest_dir = dest_root.join(target_name);

    let _ = notify.send(
        Notification::info(format!("Installing {target_name}"))
            .body("Extracting…")
            .sticky()
            .id(notify_id.to_owned()),
    );

    let downloads_dir = dest_root.join(DOWNLOADS_DIR);
    tokio::fs::create_dir_all(&downloads_dir)
        .await
        .with_context(|| format!("mkdir {}", downloads_dir.display()))?;

    // Extract into a sibling dir of the eventual destination so the
    // final rename stays on the same filesystem. We use a manually-named
    // dir (not TempDir) because we may rename the dir itself away — Drop
    // semantics on a renamed TempDir are awkward.
    let staging = downloads_dir.join(format!(
        "akagi-extract-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let install_result = (|| -> Result<()> {
        std::fs::create_dir(&staging).with_context(|| format!("mkdir {}", staging.display()))?;
        extract_zip_safe(zip_path, &staging)
            .with_context(|| format!("extract {}", zip_path.display()))?;

        let resolved_root = strip_single_top_level(&staging)?;
        validate_layout(&resolved_root)?;

        std::fs::rename(&resolved_root, &dest_dir).with_context(|| {
            format!(
                "rename {} -> {}",
                resolved_root.display(),
                dest_dir.display()
            )
        })?;
        Ok(())
    })();

    // Best-effort cleanup of any leftover staging contents. If the
    // staging dir was renamed away wholesale, this is a no-op error.
    let _ = std::fs::remove_dir_all(&staging);
    if delete_zip_after {
        let _ = tokio::fs::remove_file(zip_path).await;
    }

    install_result?;

    // Heads-up (non-fatal): a complete Akagi bot ships `pyproject.toml`
    // (deps / `uv sync`) and `manifest.toml` (settings schema). `bot.py`
    // alone clears `validate_layout`, but if these are also missing the user
    // most likely pointed the installer at the wrong source. Warn so they can
    // double-check rather than silently ending up with a bot that has no deps
    // and no settings.
    let missing = missing_recommended_files(&dest_dir);
    if !missing.is_empty() {
        let _ = notify.send(
            Notification::warn(format!("{target_name} may not be a valid bot"))
                .body(format!(
                    "Installed from {} but it is missing {}. This might not be the \
                     target you intended to install — a complete Akagi bot ships both \
                     pyproject.toml and manifest.toml.",
                    source_label,
                    missing.join(" and "),
                ))
                .id(format!("{notify_id}-layout")),
        );
    }

    // Post-install: run `uv sync` so dependency failures surface here rather
    // than at game-start. Skip silently for pyproject-less bots — only the
    // explicit Reinstall-environment path treats that as an error.
    let pyproject = dest_dir.join("pyproject.toml");
    if pyproject.is_file() {
        match runtime {
            Some(rt) => {
                let _ = notify.send(
                    Notification::info(format!("Installing {target_name}"))
                        .body("Installing Python dependencies (uv sync)…")
                        .sticky()
                        .id(notify_id.to_owned()),
                );
                if let Err(e) = rt.ensure_synced(&dest_dir).await {
                    // Make the install atomic: a failed `uv sync` must not
                    // leave a half-installed bot dir behind (it would show up
                    // in the registry as an env-not-ready bot the user then has
                    // to delete by hand). Roll back the freshly-extracted dir;
                    // the user re-runs the installer to retry.
                    let _ = std::fs::remove_dir_all(&dest_dir);
                    return Err(e)
                        .with_context(|| format!("uv sync after install of {target_name}"));
                }
            }
            None => {
                let _ = notify.send(
                    Notification::warn(format!("{target_name} installed without sync"))
                        .body("No python3+uv runtime found on PATH; deps will be installed at game start.")
                        .id(notify_id.to_owned()),
                );
            }
        }
    }

    let _ = notify.send(
        Notification::success(format!("{target_name} installed"))
            .body(success_body)
            .id(notify_id.to_owned()),
    );

    Ok(BotEntry {
        name: target_name.to_owned(),
        dir: dest_dir.clone(),
        pyproject: pyproject.is_file().then_some(pyproject),
        manifest: Manifest::load(&dest_dir).ok().flatten(),
    })
}

fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        bail!("repo {repo:?} is not in the form `owner/name`");
    }
    for part in parts {
        if !part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            bail!("repo {repo:?} has illegal characters; expected [A-Za-z0-9._-]+/[A-Za-z0-9._-]+");
        }
    }
    Ok(())
}

/// Reduce a user-supplied repo identifier to canonical `owner/name`.
/// Accepts: bare `owner/name`, `https?://[www.]github.com/owner/name`,
/// optional `.git` suffix, trailing slash, query string, hash fragment.
/// The result still goes through `validate_repo`; this function only
/// strips the parts that are unambiguously safe to drop.
pub fn normalize_repo(input: &str) -> String {
    let mut s = input.trim();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest;
    }
    if let Some(rest) = s.strip_prefix("github.com/") {
        s = rest;
    }
    if let Some(idx) = s.find(['?', '#']) {
        s = &s[..idx];
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        s.to_owned()
    }
}

fn validate_target_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('.') {
        bail!("target name {name:?} is invalid");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("target name {name:?} contains path separators");
    }
    if name == "base" {
        bail!("target name {name:?} is reserved");
    }
    Ok(())
}

/// Choose one asset out of the release.
///
/// Public for unit tests so we can verify the glob and fallback rules
/// without spinning up a network mock.
pub fn pick_asset<'a>(assets: &'a [Asset], glob: Option<&str>) -> Result<&'a Asset> {
    if assets.is_empty() {
        bail!("release has no assets");
    }

    if let Some(pat) = glob {
        let matcher = Glob::new(pat)
            .with_context(|| format!("compile glob {pat:?}"))?
            .compile_matcher();
        let matches: Vec<&Asset> = assets
            .iter()
            .filter(|a| matcher.is_match(&a.name))
            .collect();
        match matches.len() {
            0 => bail!("no asset in release matches glob {pat:?}"),
            1 => Ok(matches[0]),
            n => bail!(
                "{n} assets matched glob {pat:?}; tighten the pattern (matched: {:?})",
                matches.iter().map(|a| &a.name).collect::<Vec<_>>()
            ),
        }
    } else {
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no .zip asset in release; pass asset_glob to pick a non-zip explicitly"
                )
            })
    }
}

/// If `dir` contains exactly one entry and that entry is a directory,
/// return that nested directory. Otherwise return `dir` unchanged.
///
/// Real-world release zips usually wrap everything in a single top-level
/// dir like `mortal-v0.5.0/…`. Strip it so the bot's `bot.py` ends up
/// directly under `<bot>/bot.py` rather than `<bot>/mortal-v0.5.0/bot.py`.
pub fn strip_single_top_level(dir: &Path) -> Result<PathBuf> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    if entries.len() != 1 {
        return Ok(dir.to_path_buf());
    }
    let only = entries.remove(0);
    if only.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
        Ok(only.path())
    } else {
        Ok(dir.to_path_buf())
    }
}

/// Reject installs that don't look like bots — `bot.py` is the registry
/// contract. Pyproject is recommended but not enforced (some bots may
/// run on system python without uv).
pub fn validate_layout(bot_root: &Path) -> Result<()> {
    let bot_py = bot_root.join("bot.py");
    if !bot_py.is_file() {
        bail!("extracted archive does not contain bot.py at the top level — refusing install");
    }
    Ok(())
}

/// Files a well-formed Akagi bot is expected to ship alongside `bot.py`:
/// `pyproject.toml` (Python deps / `uv sync`) and `manifest.toml` (settings
/// schema + metadata). Recommended, not required — `validate_layout` only
/// hard-fails on a missing `bot.py`. When these are also absent the install
/// target is most likely not the bot the user meant to install.
const RECOMMENDED_FILES: [&str; 2] = ["pyproject.toml", "manifest.toml"];

/// Return the [`RECOMMENDED_FILES`] that are *not* present at the top level
/// of `bot_root`, in declaration order. An empty vec means the layout looks
/// like a complete bot.
pub fn missing_recommended_files(bot_root: &Path) -> Vec<&'static str> {
    RECOMMENDED_FILES
        .into_iter()
        .filter(|name| !bot_root.join(name).is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.com/{name}"),
            size: None,
            digest: None,
        }
    }

    #[test]
    fn validate_repo_accepts_owner_slash_name() {
        validate_repo("Equim-chan/Mortal").unwrap();
        validate_repo("a.b/c-d_e.f").unwrap();
    }

    #[test]
    fn validate_repo_rejects_bad_input() {
        for bad in [
            "",
            "noslash",
            "/leading",
            "trailing/",
            "a/b/c",
            "owner with space/name",
            "owner/name?query",
        ] {
            let err = validate_repo(bad).unwrap_err();
            assert!(
                err.to_string().contains("repo"),
                "expected error for {bad:?}: {err:#}"
            );
        }
    }

    #[test]
    fn validate_target_name_rejects_separators_and_reserved() {
        validate_target_name("mortal").unwrap();
        for bad in ["", ".hidden", "a/b", "a\\b", "..", "with..parent", "base"] {
            assert!(
                validate_target_name(bad).is_err(),
                "{bad:?} should be invalid"
            );
        }
    }

    #[test]
    fn target_name_defaults_to_repo_second_segment() {
        let s = GithubInstallSpec {
            repo: "owner/name-with-dashes".into(),
            asset_glob: None,
            name: None,
        };
        assert_eq!(s.target_name(), "name-with-dashes");
    }

    #[test]
    fn target_name_override_takes_precedence() {
        let s = GithubInstallSpec {
            repo: "owner/upstream".into(),
            asset_glob: None,
            name: Some("local-alias".into()),
        };
        assert_eq!(s.target_name(), "local-alias");
    }

    #[test]
    fn pick_asset_first_zip_when_no_glob() {
        let assets = vec![
            asset("checksum.txt"),
            asset("mortal-v1.zip"),
            asset("mortal-v2.zip"),
        ];
        let chosen = pick_asset(&assets, None).unwrap();
        assert_eq!(chosen.name, "mortal-v1.zip");
    }

    #[test]
    fn pick_asset_no_zip_errors_without_glob() {
        let assets = vec![asset("checksum.txt"), asset("source.tar.gz")];
        let err = pick_asset(&assets, None).unwrap_err();
        assert!(err.to_string().contains("no .zip asset"));
    }

    #[test]
    fn pick_asset_glob_single_match() {
        let assets = vec![
            asset("mortal-v1.zip"),
            asset("mortal-v1-debug.zip"),
            asset("mortal-v1.sig"),
        ];
        let chosen = pick_asset(&assets, Some("mortal-v?.zip")).unwrap();
        assert_eq!(chosen.name, "mortal-v1.zip");
    }

    #[test]
    fn pick_asset_glob_no_match_errors() {
        let assets = vec![asset("mortal-v1.zip")];
        let err = pick_asset(&assets, Some("nope-*.zip")).unwrap_err();
        assert!(err.to_string().contains("no asset"));
    }

    #[test]
    fn pick_asset_glob_multiple_matches_errors() {
        let assets = vec![asset("mortal-v1.zip"), asset("mortal-v2.zip")];
        let err = pick_asset(&assets, Some("mortal-*.zip")).unwrap_err();
        assert!(err.to_string().contains("matched glob"));
    }

    #[test]
    fn pick_asset_empty_release_errors() {
        let err = pick_asset(&[], None).unwrap_err();
        assert!(err.to_string().contains("no assets"));
    }

    #[test]
    fn strip_top_level_strips_single_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("mortal-v1/sub")).unwrap();
        std::fs::write(tmp.path().join("mortal-v1/bot.py"), b"").unwrap();

        let resolved = strip_single_top_level(tmp.path()).unwrap();
        assert_eq!(resolved, tmp.path().join("mortal-v1"));
    }

    #[test]
    fn strip_top_level_keeps_root_when_multiple_entries() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bot.py"), b"").unwrap();
        std::fs::write(tmp.path().join("README.md"), b"").unwrap();

        let resolved = strip_single_top_level(tmp.path()).unwrap();
        assert_eq!(resolved, tmp.path());
    }

    #[test]
    fn validate_layout_requires_bot_py() {
        let tmp = TempDir::new().unwrap();
        let err = validate_layout(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("bot.py"));

        std::fs::write(tmp.path().join("bot.py"), b"").unwrap();
        validate_layout(tmp.path()).unwrap();
    }

    #[test]
    fn missing_recommended_files_flags_absent_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bot.py"), b"").unwrap();

        // Neither recommended file present → both reported, in order.
        assert_eq!(
            missing_recommended_files(tmp.path()),
            vec!["pyproject.toml", "manifest.toml"],
        );

        // Only pyproject present → manifest still flagged.
        std::fs::write(tmp.path().join("pyproject.toml"), b"").unwrap();
        assert_eq!(missing_recommended_files(tmp.path()), vec!["manifest.toml"]);

        // Both present → nothing flagged.
        std::fs::write(tmp.path().join("manifest.toml"), b"").unwrap();
        assert!(missing_recommended_files(tmp.path()).is_empty());
    }

    // --- local-zip install ------------------------------------------------

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;
        let f = std::fs::File::create(path).unwrap();
        let mut z = ZipWriter::new(f);
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file((*name).to_string(), opts).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn local_zip_target_name_defaults_to_stem() {
        let s = LocalZipInstallSpec {
            zip_path: PathBuf::from("/tmp/cool-bot-v1.0.zip"),
            name: None,
        };
        assert_eq!(s.target_name(), "cool-bot-v1.0");
    }

    #[test]
    fn local_zip_target_name_override_takes_precedence() {
        let s = LocalZipInstallSpec {
            zip_path: PathBuf::from("/tmp/cool-bot.zip"),
            name: Some("alias".into()),
        };
        assert_eq!(s.target_name(), "alias");
    }

    #[tokio::test]
    async fn install_from_zip_extracts_and_keeps_source() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let zip = src.path().join("mybot.zip");
        // Archive wrapped in a single top-level dir, like a real release zip.
        make_zip(&zip, &[("mybot/bot.py", b"print('hi')\n")]);

        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: zip.clone(),
            name: Some("testbot".into()),
        };
        let entry = install_from_zip(spec, dest.path(), &notify, None)
            .await
            .expect("install should succeed");

        assert_eq!(entry.name, "testbot");
        assert_eq!(entry.dir, dest.path().join("testbot"));
        assert!(dest.path().join("testbot/bot.py").is_file());
        assert!(entry.pyproject.is_none());
        // The user's source zip must be left untouched.
        assert!(zip.is_file(), "source zip should not be deleted");
    }

    #[tokio::test]
    async fn install_from_zip_derives_name_from_filename() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        // No wrapping dir: a single top-level file stays at the root.
        let zip = src.path().join("coolbot.zip");
        make_zip(&zip, &[("bot.py", b"print('hi')\n")]);

        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: zip,
            name: None,
        };
        let entry = install_from_zip(spec, dest.path(), &notify, None)
            .await
            .expect("install should succeed");

        assert_eq!(entry.name, "coolbot");
        assert!(dest.path().join("coolbot/bot.py").is_file());
    }

    #[tokio::test]
    async fn install_from_zip_rejects_existing_dest() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let zip = src.path().join("dup.zip");
        make_zip(&zip, &[("bot.py", b"")]);
        std::fs::create_dir(dest.path().join("dup")).unwrap();

        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: zip,
            name: Some("dup".into()),
        };
        let err = install_from_zip(spec, dest.path(), &notify, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn install_from_zip_rejects_missing_file() {
        let dest = TempDir::new().unwrap();
        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: PathBuf::from("/no/such/file.zip"),
            name: None,
        };
        let err = install_from_zip(spec, dest.path(), &notify, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("is not a file"));
    }

    #[tokio::test]
    async fn install_from_zip_rejects_archive_without_bot_py() {
        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let zip = src.path().join("nobot.zip");
        make_zip(&zip, &[("README.md", b"# hi\n")]);

        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: zip,
            name: Some("nobot".into()),
        };
        let err = install_from_zip(spec, dest.path(), &notify, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("bot.py"));
        // Failed install leaves no destination dir behind.
        assert!(!dest.path().join("nobot").exists());
    }

    #[tokio::test]
    async fn install_from_zip_rolls_back_on_sync_failure() {
        use crate::bot::runtime::{PythonRuntime, RuntimeMode};

        let src = TempDir::new().unwrap();
        let dest = TempDir::new().unwrap();
        let zip = src.path().join("syncfail.zip");
        // Ship a pyproject (+ lock) so the installer attempts `uv sync`.
        make_zip(
            &zip,
            &[
                ("bot.py", b""),
                (
                    "pyproject.toml",
                    b"[project]\nname = \"x\"\nversion = \"0.0.0\"\n",
                ),
                ("uv.lock", b"# dummy\n"),
            ],
        );

        // A runtime pointing at non-existent binaries makes `uv sync` fail at
        // spawn time, exercising the rollback path.
        let rt = PythonRuntime::from_paths(
            PathBuf::from("/nonexistent/python"),
            PathBuf::from("/nonexistent/uv"),
            RuntimeMode::System,
        );
        let notify = crate::event_bus::notify_bus();
        let spec = LocalZipInstallSpec {
            zip_path: zip,
            name: Some("syncfail".into()),
        };
        let err = install_from_zip(spec, dest.path(), &notify, Some(&rt))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("uv sync"));
        // Atomic install: a failed `uv sync` must roll back the bot dir so it
        // doesn't linger in the registry as an env-not-ready bot.
        assert!(
            !dest.path().join("syncfail").exists(),
            "failed sync must not leave a bot dir behind"
        );
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn forged_or_mirror_supplied_digests_are_not_trusted() {
        let digest = "a".repeat(64);
        let expected = format!("sha256:{digest}");
        assert!(authenticated_digest(Source::Direct, Some(&expected), &digest).unwrap());
        assert!(authenticated_digest(Source::Direct, Some(&expected), &"b".repeat(64)).is_err());
        assert!(!authenticated_digest(Source::Mirror, Some(&expected), &digest).unwrap());
        assert!(!authenticated_digest(Source::Direct, None, &digest).unwrap());
    }
    #[test]
    fn unsigned_mirrors_require_direct_authenticated_digest() {
        assert!(unsigned_transport_allowed(
            Source::Direct,
            Source::Direct,
            false
        ));
        assert!(unsigned_transport_allowed(
            Source::Direct,
            Source::Mirror,
            true
        ));
        assert!(!unsigned_transport_allowed(
            Source::Direct,
            Source::Mirror,
            false
        ));
        assert!(!unsigned_transport_allowed(
            Source::Mirror,
            Source::Direct,
            true
        ));
        assert!(!unsigned_transport_allowed(
            Source::Mirror,
            Source::Mirror,
            true
        ));
    }
}
