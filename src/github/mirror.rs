//! gh-proxy-style mirror fallback for GitHub downloads.
//!
//! A gh-proxy accelerator takes the original absolute URL appended to
//! its own origin — `https://<mirror>/https://github.com/...` — and
//! proxies the response. This module turns one GitHub URL plus the
//! user's `[network]` config into an ordered list of candidate URLs to
//! try, each tagged with how much the bytes can be trusted.
//!
//! Trust model: anything fetched through a mirror is attacker-supplied
//! until proven otherwise. Callers must treat `Source::Mirror` results
//! accordingly — the updater requires a valid minisign signature (see
//! [`super::signing`]), the bot installer also accepts a digest authenticated by direct GitHub
//! metadata and rejects unverified mirror content.

use crate::config::{GithubMirrorMode, NetworkConfig};
use serde::Serialize;

/// Where a successful fetch actually came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Straight from GitHub over TLS — trusted as much as it ever was.
    Direct,
    /// Via a third-party accelerator — integrity must come from a
    /// signature, not the transport.
    Mirror,
}

/// Built-in accelerator origins, reachability-tested 2026-08-12. Not
/// every one can proxy `api.github.com` (some 403 it) — that's fine,
/// the refusal is fast and candidate iteration just moves on. Public
/// accelerators churn; the `github_custom_mirror` setting exists
/// precisely because this list will rot between releases.
pub const BUILTIN_MIRRORS: &[&str] = &[
    "https://gh-proxy.com",
    "https://wget.la",
    "https://ghfast.top",
    "https://ghproxy.net",
    "https://gh.llkk.cc",
];

/// `<prefix>/<original absolute URL>` — the shape every gh-proxy-style
/// accelerator accepts.
pub fn prefixed(prefix: &str, url: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), url)
}

/// Ordered `(url, source)` candidates for fetching `url` under the
/// user's mirror mode. The custom mirror (when set) is tried before the
/// built-ins — the user vouched for it.
pub fn candidates(cfg: &NetworkConfig, url: &str) -> Vec<(String, Source)> {
    let direct = (url.to_owned(), Source::Direct);

    let mut mirrors: Vec<(String, Source)> = Vec::new();
    if let Some(custom) = cfg.custom_mirror() {
        mirrors.push((prefixed(custom, url), Source::Mirror));
    }
    for prefix in BUILTIN_MIRRORS {
        let candidate = prefixed(prefix, url);
        // The custom mirror may be one of the built-ins; don't try it twice.
        if mirrors.iter().any(|(u, _)| *u == candidate) {
            continue;
        }
        mirrors.push((candidate, Source::Mirror));
    }

    match cfg.github_mirror_mode {
        GithubMirrorMode::Direct => vec![direct],
        GithubMirrorMode::Auto => {
            let mut v = vec![direct];
            v.extend(mirrors);
            v
        }
        GithubMirrorMode::Mirror => {
            mirrors.push(direct);
            mirrors
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: GithubMirrorMode, custom: &str) -> NetworkConfig {
        NetworkConfig {
            github_mirror_mode: mode,
            github_custom_mirror: custom.into(),
        }
    }

    const URL: &str = "https://github.com/owner/repo/releases/download/v1/a.zip";

    #[test]
    fn prefixed_joins_with_single_slash() {
        assert_eq!(
            prefixed("https://gh-proxy.com", URL),
            format!("https://gh-proxy.com/{URL}")
        );
        assert_eq!(
            prefixed("https://gh-proxy.com/", URL),
            format!("https://gh-proxy.com/{URL}")
        );
    }

    #[test]
    fn direct_mode_never_offers_mirrors() {
        let v = candidates(&cfg(GithubMirrorMode::Direct, "https://gh-proxy.com"), URL);
        assert_eq!(v, vec![(URL.to_owned(), Source::Direct)]);
    }

    #[test]
    fn auto_mode_puts_direct_first_then_mirrors() {
        let v = candidates(&cfg(GithubMirrorMode::Auto, ""), URL);
        assert_eq!(v[0], (URL.to_owned(), Source::Direct));
        assert_eq!(v.len(), 1 + BUILTIN_MIRRORS.len());
        assert!(v[1..].iter().all(|(_, s)| *s == Source::Mirror));
    }

    #[test]
    fn custom_mirror_is_tried_before_builtins_and_deduped() {
        let v = candidates(&cfg(GithubMirrorMode::Auto, "https://my.mirror"), URL);
        assert_eq!(v[1].0, format!("https://my.mirror/{URL}"));

        // Custom equal to a built-in appears exactly once, in the custom slot.
        let v = candidates(&cfg(GithubMirrorMode::Auto, "https://gh-proxy.com/"), URL);
        let hits = v
            .iter()
            .filter(|(u, _)| u.starts_with("https://gh-proxy.com/"))
            .count();
        assert_eq!(hits, 1);
        assert_eq!(v.len(), 1 + BUILTIN_MIRRORS.len());
    }

    #[test]
    fn mirror_mode_puts_direct_last() {
        let v = candidates(&cfg(GithubMirrorMode::Mirror, ""), URL);
        assert_eq!(v.last().unwrap(), &(URL.to_owned(), Source::Direct));
        assert!(v[..v.len() - 1].iter().all(|(_, s)| *s == Source::Mirror));
    }
}
