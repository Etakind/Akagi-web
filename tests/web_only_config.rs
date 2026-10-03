//! 网页维护版配置升级边界。仅使用临时目录及虚构配置。
use akagi::config::{merge_into, AppConfig, CaptureMode};

#[test]
fn new_config_is_majsoul_chromium() {
    let config: AppConfig = toml::from_str("").unwrap();
    assert_eq!(config.capture.mode, CaptureMode::Chromium);
    assert!(config.capture.enabled);
    assert!(config.capture.unavailable_reason.is_none());
    assert!(!config.autoplay.enabled);
    assert_eq!(config.bot.active_4p, "akagi-native");
    let encoded = toml::to_string(&config).unwrap();
    assert!(!encoded.contains("[proxy]"));
}

#[test]
fn existing_browser_and_bot_settings_survive() {
    let config: AppConfig = toml::from_str(
        r#"
[proxy]
enabled = false
[capture]
mode = "chromium"
[capture.chromium]
attach_port = 9222
user_data_dir = "/synthetic/profile"
[bot.api]
key = "FAKE_MIGRATION_KEY"
"#,
    )
    .unwrap();
    assert!(!config.capture.enabled);
    assert_eq!(config.capture.chromium.attach_port, 9222);
    assert_eq!(config.capture.chromium.user_data_dir, "/synthetic/profile");
    assert!(!toml::to_string(&config)
        .unwrap()
        .contains("FAKE_MIGRATION_KEY"));
}

#[test]
fn explicit_capture_toggle_wins_over_legacy_toggle() {
    for (new, old) in [(true, false), (false, true)] {
        let config: AppConfig = toml::from_str(&format!(
            "[proxy]\nenabled = {old}\n[capture]\nenabled = {new}\n"
        ))
        .unwrap();
        assert_eq!(config.capture.enabled, new);
    }
}

#[test]
fn removed_modes_are_blocked_without_resetting_other_settings() {
    for legacy in [
        "[capture]\nmode = 'mitm'",
        "[platform]\nkind = 'RiichiCity'",
    ] {
        let config: AppConfig =
            toml::from_str(&format!("{legacy}\n[bot.api]\nkey = 'FAKE_KEEP_ME'\n")).unwrap();
        assert!(!config.capture.enabled);
        assert!(config.capture.unavailable_reason.is_some());
        assert!(!toml::to_string(&config).unwrap().contains("FAKE_KEEP_ME"));
    }
}

#[test]
fn saving_removes_only_retired_known_keys() {
    let source = r#"
# User note
[proxy]
enabled = false
addr = "127.0.0.1:23410"
ca_dir = "./ca"
block_telemetry = true
rewrite_certificate_report = true
user_note = "keep this unknown value"
[capture]
mode = "chromium"
[other_tool]
custom = 42
"#;
    let config: AppConfig = toml::from_str(source).unwrap();
    let merged = merge_into(&config, source).unwrap();
    let data: toml::Value = toml::from_str(&merged).unwrap();
    assert!(merged.contains("# User note"));
    assert_eq!(
        data["proxy"]["user_note"].as_str(),
        Some("keep this unknown value")
    );
    assert!(data["proxy"].get("addr").is_none());
    assert!(data["proxy"].get("ca_dir").is_none());
    assert!(data["proxy"].get("enabled").is_none());
    assert_eq!(data["other_tool"]["custom"].as_integer(), Some(42));
    assert_eq!(data["capture"]["enabled"].as_bool(), Some(false));
    assert_eq!(merged, merge_into(&config, &merged).unwrap());
}

#[test]
fn reading_unsupported_config_does_not_rewrite_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let source = "[capture]\nmode = 'mitm'\n[logging]\nlevel = 'warn'\n";
    std::fs::write(&path, source).unwrap();
    let (config, _) = akagi::config::load_config(Some(&path));
    assert!(!config.capture.enabled);
    assert!(config.capture.unavailable_reason.is_some());
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn inline_user_fields_survive_config_upgrade() {
    let source = "capture = { enabled = false, mode = 'chromium', my_note = 'keep' }\nproxy = { enabled = true, my_note = 'keep too' }\n";
    let config: AppConfig = toml::from_str(source).unwrap();
    let merged = merge_into(&config, source).unwrap();
    let data: toml::Value = toml::from_str(&merged).unwrap();
    assert_eq!(data["capture"]["my_note"].as_str(), Some("keep"));
    assert_eq!(data["proxy"]["my_note"].as_str(), Some("keep too"));
    assert_eq!(data["capture"]["enabled"].as_bool(), Some(false));
    assert!(data["proxy"].get("enabled").is_none());
}

#[test]
fn pre_capture_proxy_config_does_not_silently_launch_a_browser() {
    let config: AppConfig = toml::from_str("[proxy]\nenabled = true\n").unwrap();
    assert!(!config.capture.enabled);
    assert!(config.capture.unavailable_reason.is_some());
}

#[test]
fn tenhou_is_supported_and_external_bot_migration_disables_autoplay() {
    let source = "[platform]\nkind = 'Tenhou'\n[capture]\nmode = 'chromium'\n[bot]\nactive_4p = 'external'\n[autoplay]\nenabled = true\n[bot.api]\nenabled = true\nkey = 'FAKE_SECRET'\n";
    let config: AppConfig = toml::from_str(source).unwrap();
    assert!(config.capture.enabled);
    assert_eq!(config.platform.kind, akagi::config::Platform::Tenhou);
    assert!(!config.autoplay.enabled);
    assert!(config.bot.migration_notice.is_some());
    assert!(!merge_into(&config, source).unwrap().contains("FAKE_SECRET"));
}
