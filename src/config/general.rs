use serde::{Deserialize, Serialize};

pub const OVERLAY_DEFAULTS_REVISION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Set to `true` once the first-run setup wizard has finished.
    /// Existing pre-wizard configs default to `true` via migration so
    /// upgraded users don't see the wizard.
    pub first_run_completed: bool,
    /// One-time upgrade opt-in; later user closes must remain closed.
    #[serde(default)]
    pub overlay_defaults_revision: u32,
    #[serde(skip)]
    pub migration_notice: Option<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            first_run_completed: false,
            overlay_defaults_revision: OVERLAY_DEFAULTS_REVISION,
            migration_notice: None,
        }
    }
}

pub(super) fn migrate_overlay_defaults(
    config: &mut super::AppConfig,
    source: &str,
    path: &std::path::Path,
) -> std::io::Result<bool> {
    migrate_with(config, source, |body| {
        crate::util::private_fs::replace(path, body)
    })
}

fn migrate_with(
    config: &mut super::AppConfig,
    source: &str,
    write: impl FnOnce(&str) -> std::io::Result<()>,
) -> std::io::Result<bool> {
    let document: toml_edit::DocumentMut = source.parse().map_err(std::io::Error::other)?;
    let revision = document
        .get("general")
        .and_then(|g| g.get("overlay_defaults_revision"))
        .and_then(toml_edit::Item::as_integer)
        .unwrap_or(0);
    if revision >= i64::from(OVERLAY_DEFAULTS_REVISION) {
        return Ok(false);
    }
    // Patch only these fields: ordinary saves also perform unrelated legacy
    // cleanup, which must not be a side effect of opening the overlay.
    #[derive(Serialize)]
    struct Patch {
        general: Revision,
        overlay: Enabled,
    }
    #[derive(Serialize)]
    struct Revision {
        overlay_defaults_revision: u32,
    }
    #[derive(Serialize)]
    struct Enabled {
        enabled: bool,
    }
    let patch = Patch {
        general: Revision {
            overlay_defaults_revision: OVERLAY_DEFAULTS_REVISION,
        },
        overlay: Enabled { enabled: true },
    };
    let body = super::merge::merge_fields_into(&patch, source).map_err(std::io::Error::other)?;
    write(&body)?;
    config.overlay.enabled = true;
    config.general.overlay_defaults_revision = OVERLAY_DEFAULTS_REVISION;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_config, AppConfig};
    use std::{fs, io};

    #[test]
    fn fresh_user_has_overlay_and_revision_without_skipping_setup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let (config, _) = load_config(Some(&path));
        assert!(config.overlay.enabled);
        assert!(!config.general.first_run_completed);
        assert_eq!(config.general.overlay_defaults_revision, 1);
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("overlay_defaults_revision = 1"));
    }

    #[test]
    fn legacy_closed_overlay_is_enabled_once_and_later_close_survives() {
        for source in [
            "[overlay]\nenabled = false\n",
            "[general]\nfirst_run_completed = true\n[overlay]\nenabled = false\n",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("config.toml");
            fs::write(&path, source).unwrap();
            let (mut config, _) = load_config(Some(&path));
            assert!(config.overlay.enabled);
            assert!(config.general.first_run_completed);
            config.overlay.enabled = false;
            let body =
                crate::config::merge_into(&config, &fs::read_to_string(&path).unwrap()).unwrap();
            fs::write(&path, &body).unwrap();
            let (closed, _) = load_config(Some(&path));
            assert!(!closed.overlay.enabled);
            assert_eq!(closed.general.overlay_defaults_revision, 1);
            assert_eq!(fs::read_to_string(&path).unwrap(), body);
        }
    }

    #[test]
    fn selective_upgrade_preserves_comments_inline_fields_and_legacy_values() {
        let source = "# overlay_defaults_revision = 1 is only a comment\ngeneral = { first_run_completed = false, custom = 42 }\n[overlay]\n# User note\nenabled = false # keep explanation\nfont_size = 20\ncustom = 'keep'\n[capture]\nmode = 'mitm'\n[proxy]\naddr = 'synthetic'\n";
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        fs::write(&path, source).unwrap();
        let (config, _) = load_config(Some(&path));
        assert!(config.overlay.enabled);
        assert!(!config.general.first_run_completed);
        assert_eq!(config.overlay.font_size, 20);
        let body = fs::read_to_string(path).unwrap();
        assert!(body.contains("# User note"));
        assert!(body.contains("# keep explanation"));
        let value: toml::Value = toml::from_str(&body).unwrap();
        assert_eq!(value["general"]["custom"].as_integer(), Some(42));
        assert_eq!(value["overlay"]["custom"].as_str(), Some("keep"));
        assert_eq!(value["capture"]["mode"].as_str(), Some("mitm"));
        assert_eq!(value["proxy"]["addr"].as_str(), Some("synthetic"));
    }

    #[test]
    fn failed_upgrade_does_not_apply_in_memory_or_on_disk() {
        let source = "[general]\nfirst_run_completed = true\n[overlay]\nenabled = false\n";
        let mut config: AppConfig = toml::from_str(source).unwrap();
        assert_eq!(config.general.overlay_defaults_revision, 0);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        fs::write(&path, source).unwrap();
        let result = migrate_with(&mut config, source, |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "simulated failure",
            ))
        });
        assert!(result.is_err());
        assert!(!config.overlay.enabled);
        assert_eq!(config.general.overlay_defaults_revision, 0);
        assert_eq!(fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn wizard_rerun_and_future_revision_preserve_explicit_close() {
        let source = "[general]\nfirst_run_completed = false\noverlay_defaults_revision = 2\n[overlay]\nenabled = false\n";
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        fs::write(&path, source).unwrap();
        let (mut config, _) = load_config(Some(&path));
        assert!(!config.overlay.enabled);
        config.general.first_run_completed = true;
        fs::write(&path, crate::config::merge_into(&config, source).unwrap()).unwrap();
        let (completed, _) = load_config(Some(&path));
        assert_eq!(completed.general.overlay_defaults_revision, 2);
        assert!(completed.general.first_run_completed);
        assert!(!completed.overlay.enabled);
    }
}
