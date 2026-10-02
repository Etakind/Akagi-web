mod autoplay;
mod bot;
mod capture;
mod general;
mod logging;
mod merge;
mod network;
mod overlay;
mod platform;

pub use autoplay::{
    AutoplayConfig, DelayDistribution, DelayMode, DelayModelConfig, MajsoulAutoplayConfig,
};
pub use bot::{BotConfig, NativeApiConfig};
pub use capture::{CaptureConfig, CaptureMode, ChromiumConfig, HttpCaptureConfig};
pub use general::GeneralConfig;
pub use logging::LoggingConfig;
pub use merge::merge_into;
pub use network::{GithubMirrorMode, NetworkConfig};
pub use overlay::{OverlayConfig, TOP_N_MAX, TOP_N_MIN};
pub use platform::{Platform, PlatformConfig};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub logging: LoggingConfig,
    pub platform: PlatformConfig,
    pub bot: BotConfig,
    pub capture: CaptureConfig,
    pub autoplay: AutoplayConfig,
    pub overlay: OverlayConfig,
    pub network: NetworkConfig,
}

// 仅在读取边界识别废弃字段，运行配置不携带代理或其他游戏实现。
impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut raw = serde_json::Value::deserialize(deserializer)?;
        let object = raw
            .as_object_mut()
            .ok_or_else(|| serde::de::Error::custom("configuration must be a table"))?;
        let had_legacy_proxy = object.contains_key("proxy");
        let legacy_enabled = object
            .get("proxy")
            .and_then(|p| p.get("enabled"))
            .and_then(|v| v.as_bool());
        let unsupported_game = object
            .get("platform")
            .and_then(|p| p.get("kind"))
            .is_some_and(|v| v.as_str() != Some("Majsoul"));
        let capture = object
            .entry("capture")
            .or_insert_with(|| serde_json::json!({}));
        let capture = capture
            .as_object_mut()
            .ok_or_else(|| serde::de::Error::custom("capture must be a table"))?;
        let unsupported_mode = capture
            .get("mode")
            .is_some_and(|v| v.as_str() != Some("chromium"))
            || (had_legacy_proxy
                && !capture.contains_key("mode")
                && !capture.contains_key("enabled"));
        if !capture.contains_key("enabled") {
            capture.insert(
                "enabled".into(),
                serde_json::json!(legacy_enabled.unwrap_or(true)),
            );
        }
        capture.insert("mode".into(), serde_json::json!("chromium"));
        object.insert("platform".into(), serde_json::json!({"kind":"Majsoul"}));
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct CurrentConfig {
            pub general: GeneralConfig,
            pub logging: LoggingConfig,
            pub platform: PlatformConfig,
            pub bot: BotConfig,
            pub capture: CaptureConfig,
            pub autoplay: AutoplayConfig,
            pub overlay: OverlayConfig,
            pub network: NetworkConfig,
        }
        let c: CurrentConfig = serde_json::from_value(raw)
            .map_err(|_| serde::de::Error::custom("invalid configuration; details omitted"))?;
        let mut config = Self {
            general: c.general,
            logging: c.logging,
            platform: c.platform,
            bot: c.bot,
            capture: c.capture,
            autoplay: c.autoplay,
            overlay: c.overlay,
            network: c.network,
        };
        if unsupported_game || unsupported_mode {
            config.capture.enabled = false;
            config.capture.unavailable_reason = Some("此版本仅支持雀魂网页端 Chromium 采集；旧游戏或 MITM 配置已停用，请在设置中确认浏览器后重新启用采集。".into());
        }
        Ok(config)
    }
}

enum ResolvedPath {
    Existing(PathBuf),
    Missing(PathBuf),
}

fn resolve_config_path(cli_path: Option<&Path>) -> ResolvedPath {
    resolve_config_path_inner(cli_path, crate::util::user_config_root())
}

fn resolve_config_path_inner(
    cli_path: Option<&Path>,
    user_cfg_root: Option<PathBuf>,
) -> ResolvedPath {
    if let Some(p) = cli_path {
        if p.exists() {
            return ResolvedPath::Existing(p.to_path_buf());
        }
        return ResolvedPath::Missing(p.to_path_buf());
    }

    // Existing-file search: prefer user config dir, then exe-dir, then cwd.
    if let Some(user_cfg) = &user_cfg_root {
        let candidate = user_cfg.join("config.toml");
        if candidate.exists() {
            return ResolvedPath::Existing(candidate);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join("configs").join("config.toml");
            if candidate.exists() {
                return ResolvedPath::Existing(candidate);
            }
        }
    }

    let cwd_candidate = PathBuf::from("configs.toml");
    if cwd_candidate.exists() {
        return ResolvedPath::Existing(cwd_candidate);
    }

    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("configs").join("config.toml")))
        .unwrap_or(cwd_candidate);
    ResolvedPath::Missing(target)
}

fn write_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            crate::util::private_fs::directory(parent)?;
        }
    }
    let defaults = AppConfig::default();
    let body = toml::to_string_pretty(&defaults).map_err(std::io::Error::other)?;
    crate::util::private_fs::write(path, body)
}

/// Load and parse the config. Returns the parsed `AppConfig` and the
/// path it was loaded from (so callers can persist updates back to the
/// same file via `commands::update_config`).
///
/// On any failure path the in-memory default is returned, but the path
/// returned is the one we *would* have written to — keeping `update_config`
/// from silently writing to an unexpected location.
pub fn load_config(cli_path: Option<&Path>) -> (AppConfig, PathBuf) {
    let path = match resolve_config_path(cli_path) {
        ResolvedPath::Existing(p) => p,
        ResolvedPath::Missing(target) => {
            eprintln!(
                "No config file found, writing defaults to: {}",
                target.display()
            );
            match write_default_config(&target) {
                Ok(()) => target,
                Err(e) => {
                    if let Some(user_cfg) = crate::util::user_config_root() {
                        let fallback = user_cfg.join("config.toml");
                        if fallback != target {
                            eprintln!(
                                "Write to {} failed: {e}. Retrying at {}",
                                target.display(),
                                fallback.display()
                            );
                            match write_default_config(&fallback) {
                                Ok(()) => fallback,
                                Err(e2) => {
                                    eprintln!(
                                        "Failed to write default config: {e2}, using in-memory defaults"
                                    );
                                    return (AppConfig::default(), fallback);
                                }
                            }
                        } else {
                            eprintln!(
                                "Failed to write default config: {e}, using in-memory defaults"
                            );
                            return (AppConfig::default(), target);
                        }
                    } else {
                        eprintln!("Failed to write default config: {e}, using in-memory defaults");
                        return (AppConfig::default(), target);
                    }
                }
            }
        }
    };

    eprintln!("Loading config from: {}", path.display());

    let mut cfg = match crate::util::private_fs::read_and_protect(&path) {
        Ok(content) => match toml::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(_) => {
                eprintln!("Failed to parse config; details omitted, using defaults");
                let mut config = AppConfig::default();
                config.capture.enabled = false;
                config.capture.unavailable_reason =
                    Some("配置无法读取，请在设置中检查后启用采集。".into());
                config
            }
        },
        Err(e) => {
            eprintln!("Failed to read config: {e}, using defaults");
            AppConfig::default()
        }
    };
    // Migrate legacy `[bot] active = "..."` into `active_4p` once.
    cfg.bot.migrate_legacy_active();
    // Pre-existing configs (created before the first-run wizard landed)
    // shouldn't be hijacked into the wizard. Detect by presence of any
    // non-default field that the user must have written deliberately.
    migrate_first_run_marker(&mut cfg, &path);
    (cfg, path)
}

/// Existing users upgrading to a build that introduces the wizard get
/// `first_run_completed = true` automatically — they've already run the
/// app at least once and don't need onboarding. Detected by the config
/// file existing on disk *and* not being a freshly-written defaults file.
///
/// A defaults file written by `write_default_config` contains the full
/// serialised AppConfig; we treat any config file that lacks the new
/// `general.first_run_completed` key as legacy (serde fills the default
/// `false` on parse, so we flip it to `true` after parsing).
fn migrate_first_run_marker(cfg: &mut AppConfig, path: &Path) {
    if cfg.general.first_run_completed {
        return;
    }
    let Ok(body) = std::fs::read_to_string(path) else {
        return;
    };
    // If the file lacks the explicit key, it's a legacy file: respect the
    // user's prior config (whatever is on disk works for them already)
    // and skip the wizard. Fresh defaults files written by us *do* contain
    // the explicit `first_run_completed = false` line, so they still trigger
    // the wizard.
    if !body.contains("first_run_completed") {
        cfg.general.first_run_completed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        d.push(format!("akagi-cfg-{tag}-{pid}-{nanos}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_defaults_when_cli_path_missing() {
        let dir = temp_dir("cli-missing");
        let target = dir.join("nested").join("config.toml");
        assert!(!target.exists());

        let (cfg, path) = load_config(Some(&target));

        assert!(target.exists(), "default config file should be created");
        assert_eq!(path, target);
        let body = std::fs::read_to_string(&target).unwrap();
        let round_trip: AppConfig = toml::from_str(&body).unwrap();
        assert_eq!(
            round_trip.general.first_run_completed,
            cfg.general.first_run_completed
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The nested `[bot.api]` table must survive a TOML round-trip inside the
    /// full `AppConfig` — a table field serialized among scalar `[bot]` keys is
    /// an easy way to get "table before value" ordering wrong. Pure string
    /// round-trip, no network: every value is a placeholder (loopback URL,
    /// fake key, made-up model ids).
    #[test]
    fn bot_api_section_round_trips_through_toml() {
        let mut cfg = AppConfig::default();
        cfg.bot.api.enabled = true;
        cfg.bot.api.base_url = "http://127.0.0.1:8080".into();
        cfg.bot.api.key = "test-key-not-real".into();
        cfg.bot.api.model_4p = "4p-model".into();
        cfg.bot.api.model_3p = "3p-model".into();

        let body = toml::to_string_pretty(&cfg).unwrap();
        assert!(
            body.contains("[bot.api]"),
            "expected a [bot.api] table in:\n{body}"
        );

        let back: AppConfig = toml::from_str(&body).unwrap();
        assert!(back.bot.api.enabled);
        assert_eq!(back.bot.api.base_url, "http://127.0.0.1:8080");
        assert_eq!(back.bot.api.key, "test-key-not-real");
        assert_eq!(back.bot.api.model_4p, "4p-model");
        assert_eq!(back.bot.api.model_3p, "3p-model");
        assert!(back.bot.api.is_active());
        assert_eq!(back.bot.api.model_for(3), "3p-model");
        assert_eq!(back.bot.api.model_for(4), "4p-model");
    }

    /// A legacy config file without any `[bot.api]` section still parses, with
    /// the API path defaulting to off (fully offline local model). The default
    /// server URL is pre-filled but that alone must not activate the API.
    #[test]
    fn missing_bot_api_section_defaults_to_disabled() {
        let legacy = "[bot]\nenabled = true\nactive_4p = \"akagi-native\"\n";
        let cfg: AppConfig = toml::from_str(legacy).unwrap();
        assert!(!cfg.bot.api.enabled);
        assert!(
            !cfg.bot.api.is_active(),
            "default URL alone must not activate"
        );
        assert_eq!(cfg.bot.api.base_url, NativeApiConfig::default().base_url);
        assert!(!cfg.bot.api.base_url.is_empty(), "URL should be pre-filled");
    }

    #[test]
    fn reuses_existing_cli_path() {
        let dir = temp_dir("cli-existing");
        let target = dir.join("config.toml");
        std::fs::write(&target, "[general]\nfirst_run_completed = true\n").unwrap();

        let (cfg, path) = load_config(Some(&target));
        assert!(cfg.general.first_run_completed);
        assert_eq!(path, target);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_appimage_does_not_route_to_user_config_dir() {
        let user_cfg = temp_dir("non-appimage-user-cfg");
        let resolved = resolve_config_path_inner(None, Some(user_cfg.clone()));
        match resolved {
            ResolvedPath::Missing(p) => {
                assert!(
                    !p.starts_with(&user_cfg),
                    "non-appimage should not route to user cfg dir, got {}",
                    p.display()
                );
            }
            ResolvedPath::Existing(p) => panic!("expected Missing, got Existing({})", p.display()),
        }
        std::fs::remove_dir_all(&user_cfg).ok();
    }
}
