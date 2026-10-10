mod autoplay;
mod bot;
mod capture;
mod general;
mod logging;
mod merge;
mod overlay;
mod platform;

pub use autoplay::{
    AutoplayConfig, DelayDistribution, DelayMode, DelayModelConfig, MajsoulAutoplayConfig,
};
pub use bot::BotConfig;
pub use capture::{CaptureConfig, CaptureMode, ChromiumConfig, HttpCaptureConfig};
pub use general::GeneralConfig;
pub use general::OVERLAY_DEFAULTS_REVISION;
pub use logging::LoggingConfig;
pub use merge::merge_into;
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
            .is_some_and(|v| !matches!(v.as_str(), Some("Majsoul" | "Tenhou")));
        let selected_tenhou = object
            .get("platform")
            .and_then(|p| p.get("kind"))
            .and_then(|v| v.as_str())
            == Some("Tenhou");
        let missing_start_url = object
            .get("capture")
            .and_then(|p| p.get("chromium"))
            .and_then(|p| p.get("start_url"))
            .is_none();
        let migrated_bot = object.get("bot").is_some_and(|b| {
            [
                ("active_4p", crate::bot::native::NATIVE_4P),
                ("active_3p", crate::bot::native::NATIVE_3P),
                ("active", crate::bot::native::NATIVE_4P),
            ]
            .iter()
            .any(|(key, expected)| {
                b.get(key)
                    .and_then(|v| v.as_str())
                    .is_some_and(|name| !name.is_empty() && name != *expected)
            })
        });
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
        if unsupported_game {
            object.insert("platform".into(), serde_json::json!({"kind":"Majsoul"}));
        }
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
        };
        config.overlay.font_size = config.overlay.clamped_font_size();
        if selected_tenhou && missing_start_url {
            config.capture.chromium.start_url = Platform::Tenhou.default_url().into();
        }
        config.bot.active_4p = crate::bot::native::NATIVE_4P.into();
        config.bot.active_3p = crate::bot::native::NATIVE_3P.into();
        if migrated_bot {
            config.autoplay.enabled = false;
            config.bot.migration_notice = Some("External bot selection replaced by bundled local models; autoplay disabled. Review settings before enabling it.".into());
        }
        if unsupported_game || unsupported_mode {
            config.capture.enabled = false;
            config.capture.unavailable_reason = Some("此版本仅支持 Majsoul / Tenhou 官方网页端 Chromium 采集；旧游戏或 MITM 配置已停用，请在设置中确认浏览器后重新启用采集。".into());
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

    if let Ok(exe) = std::env::current_exe() {
        if exe
            .parent()
            .is_some_and(|p| !crate::util::directory_writable(p))
        {
            if let Some(root) = user_cfg_root {
                return ResolvedPath::Missing(root.join("config.toml"));
            }
        }
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
            Ok(mut config) => {
                if let Err(error) = general::migrate_overlay_defaults(&mut config, &content, &path)
                {
                    let notice = format!("无法保存悬浮窗的一次性升级设置（{:?}）；已保留原设置。请检查配置文件写入权限后重启，或在设置中手动开启悬浮窗。", error.kind());
                    eprintln!("{notice}");
                    config.general.migration_notice = Some(notice);
                }
                config
            }
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
    fn app_config_normalizes_overlay_font_size_at_the_read_boundary() {
        let legacy: AppConfig = toml::from_str("[bot]\nenabled = true\n").unwrap();
        assert_eq!(
            legacy.overlay.font_size, 14,
            "legacy config gets the new default"
        );

        let too_small: AppConfig = toml::from_str("[overlay]\nfont_size = 0\n").unwrap();
        assert_eq!(too_small.overlay.font_size, 12);

        let too_large: AppConfig = toml::from_str("[overlay]\nfont_size = 100\n").unwrap();
        assert_eq!(too_large.overlay.font_size, 24);
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
