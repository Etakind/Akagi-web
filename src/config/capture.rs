//! 雀魂网页端采集配置。唯一运行模式为 Chromium；旧模式由配置读取层拒绝。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// 旧配置的不可用原因；由配置读取层生成，不接受 IPC 写入。
    #[serde(default, skip_deserializing)]
    pub unavailable_reason: Option<String>,
    pub chromium: ChromiumConfig,
    pub http: HttpCaptureConfig,
}

/// What to record of the HTTP traffic a backend intercepts.
///
/// Akagi used to record WebSocket frames and discard everything else,
/// which hid the game's own HTTP entirely — route topology, version
/// endpoints, and the analytics beacons through which the client reports
/// on itself. This turns that back on.
///
/// `record_all` expands the scope of metadata collection only. Headers,
/// URLs and bodies are always redacted before storage and broadcasting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpCaptureConfig {
    /// Record every intercepted exchange, not just recognized ones.
    /// Off by default — see the struct docs.
    pub record_all: bool,
    /// Legacy compatibility field. Ignored: raw body capture is disabled.
    pub bodies: bool,
    /// Ceiling on a buffered body. Larger ones are recorded with their
    /// size and the reason they were skipped, never truncated silently.
    pub max_body_bytes: usize,
    /// Chromium backend only: also record static subresources (images,
    /// fonts, media, stylesheets). Off by default — a WebGL client pulls
    /// enough of them to bury everything else, and none of it says
    /// anything about the client.
    pub static_assets: bool,
}

impl Default for HttpCaptureConfig {
    fn default() -> Self {
        Self {
            record_all: false,
            bodies: false,
            max_body_bytes: 256 * 1024,
            static_assets: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    #[default]
    Chromium,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromiumConfig {
    /// Absolute path to a chrome/chromium binary. `""` = auto-detect.
    pub executable: String,
    /// Optional existing browser port on 127.0.0.1. Zero launches an isolated browser.
    /// Attached browsers are never closed by Akagi; only official Majsoul tabs are recorded.
    pub attach_port: u16,
    /// In attach mode, reads only DevToolsActivePort here; never mutates the profile.
    /// Otherwise user-data-dir for the controlled profile. `""` = exe-adjacent
    /// `chrome-profile/` (resolved via `util::resolve_dir`).
    pub user_data_dir: String,
    /// URL to navigate to on launch. `""` = don't auto-navigate (open new tab page).
    pub start_url: String,
    /// Chrome-for-Testing channel/version to download as fallback.
    /// `"stable"` / `"beta"` resolve to the latest channel pin; otherwise
    /// treated as a literal version (e.g. `"131.0.6778.85"`).
    pub cft_channel: String,
    /// Force using Chrome-for-Testing even when system Chrome is detected.
    pub force_cft: bool,
    /// Extra CLI args appended after our defaults. Advanced users only.
    pub extra_args: Vec<String>,
}

impl std::fmt::Debug for ChromiumConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChromiumConfig")
            .field("executable", &self.executable)
            .field("start_url", &crate::privacy::url(&self.start_url))
            .field("extra_args", &"[redacted]")
            .field("force_cft", &self.force_cft)
            .finish_non_exhaustive()
    }
}

impl Default for ChromiumConfig {
    fn default() -> Self {
        Self {
            executable: String::new(),
            attach_port: 0,
            user_data_dir: String::new(),
            start_url: "https://game.maj-soul.com/1/".to_string(),
            cft_channel: "stable".to_string(),
            force_cft: false,
            extra_args: vec![],
        }
    }
}

fn enabled_by_default() -> bool {
    true
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Chromium,
            enabled: true,
            unavailable_reason: None,
            chromium: Default::default(),
            http: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let cfg = CaptureConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.mode, CaptureMode::Chromium);
        assert_eq!(back.chromium.cft_channel, "stable");
    }

    #[test]
    fn mode_serialises_lowercase() {
        let cfg = CaptureConfig {
            mode: CaptureMode::Chromium,
            enabled: true,
            unavailable_reason: None,
            chromium: Default::default(),
            http: Default::default(),
        };
        let s = toml::to_string(&cfg).unwrap();
        assert!(s.contains("mode = \"chromium\""), "got: {s}");
    }

    /// Default metadata collection is limited to recognized exchanges.
    #[test]
    fn http_capture_is_opt_in_and_bodies_are_disabled() {
        let cfg = CaptureConfig::default();
        assert!(!cfg.http.record_all, "full capture must be opt-in");
        assert!(!cfg.http.bodies);
        assert!(!cfg.http.static_assets);
        assert_eq!(cfg.http.max_body_bytes, 256 * 1024);

        let s = toml::to_string(&cfg).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.http, cfg.http);
    }

    #[test]
    fn chromium_config_round_trip() {
        let original = CaptureConfig {
            mode: CaptureMode::Chromium,
            enabled: true,
            unavailable_reason: None,
            chromium: ChromiumConfig {
                executable: "/opt/chrome/chrome".into(),
                attach_port: 0,
                user_data_dir: "/tmp/profile".into(),
                start_url: "https://example.test/".into(),
                cft_channel: "131.0.6778.85".into(),
                force_cft: true,
                extra_args: vec!["--lang=ja".into()],
            },
            http: Default::default(),
        };
        let s = toml::to_string(&original).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.mode, CaptureMode::Chromium);
        assert_eq!(back.chromium.executable, "/opt/chrome/chrome");
        assert!(back.chromium.force_cft);
        assert_eq!(back.chromium.extra_args, vec!["--lang=ja"]);
    }

    #[test]
    fn missing_section_uses_defaults() {
        // Older configs that lack [capture] entirely should still parse.
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(default)]
            capture: CaptureConfig,
        }
        let s = "";
        let w: Wrap = toml::from_str(s).unwrap();
        assert_eq!(w.capture.mode, CaptureMode::Chromium);
    }
}
