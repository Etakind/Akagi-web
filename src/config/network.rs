use serde::{Deserialize, Serialize};

/// How GitHub-hosted downloads (release metadata, release zips) are
/// routed. Exists because a significant share of users are behind
/// network filtering that black-holes `api.github.com` /
/// `objects.githubusercontent.com`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubMirrorMode {
    /// Try GitHub directly first (bounded by a short timeout), then fall
    /// back to accelerator mirrors. The right default everywhere: users
    /// with working GitHub access never touch a mirror.
    #[default]
    Auto,
    /// Never use mirrors — direct connections only.
    Direct,
    /// Mirrors first, direct as the last resort. For users who know
    /// their GitHub access is blocked and don't want to wait out the
    /// direct-connection timeout on every check.
    Mirror,
}

/// `[network]` section.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub github_mirror_mode: GithubMirrorMode,
    /// Optional gh-proxy-style accelerator prefix, e.g.
    /// `https://gh-proxy.com`. Requests become
    /// `<prefix>/<original absolute URL>`. When set it is tried before
    /// the built-in mirror list (public accelerators churn; a user-picked
    /// one that works today beats our shipped list). Empty = unset.
    pub github_custom_mirror: String,
}

impl NetworkConfig {
    /// The custom mirror, normalised: trimmed, trailing `/` stripped,
    /// `None` unless it is a plausible `http(s)://` prefix.
    pub fn custom_mirror(&self) -> Option<&str> {
        let m = self.github_custom_mirror.trim().trim_end_matches('/');
        if (m.starts_with("https://") || m.starts_with("http://")) && m.len() > "https://".len() {
            Some(m)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for NetworkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkConfig")
            .field("github_mirror_mode", &self.github_mirror_mode)
            .field("github_custom_mirror", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `config.toml` written before this section existed must still
    /// parse and land on the defaults.
    #[test]
    fn older_configs_gain_the_section() {
        let cfg: NetworkConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.github_mirror_mode, GithubMirrorMode::Auto);
        assert!(cfg.custom_mirror().is_none());
    }

    #[test]
    fn custom_mirror_normalises_and_validates() {
        let mk = |s: &str| NetworkConfig {
            github_custom_mirror: s.into(),
            ..Default::default()
        };
        assert_eq!(
            mk(" https://gh-proxy.com/ ").custom_mirror(),
            Some("https://gh-proxy.com")
        );
        assert_eq!(mk("").custom_mirror(), None);
        assert_eq!(mk("gh-proxy.com").custom_mirror(), None, "scheme required");
        assert_eq!(mk("https://").custom_mirror(), None, "host required");
    }
}
