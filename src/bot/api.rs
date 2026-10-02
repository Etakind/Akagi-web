//! HTTP client for the remote inference API (`/v3/*`), as published by the
//! inference server's own API documentation.
//!
//! The service is **stateless**: every [`ApiClient::react`] call uploads the
//! current kyoku's mjai event stream (from the bot's seat perspective, ending
//! at the decision point) and gets back the move to play. This module is a thin
//! typed wrapper around those endpoints — the mjai stream shaping / censoring
//! lives in [`crate::bot::native`], the caller.
//!
//! Two consumers:
//! - the built-in bot ([`crate::bot::native::NativeBot`]) calls
//!   [`ApiClient::react`] at each decision point, while cloud inference is on;
//! - the IPC layer ([`crate::ipc::commands`]) calls [`redeem`],
//!   [`ApiClient::key_status`], [`ApiClient::models`] and [`health`] so the
//!   frontend can redeem codes and inspect a key.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// Timeout for the management endpoints (key status, models, redeem, health).
/// These run from the UI, never on a game's critical path, so they can afford
/// to wait out a slow server.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Default timeout for [`ApiClient::react`], which sits on a live game's
/// critical path. Majsoul's turn timer is ~5s plus a shared time bank, and a
/// reach costs two react calls (declare, then discard), so the whole two-step
/// has to fit inside one turn. Keep this tight: a hung server falls back to the
/// local model promptly instead of making the bot miss its turn. Repeated
/// failures are then suppressed by the caller's circuit breaker, so only the
/// first turn of an outage pays this cost.
///
/// This is only the fallback for clients built without an explicit timeout
/// (e.g. the management-endpoint helpers). The built-in bot overrides it per
/// session from `bot.api.react_timeout_ms` via [`ApiClient::with_react_timeout`].
pub const REACT_TIMEOUT: Duration = Duration::from_millis(3_000);

/// Timeout for `POST /v3/review`. A submit uploads a whole game (hundreds of
/// KB gzipped) and the server replays the entire stream through its rules
/// engine before answering `202` — comfortably slower than the management
/// endpoints, and never on a game's critical path, so give it real headroom.
const REVIEW_SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Response from `POST /v3/react`.
#[derive(Debug, Clone, Deserialize)]
pub struct ReactResponse {
    /// The move to play, as a standard mjai event (`actor == player_id`).
    /// `None` when the seat has no legal action for the final event.
    #[serde(default)]
    pub reaction: Option<Value>,
    /// Up to `topk` coarse action labels ranked by probability. `candidates[0]`
    /// corresponds to `reaction`.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// The model id that actually served the request.
    #[serde(default)]
    pub model: Option<String>,
}

/// One entry of the policy's top-k distribution. `action` is a coarse label
/// (e.g. `dahai:W`, `reach`, `pon`) — the exact tiles are in `reaction`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub action: String,
    #[serde(default)]
    pub prob: f64,
}

/// Response from `GET /v3/key` — the key's plan, expiry and live limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStatus {
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub usage_today: u64,
    #[serde(default)]
    pub rpd: u64,
    /// Requests/minute. The server reports this as a float (e.g. `10.0`), so it
    /// is typed `f64` — deserializing it into an integer would fail the parse.
    #[serde(default)]
    pub rpm: f64,
    #[serde(default)]
    pub topk: u32,
    /// Whole-game reviews submitted today. Separate meter from `usage_today`
    /// (resets at UTC midnight, not server-local). Absent on older servers ⇒ 0.
    #[serde(default)]
    pub reviews_today: u64,
    /// Review jobs the plan allows per day. `0` ⇒ the plan has no review
    /// access (`POST /v3/review` answers 403). Absent on older servers ⇒ 0.
    #[serde(default)]
    pub reviews_per_day: u64,
}

/// One model the key's plan may use (`GET /v3/models`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub desc: String,
}

/// Response from `POST /v3/redeem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    /// The raw 32-char key — present **only** when a new key is minted
    /// (`extended == false`). Never re-shown on a renewal.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_last4: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub expires_at: String,
    /// `true` when time was stacked onto an existing key (no new key issued).
    #[serde(default)]
    pub extended: bool,
}

/// Response from `GET /healthz` — liveness + aggregate load.
///
/// The endpoint exposes aggregates only; nothing about the model registry
/// (model ids come from the authenticated, plan-filtered `/v3/models`).
/// `status` is `"degraded"` when any model worker is down.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    #[serde(default)]
    pub status: String,
    /// Total pending + in-flight inference rows. Older servers sent a
    /// per-model map here — still accepted, summed to the same total, so a
    /// health check against a not-yet-updated server doesn't read as down.
    #[serde(default, deserialize_with = "queue_depth_total")]
    pub queue_depth: i64,
    /// `false` when any model worker is down. Absent on older servers;
    /// defaults to `true` so their `"ok"` doesn't render as degraded.
    #[serde(default = "default_true")]
    pub workers_alive: bool,
}

fn default_true() -> bool {
    true
}

// ---------- Whole-game review (`/v3/review*`, `/v3/shares`, `/v3/shared/*`) ----------

/// Response from `POST /v3/review` — the queued background job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSubmitted {
    pub review_id: String,
    #[serde(default)]
    pub status: String,
}

/// Response from `GET /v3/review/{review_id}` — job progress, meta-only.
/// A `done` job carries the share URL; the result body itself is only ever
/// served through that URL (`GET /v3/shared/{share_id}`, no auth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewJobStatus {
    /// `queued` | `running` | `failed` | `done`.
    #[serde(default)]
    pub status: String,
    /// 0.0..=1.0 while queued/running.
    #[serde(default)]
    pub progress: Option<f64>,
    /// Failure reason when `status == "failed"`.
    #[serde(default)]
    pub error: Option<String>,
    /// Set when `done`; `None` after a revoke (re-issue via
    /// [`ApiClient::review_share`]).
    #[serde(default)]
    pub share_id: Option<String>,
    /// Public viewer URL for the result when `done` (and not revoked).
    #[serde(default)]
    pub url: Option<String>,
}

/// Response from `POST /v3/review/{review_id}/share` — the review's public
/// link (re-issued with a fresh id after a revoke).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareIssued {
    pub share_id: String,
    pub url: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub anonymized: bool,
}

/// Aggregate result numbers carried by the share listing, so a list can be
/// labelled without fetching result bodies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShareSummary {
    #[serde(default)]
    pub n_decisions: u64,
    #[serde(default)]
    pub n_match: u64,
    #[serde(default)]
    pub match_rate: f64,
    #[serde(default)]
    pub avg_actual_prob: f64,
}

/// One live share link from `GET /v3/shares` (newest first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareEntry {
    pub share_id: String,
    /// The review job this share serves — the join key back to a submit.
    #[serde(default)]
    pub review_id: String,
    /// The review's submit time (RFC 3339).
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub anonymized: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub player_id: Option<u8>,
    #[serde(default)]
    pub summary: Option<ShareSummary>,
}

/// Accept the aggregate integer or the legacy per-model map (summed).
fn queue_depth_total<'de, D>(de: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Depth {
        Total(i64),
        PerModel(BTreeMap<String, i64>),
    }
    Ok(match Depth::deserialize(de)? {
        Depth::Total(n) => n,
        Depth::PerModel(m) => m.values().sum(),
    })
}

#[derive(Serialize)]
struct ReactRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    player_id: u8,
    events: Vec<Value>,
}

#[derive(Serialize)]
struct ReviewRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    player_id: Option<u8>,
    events: Vec<Value>,
}

#[derive(Serialize)]
struct RedeemRequest<'a> {
    code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    renew_key: Option<&'a str>,
}

/// Authenticated client bound to one server + key.
///
/// Each `ApiClient` builds its own `reqwest::Client`, and with it a fresh
/// connection pool — so **hold one and reuse it** rather than constructing one
/// per request, or every call pays a new TCP + TLS handshake. The built-in bot
/// keeps one alive across a game and only rebuilds it when the server URL or
/// key changes.
pub struct ApiClient {
    base: String,
    key: String,
    http: reqwest::Client,
    /// Per-call timeout for [`Self::react`]. Defaults to [`REACT_TIMEOUT`];
    /// overridden per session via [`Self::with_react_timeout`].
    react_timeout: Duration,
}

impl ApiClient {
    /// Build a client for `base_url` authenticating with `key`, optionally
    /// routing through `proxy` (see [`http_client`]; empty ⇒ direct). A
    /// trailing slash on the URL is tolerated. The react timeout starts at the
    /// [`REACT_TIMEOUT`] default; use [`Self::with_react_timeout`] to override.
    pub fn new(base_url: &str, key: &str, proxy: &str) -> Result<Self> {
        Ok(Self {
            base: normalize_base(base_url),
            key: key.trim().to_string(),
            http: http_client(REQUEST_TIMEOUT, proxy)?,
            react_timeout: REACT_TIMEOUT,
        })
    }

    /// Override the per-call timeout applied to [`Self::react`]. The built-in
    /// bot sets this from `bot.api.react_timeout_ms` (already clamped by
    /// `NativeApiConfig::effective_react_timeout`). Builder-style so existing
    /// non-react callers (redeem/key/models) keep the default unchanged.
    pub fn with_react_timeout(mut self, timeout: Duration) -> Self {
        self.react_timeout = timeout;
        self
    }

    /// `POST /v3/react` — the move for the final event of `events`. `model`
    /// `None`/empty lets the server pick its game default.
    ///
    /// Bounded by [`Self::react_timeout`] (the configured react timeout) rather
    /// than the client-wide [`REQUEST_TIMEOUT`]: this one blocks the bot's turn.
    pub async fn react(
        &self,
        model: Option<&str>,
        player_id: u8,
        events: Vec<Value>,
    ) -> Result<ReactResponse> {
        let url = format!("{}/v3/react", self.base);
        let body = ReactRequest {
            model: model.filter(|m| !m.is_empty()),
            player_id,
            events,
        };
        let resp = self
            .http
            .post(&url)
            .timeout(self.react_timeout)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("POST /v3/react")?;
        let resp = check(resp, "react").await?;
        resp.json::<ReactResponse>()
            .await
            .context("parse /v3/react response")
    }

    /// `GET /v3/key` — the key's plan, expiry and live rate limits.
    pub async fn key_status(&self) -> Result<KeyStatus> {
        let url = format!("{}/v3/key", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("GET /v3/key")?;
        let resp = check(resp, "key status").await?;
        resp.json::<KeyStatus>()
            .await
            .context("parse /v3/key response")
    }

    /// `GET /v3/models` — the models this key's plan may use.
    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/v3/models", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("GET /v3/models")?;
        let resp = check(resp, "models").await?;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            models: Vec<ModelInfo>,
        }
        Ok(resp
            .json::<Wrap>()
            .await
            .context("parse /v3/models response")?
            .models)
    }

    /// `POST /v3/review` — queue a whole-game background review. `events` is
    /// the full game from the reviewed seat's censored perspective (same
    /// shaping as [`Self::react`]); `player_id` pins the seat the server
    /// infers, as a consistency check. The body is gzipped — the server
    /// auto-detects the magic bytes, no `Content-Encoding` needed.
    ///
    /// Rate limits worth surfacing to the user: one accepted submit per key
    /// per 10 minutes, `reviews_per_day` per UTC day, and one queued/running
    /// job per key at a time — all answered as `429` with `Retry-After`,
    /// which [`check`] folds into the error message.
    pub async fn submit_review(
        &self,
        model: Option<&str>,
        player_id: Option<u8>,
        events: Vec<Value>,
    ) -> Result<ReviewSubmitted> {
        let url = format!("{}/v3/review", self.base);
        let body = ReviewRequest {
            model: model.filter(|m| !m.is_empty()),
            player_id,
            events,
        };
        let raw = serde_json::to_vec(&body).context("encode /v3/review body")?;
        let gz = gzip(&raw).context("gzip /v3/review body")?;
        let resp = self
            .http
            .post(&url)
            .timeout(REVIEW_SUBMIT_TIMEOUT)
            .bearer_auth(&self.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(gz)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("POST /v3/review")?;
        let resp = check(resp, "review submit").await?;
        resp.json::<ReviewSubmitted>()
            .await
            .context("parse /v3/review response")
    }

    /// `GET /v3/review/{review_id}` — poll a review job (meta-only; the
    /// result body is served exclusively through the share URL). Polls draw
    /// on a separate per-key read bucket (20/min, burst 40) shared with
    /// [`Self::shares`] and [`Self::revoke_share`] — poll at a 4-5 s cadence.
    pub async fn review_status(&self, review_id: &str) -> Result<ReviewJobStatus> {
        let id = validate_id(review_id, "review id")?;
        let url = format!("{}/v3/review/{id}", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("GET /v3/review/<id>")?;
        let resp = check(resp, "review status").await?;
        resp.json::<ReviewJobStatus>()
            .await
            .context("parse review status response")
    }

    /// `POST /v3/review/{review_id}/share` — the review's public link.
    /// Answers `409 review_not_done` while the job is still queued/running;
    /// after a revoke it re-issues a fresh link under a **new** id.
    pub async fn review_share(&self, review_id: &str) -> Result<ShareIssued> {
        let id = validate_id(review_id, "review id")?;
        let url = format!("{}/v3/review/{id}/share", self.base);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("POST /v3/review/<id>/share")?;
        let resp = check(resp, "review share").await?;
        resp.json::<ShareIssued>()
            .await
            .context("parse review share response")
    }

    /// `GET /v3/shares` — this key's live share links, newest first. Works
    /// for an expired key too (expiry gates inference, not share management).
    pub async fn shares(&self) -> Result<Vec<ShareEntry>> {
        let url = format!("{}/v3/shares", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("GET /v3/shares")?;
        let resp = check(resp, "shares").await?;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            shares: Vec<ShareEntry>,
        }
        Ok(resp
            .json::<Wrap>()
            .await
            .context("parse /v3/shares response")?
            .shares)
    }

    /// `DELETE /v3/shared/{share_id}` — revoke a share link (creator key
    /// only). Unpublishes the result body for everyone including us; the
    /// review itself stays stored and [`Self::review_share`] can re-issue a
    /// fresh link later.
    pub async fn revoke_share(&self, share_id: &str) -> Result<()> {
        let id = validate_id(share_id, "share id")?;
        let url = format!("{}/v3/shared/{id}", self.base);
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("DELETE /v3/shared/<id>")?;
        check(resp, "share revoke").await?;
        Ok(())
    }
}

/// Guard an id that gets interpolated into a URL path. Server-issued ids are
/// plain ASCII alphanumerics (32-hex review ids, base62 share ids); anything
/// else — and in particular `/`, `?`, `#`, `..` — must never reach the URL,
/// where it would address a different route.
fn validate_id<'a>(id: &'a str, what: &str) -> Result<&'a str> {
    let id = id.trim();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("invalid {what}");
    }
    Ok(id)
}

/// Gzip `raw` at the default compression level.
fn gzip(raw: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(raw)?;
    Ok(enc.finish()?)
}

/// `POST /v3/redeem` (no auth). By default mints a **new** key; pass
/// `renew_key` to stack time onto a key you already hold. `email` links minted
/// keys to an account (ignored when `renew_key` is set).
pub async fn redeem(
    base_url: &str,
    proxy: &str,
    code: &str,
    email: Option<&str>,
    renew_key: Option<&str>,
) -> Result<RedeemResponse> {
    let base = normalize_base(base_url);
    let http = http_client(REQUEST_TIMEOUT, proxy)?;
    let url = format!("{base}/v3/redeem");
    let body = RedeemRequest {
        code: code.trim(),
        email: email.map(str::trim).filter(|s| !s.is_empty()),
        renew_key: renew_key.map(str::trim).filter(|s| !s.is_empty()),
    };
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("POST /v3/redeem")?;
    let resp = check(resp, "redeem").await?;
    resp.json::<RedeemResponse>()
        .await
        .context("parse /v3/redeem response")
}

/// `GET /healthz` (no auth) — liveness + aggregate load.
pub async fn health(base_url: &str, proxy: &str) -> Result<Health> {
    let base = normalize_base(base_url);
    let http = http_client(REQUEST_TIMEOUT, proxy)?;
    let url = format!("{base}/healthz");
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("GET /healthz")?;
    let resp = check(resp, "health").await?;
    resp.json::<Health>()
        .await
        .context("parse /healthz response")
}

/// Build the HTTP client all inference-server traffic goes through, honoring
/// the user's proxy setting (`bot.api.proxy`). `proxy` accepts
/// `http://`, `https://`, `socks5://` or `socks5h://` URLs (`socks5h` resolves
/// DNS on the proxy — what you want when the server's name doesn't resolve
/// locally); empty/whitespace ⇒ direct connection. Shared with
/// [`crate::bot::purchase`], so the whole `/v3` + `/paypal` surface follows
/// one setting.
pub(crate) fn http_client(timeout: Duration, proxy: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    let proxy = proxy.trim();
    if !proxy.is_empty() {
        // Scheme match is case-insensitive (RFC 3986; the url crate lowercases
        // on parse anyway) — `HTTP://…` pasted from a provider dashboard works.
        const SCHEMES: [&str; 4] = ["http://", "https://", "socks5://", "socks5h://"];
        let scheme_ok = SCHEMES.iter().any(|s| {
            proxy
                .get(..s.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(s))
        });
        // Never echo the proxy string into errors: proxy URLs routinely carry
        // credentials (`socks5://user:pass@host`), and these messages end up
        // in toasts AND in the persisted session log (which users attach to
        // bug reports). The user just typed the value — no diagnostic value
        // in repeating it.
        if !scheme_ok {
            bail!("unsupported proxy scheme: use http://, https://, socks5:// or socks5h://");
        }
        let p = reqwest::Proxy::all(proxy).context("invalid proxy URL")?;
        builder = builder.proxy(p);
    }
    builder.build().context("build inference-API http client")
}

pub(crate) fn normalize_base(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

/// Turn non-2xx responses into status-only errors with numeric Retry-After.
/// Untrusted error bodies may echo credentials and are never surfaced. Success
/// passes the response through untouched for the caller to deserialize.
/// Shared with the purchase client ([`crate::bot::purchase`]).
pub(crate) async fn check(resp: reqwest::Response, what: &str) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let code = status.as_u16();
    match retry_after.and_then(|s| s.parse::<u64>().ok()) {
        Some(ra) => bail!("{what} failed: HTTP {code} (retry after {ra}s)"),
        None => bail!("{what} failed: HTTP {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_client_accepts_supported_proxy_schemes_and_empty() {
        for p in [
            "",
            "  ",
            "http://127.0.0.1:7890",
            "https://p:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
            // Schemes are case-insensitive (RFC 3986).
            "HTTP://127.0.0.1:7890",
            "SOCKS5h://127.0.0.1:1080",
        ] {
            assert!(
                http_client(REQUEST_TIMEOUT, p).is_ok(),
                "proxy {p:?} should build"
            );
        }
    }

    #[test]
    fn http_client_rejects_unknown_proxy_scheme() {
        for p in [
            "ftp://x",
            "socks4://127.0.0.1:1080",
            "127.0.0.1:7890",
            "socks5:/bad",
        ] {
            let err = http_client(REQUEST_TIMEOUT, p).unwrap_err();
            assert!(
                format!("{err:#}").contains("unsupported proxy scheme"),
                "proxy {p:?} should be rejected with a clear message, got: {err:#}"
            );
        }
    }

    #[test]
    fn proxy_errors_never_echo_the_credentials() {
        // Proxy URLs often carry `user:pass@host`; the error text lands in
        // toasts and persisted logs, so it must not contain the secret.
        let err = http_client(REQUEST_TIMEOUT, "socks4://user:sekret@10.0.0.1:1080").unwrap_err();
        let text = format!("{err:#}");
        assert!(
            !text.contains("sekret") && !text.contains("10.0.0.1"),
            "error text leaked the proxy string: {text}"
        );
    }

    #[test]
    fn normalize_trims_trailing_slash_and_space() {
        assert_eq!(normalize_base("http://host:8080/"), "http://host:8080");
        assert_eq!(normalize_base("  https://host  "), "https://host");
        assert_eq!(normalize_base("http://host:8080"), "http://host:8080");
    }

    #[test]
    fn react_request_omits_empty_model() {
        let body = ReactRequest {
            model: None,
            player_id: 0,
            events: vec![],
        };
        let v = serde_json::to_value(&body).unwrap();
        assert!(v.get("model").is_none(), "model must be omitted when None");
        assert_eq!(v["player_id"], 0);
    }

    #[test]
    fn react_request_keeps_model_when_set() {
        let body = ReactRequest {
            model: Some("4p-ot2"),
            player_id: 2,
            events: vec![],
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["model"], "4p-ot2");
        assert_eq!(v["player_id"], 2);
    }

    #[test]
    fn redeem_request_omits_blank_optionals() {
        let body = RedeemRequest {
            code: "abc",
            email: Some("  "),
            renew_key: None,
        };
        // Callers pass pre-filtered options; this mirrors the API's default
        // "mint a new key anonymously" path — code only.
        let filtered = RedeemRequest {
            code: body.code,
            email: body.email.map(str::trim).filter(|s| !s.is_empty()),
            renew_key: body.renew_key,
        };
        let v = serde_json::to_value(&filtered).unwrap();
        assert_eq!(v["code"], "abc");
        assert!(v.get("email").is_none());
        assert!(v.get("renew_key").is_none());
    }

    #[test]
    fn react_response_defaults_missing_fields() {
        let r: ReactResponse = serde_json::from_str("{}").unwrap();
        assert!(r.reaction.is_none());
        assert!(r.candidates.is_empty());
        assert!(r.model.is_none());
    }

    /// The live server returns `rpm` as a float (`10.0`); parsing it into an
    /// integer would fail, so `KeyStatus.rpm` is `f64`. Lock that in.
    #[test]
    fn key_status_parses_float_rpm() {
        let raw = r#"{"plan":"basic","expires_at":"2026-08-04 19:15:42","usage_today":3,"rpd":6000,"rpm":10.0,"topk":3}"#;
        let k: KeyStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(k.plan, "basic");
        assert_eq!(k.usage_today, 3);
        assert_eq!(k.rpd, 6000);
        assert!((k.rpm - 10.0).abs() < 1e-9);
        assert_eq!(k.topk, 3);
    }

    /// The 2026-08 server update reshaped `/healthz`: `models` was dropped
    /// and `queue_depth` became a single aggregate integer. Parsing that
    /// integer into the old per-model map type was a hard serde error — a
    /// healthy server would have shown up as a failed health check.
    #[test]
    fn health_parses_aggregate_shape() {
        let raw = r#"{"status":"ok","queue_depth":3,"workers_alive":true}"#;
        let h: Health = serde_json::from_str(raw).unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.queue_depth, 3);
        assert!(h.workers_alive);

        let raw = r#"{"status":"degraded","queue_depth":0,"workers_alive":false}"#;
        let h: Health = serde_json::from_str(raw).unwrap();
        assert_eq!(h.status, "degraded");
        assert!(!h.workers_alive);
    }

    /// A not-yet-updated server still sends the legacy shape (per-model map
    /// plus a `models` list). It must keep parsing: the map sums to the same
    /// aggregate total, unknown fields are ignored, and the missing
    /// `workers_alive` defaults to alive rather than degraded.
    #[test]
    fn health_accepts_legacy_per_model_shape() {
        let raw = r#"{"status":"ok","models":["4p-x","3p-x"],"queue_depth":{"4p-x":2,"3p-x":1}}"#;
        let h: Health = serde_json::from_str(raw).unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.queue_depth, 3);
        assert!(h.workers_alive);

        // Bare minimum body — every field defaulted.
        let h: Health = serde_json::from_str("{}").unwrap();
        assert_eq!(h.queue_depth, 0);
        assert!(h.workers_alive);
    }

    #[test]
    fn models_wrapper_parses() {
        let raw = r#"{"models":[{"id":"4p-ot2","game":"4p","desc":"4p Mortal v4 (ot2), 192x40"},{"id":"3p-ot2","game":"3p","desc":"3p Mortal v4"}]}"#;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            models: Vec<ModelInfo>,
        }
        let w: Wrap = serde_json::from_str(raw).unwrap();
        assert_eq!(w.models.len(), 2);
        assert_eq!(w.models[0].id, "4p-ot2");
        assert_eq!(w.models[1].game, "3p");
    }

    // ---- end-to-end against a local mock server ----

    use crate::bot::test_http::mock_http;

    /// Value of `name` in a raw HTTP request head, case-insensitively.
    fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
        req.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    /// The bearer key must travel in the `Authorization` header — never in the
    /// URL or query string, where proxies and server access logs would capture it.
    #[tokio::test]
    async fn react_authenticates_with_a_bearer_header() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"reaction":{"type":"none"},"candidates":[{"action":"none","prob":1.0}],"model":"4p-x"}"#
                .into(),
        )]);
        let client = ApiClient::new(&base, "SECRETKEY", "").unwrap();
        let resp = client
            .react(
                Some("4p-x"),
                2,
                vec![serde_json::json!({"type": "start_game"})],
            )
            .await
            .unwrap();
        assert_eq!(resp.model.as_deref(), Some("4p-x"));

        let reqs = served.join().unwrap();
        let r = &reqs[0];
        assert!(r.starts_with("POST /v3/react "), "wrong request line: {r}");
        assert_eq!(header(r, "authorization"), Some("Bearer SECRETKEY"));
        assert!(
            !r.lines().next().unwrap().contains("SECRETKEY"),
            "key must not appear in the request line: {r}"
        );
        assert!(r.contains(r#""player_id":2"#), "{r}");
        assert!(r.contains(r#""model":"4p-x""#), "{r}");
    }

    /// The react timeout is honored per client: a server that answers slower
    /// than the configured timeout produces a transport error (which the caller
    /// turns into a local-model fallback), while a generous timeout lets the
    /// same slow response through. Regression for the configurable
    /// `react_timeout_ms` (issue #264).
    #[tokio::test]
    async fn react_honors_the_configured_timeout() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        // A one-shot server that stalls `delay` before replying 200 OK.
        fn slow_server(delay: Duration) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                if let Ok((mut sock, _)) = listener.accept() {
                    // Drain the request head so the client's send() completes;
                    // the small body fits the socket buffer, so we needn't read
                    // it all before stalling.
                    let mut buf = [0u8; 2048];
                    let _ = sock.read(&mut buf);
                    std::thread::sleep(delay);
                    let body = r#"{"reaction":{"type":"none"}}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len(),
                    );
                    let _ = sock.write_all(resp.as_bytes());
                }
            });
            format!("http://{addr}")
        }

        let events = || vec![serde_json::json!({"type": "start_game"})];

        // Timeout well under the server's delay ⇒ the react errors out.
        let base = slow_server(Duration::from_millis(400));
        let tight = ApiClient::new(&base, "K", "")
            .unwrap()
            .with_react_timeout(Duration::from_millis(80));
        assert!(
            tight.react(None, 0, events()).await.is_err(),
            "a react slower than the timeout must error"
        );

        // Same delay, timeout comfortably over it ⇒ the react succeeds.
        let base = slow_server(Duration::from_millis(400));
        let generous = ApiClient::new(&base, "K", "")
            .unwrap()
            .with_react_timeout(Duration::from_millis(3_000));
        assert!(
            generous.react(None, 0, events()).await.is_ok(),
            "a react within the timeout must succeed"
        );
    }

    #[tokio::test]
    async fn key_status_and_models_authenticate_with_a_bearer_header() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"plan":"basic","expires_at":"2026-08-04","usage_today":1,"rpd":6000,"rpm":10.0,"topk":3}"#.into(),
            ),
            (
                "200 OK",
                r#"{"models":[{"id":"4p-x","game":"4p","desc":"d"}]}"#.into(),
            ),
        ]);
        let client = ApiClient::new(&base, "SECRETKEY", "").unwrap();
        assert_eq!(client.key_status().await.unwrap().plan, "basic");
        assert_eq!(client.models().await.unwrap()[0].id, "4p-x");

        let reqs = served.join().unwrap();
        assert!(reqs[0].starts_with("GET /v3/key "), "{}", reqs[0]);
        assert_eq!(header(&reqs[0], "authorization"), Some("Bearer SECRETKEY"));
        assert!(reqs[1].starts_with("GET /v3/models "), "{}", reqs[1]);
        assert_eq!(header(&reqs[1], "authorization"), Some("Bearer SECRETKEY"));
    }

    /// `redeem` and `health` are the unauthenticated endpoints — a buyer calls
    /// them before they hold a key. Sending one would leak it needlessly.
    #[tokio::test]
    async fn redeem_and_health_send_no_authorization_header() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"key":"K","key_last4":"cdef","plan":"basic","expires_at":"2026-08-04","extended":false}"#.into(),
            ),
            (
                "200 OK",
                r#"{"status":"ok","queue_depth":0,"workers_alive":true}"#.into(),
            ),
        ]);
        let r = redeem(&base, "", " CODE ", Some(" "), None).await.unwrap();
        assert_eq!(r.key.as_deref(), Some("K"));
        assert_eq!(health(&base, "").await.unwrap().status, "ok");

        let reqs = served.join().unwrap();
        assert!(reqs[0].starts_with("POST /v3/redeem "), "{}", reqs[0]);
        assert!(header(&reqs[0], "authorization").is_none());
        // Blank optionals are dropped, and the code is trimmed.
        assert!(reqs[0].contains(r#""code":"CODE""#), "{}", reqs[0]);
        assert!(!reqs[0].contains("email"), "{}", reqs[0]);
        assert!(reqs[1].starts_with("GET /healthz "), "{}", reqs[1]);
        assert!(header(&reqs[1], "authorization").is_none());
    }

    /// End-to-end through a mock SOCKS5 proxy: greeting → no-auth → CONNECT
    /// (domain ATYP, i.e. DNS resolved BY the proxy — `socks5h`) → success →
    /// plain HTTP inside the tunnel. Locks in that socks support is actually
    /// compiled into reqwest — without the `socks` feature this whole path
    /// fails at `Proxy::all` and the setting would silently be unusable.
    #[tokio::test]
    async fn socks5h_proxy_tunnels_the_request_and_resolves_dns_remotely() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let served = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let n = s.read(&mut buf).unwrap();
            assert!(
                n >= 2 && buf[0] == 0x05,
                "not a SOCKS5 greeting: {:?}",
                &buf[..n]
            );
            s.write_all(&[0x05, 0x00]).unwrap(); // no-auth accepted
            let n = s.read(&mut buf).unwrap();
            assert!(n >= 4 && buf[1] == 0x01, "not a CONNECT: {:?}", &buf[..n]);
            // ATYP 0x03 = domain name: the client did NOT resolve locally.
            assert_eq!(buf[3], 0x03, "expected domain ATYP, got {:#x}", buf[3]);
            s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .unwrap();
            // From here the socket IS the tunnel — speak plain HTTP on it.
            let mut req = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = s.read(&mut tmp).unwrap();
                req.extend_from_slice(&tmp[..n]);
                if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"status":"ok","queue_depth":0,"workers_alive":true}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(resp.as_bytes()).unwrap();
            String::from_utf8_lossy(&req).to_string()
        });
        // The target host doesn't exist — the request can only succeed by
        // going through the mock proxy.
        let h = health("http://ot.invalid:8080", &format!("socks5h://{addr}"))
            .await
            .unwrap();
        assert_eq!(h.status, "ok");
        let req = served.join().unwrap();
        assert!(req.starts_with("GET /healthz HTTP/1.1"), "{req}");
    }

    /// Plain HTTP proxying is the same request with an absolute URI in the
    /// request line — `mock_http` can play the proxy directly.
    #[tokio::test]
    async fn http_proxy_receives_the_absolute_form_request() {
        let (proxy_base, served) = mock_http(vec![(
            "200 OK",
            r#"{"status":"ok","queue_depth":0,"workers_alive":true}"#.into(),
        )]);
        let h = health("http://ot.invalid:8080", &proxy_base).await.unwrap();
        assert_eq!(h.status, "ok");
        let reqs = served.join().unwrap();
        assert!(
            reqs[0].starts_with("GET http://ot.invalid:8080/healthz"),
            "expected an absolute-form request line, got: {}",
            reqs[0]
        );
    }

    /// `/v3/key` now reports the review quota; older servers omit the fields
    /// and must parse with 0 (⇒ "no review access") rather than fail.
    #[test]
    fn key_status_parses_review_quota_and_defaults_it() {
        let raw = r#"{"plan":"pro","expires_at":"2026-08-01 00:00:00","usage_today":1423,"rpd":200000,"rpm":240.0,"topk":5,"reviews_today":2,"reviews_per_day":10}"#;
        let k: KeyStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(k.reviews_today, 2);
        assert_eq!(k.reviews_per_day, 10);

        let raw =
            r#"{"plan":"basic","expires_at":"x","usage_today":3,"rpd":6000,"rpm":10.0,"topk":3}"#;
        let k: KeyStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(k.reviews_today, 0);
        assert_eq!(k.reviews_per_day, 0);
    }

    #[test]
    fn share_entry_and_job_status_parse_the_documented_shapes() {
        let raw = r#"{"shares":[{"share_id":"aZ3kQ9mB2xLp","review_id":"9f2c","created_at":"2026-08-24T07:12:03Z","anonymized":true,"model":"4p-ot2","player_id":2,"summary":{"n_decisions":161,"n_match":128,"match_rate":0.795,"avg_actual_prob":0.62}}]}"#;
        #[derive(Deserialize)]
        struct Wrap {
            shares: Vec<ShareEntry>,
        }
        let w: Wrap = serde_json::from_str(raw).unwrap();
        let s = &w.shares[0];
        assert_eq!(s.share_id, "aZ3kQ9mB2xLp");
        assert_eq!(s.review_id, "9f2c");
        assert_eq!(s.player_id, Some(2));
        assert_eq!(s.summary.as_ref().unwrap().n_decisions, 161);

        // The poll's four states, incl. the revoked-share null.
        let st: ReviewJobStatus =
            serde_json::from_str(r#"{"status":"queued","progress":0.0}"#).unwrap();
        assert_eq!(st.status, "queued");
        let st: ReviewJobStatus =
            serde_json::from_str(r#"{"status":"failed","error":"could not process event stream"}"#)
                .unwrap();
        assert!(st.error.is_some());
        let st: ReviewJobStatus =
            serde_json::from_str(r#"{"status":"done","share_id":null,"url":null}"#).unwrap();
        assert_eq!(st.status, "done");
        assert!(st.share_id.is_none());
    }

    /// Ids are interpolated into URL paths: anything but plain ASCII
    /// alphanumerics must be rejected before a request is made, or a crafted
    /// id could address a different route (`../`, `?`, `#`).
    #[tokio::test]
    async fn path_ids_are_validated_before_any_request() {
        // Unreachable base: if validation let the id through, the call would
        // fail with a *connect* error instead of "invalid".
        let client = ApiClient::new(crate::bot::test_http::UNREACHABLE_BASE_URL, "K", "").unwrap();
        for bad in ["", "  ", "a/b", "..", "x?y", "x#y", "id%2e%2e"] {
            for err in [
                client.review_status(bad).await.unwrap_err(),
                client.review_share(bad).await.unwrap_err(),
                client.revoke_share(bad).await.unwrap_err(),
            ] {
                let msg = format!("{err:#}");
                assert!(msg.contains("invalid"), "id {bad:?} gave: {msg}");
            }
        }
        // A trimmed valid id passes validation (and then fails on connect).
        let err = client.review_status(" 3f2a9c ").await.unwrap_err();
        assert!(
            !format!("{err:#}").contains("invalid"),
            "trimmed alnum id must pass validation"
        );
    }

    #[test]
    fn gzip_round_trips() {
        use std::io::Read;
        let raw = br#"{"player_id":2,"events":[{"type":"start_game"}]}"#;
        let gz = gzip(raw).unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b], "missing gzip magic");
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, raw);
    }

    /// The review submit must go out gzipped (the server auto-detects the
    /// magic bytes) with the bearer header; the plaintext JSON must never be
    /// on the wire.
    #[tokio::test]
    async fn submit_review_gzips_the_body_and_authenticates() {
        let (base, served) = mock_http(vec![(
            "202 Accepted",
            r#"{"review_id":"3f2a9c","status":"queued"}"#.into(),
        )]);
        let client = ApiClient::new(&base, "SECRETKEY", "").unwrap();
        let resp = client
            .submit_review(
                Some("4p-ot2"),
                Some(2),
                vec![serde_json::json!({"type": "start_game"})],
            )
            .await
            .unwrap();
        assert_eq!(resp.review_id, "3f2a9c");
        assert_eq!(resp.status, "queued");

        let reqs = served.join().unwrap();
        let r = &reqs[0];
        assert!(r.starts_with("POST /v3/review "), "wrong request line: {r}");
        assert_eq!(header(r, "authorization"), Some("Bearer SECRETKEY"));
        assert!(
            !r.contains(r#""events""#) && !r.contains(r#""player_id""#),
            "body must be gzipped, not plaintext JSON: {r}"
        );
    }

    #[tokio::test]
    async fn review_status_share_revoke_and_shares_hit_the_expected_routes() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"status":"done","share_id":"aZ3kQ9mB2xLp","url":"https://viewer/preview/s/aZ3kQ9mB2xLp"}"#.into(),
            ),
            (
                "201 Created",
                r#"{"share_id":"bY4lR0nC3yMq","url":"https://viewer/preview/s/bY4lR0nC3yMq","created_at":"2026-08-24T07:12:03Z","anonymized":true}"#.into(),
            ),
            ("200 OK", r#"{"shares":[]}"#.into()),
            ("204 No Content", String::new()),
        ]);
        let client = ApiClient::new(&base, "SECRETKEY", "").unwrap();

        let st = client.review_status("3f2a9c").await.unwrap();
        assert_eq!(st.share_id.as_deref(), Some("aZ3kQ9mB2xLp"));
        let sh = client.review_share("3f2a9c").await.unwrap();
        assert_eq!(sh.share_id, "bY4lR0nC3yMq");
        assert!(client.shares().await.unwrap().is_empty());
        client.revoke_share("aZ3kQ9mB2xLp").await.unwrap();

        let reqs = served.join().unwrap();
        assert!(reqs[0].starts_with("GET /v3/review/3f2a9c "), "{}", reqs[0]);
        assert!(
            reqs[1].starts_with("POST /v3/review/3f2a9c/share "),
            "{}",
            reqs[1]
        );
        assert!(reqs[2].starts_with("GET /v3/shares "), "{}", reqs[2]);
        assert!(
            reqs[3].starts_with("DELETE /v3/shared/aZ3kQ9mB2xLp "),
            "{}",
            reqs[3]
        );
        for r in &reqs {
            assert_eq!(header(r, "authorization"), Some("Bearer SECRETKEY"));
        }
    }

    /// A non-2xx surfaces the server's `error` message and the `Retry-After` hint.
    #[tokio::test]
    async fn http_error_omits_untrusted_server_message() {
        let (base, served) = mock_http(vec![(
            "429 Too Many Requests",
            r#"{"error":"rate limited"}"#.into(),
        )]);
        let client = ApiClient::new(&base, "K", "").unwrap();
        let err = client.key_status().await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("429"), "{msg}");
        assert!(!msg.contains("rate limited"), "{msg}");
        let _ = served.join();
    }

    /// Manual smoke test against a **live** inference server — ignored by
    /// default, never run in CI. Opt in with env vars (the key is passed via
    /// env precisely so it never lands in a file):
    ///
    /// ```sh
    /// AKAGI_LIVE_BASE_URL=https://<host> \
    /// AKAGI_LIVE_API_KEY=<key> \
    /// AKAGI_LIVE_MJAI_LOG=/path/to/full-game.mjai.jsonl \  # optional
    /// AKAGI_LIVE_SEAT=0 \                                  # optional (default 0)
    /// AKAGI_LIVE_SUBMIT=1 \                                # optional; submits!
    /// cargo test --lib live_review_smoke -- --ignored --nocapture
    /// ```
    ///
    /// Without `AKAGI_LIVE_SUBMIT=1` it only reads (`/v3/key`, `/v3/shares`).
    /// With it, the given mjai log (full-view or perspective; it is censored
    /// through `build_api_events` first, exactly like the app) is submitted
    /// as one review job — mind the server's **one submit per key per 10
    /// minutes** cooldown — then polled to completion, and the share link is
    /// revoked and re-issued to exercise the whole surface.
    #[tokio::test]
    #[ignore = "hits a live server; needs AKAGI_LIVE_* env vars"]
    async fn live_review_smoke() {
        let base = match std::env::var("AKAGI_LIVE_BASE_URL") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                eprintln!("AKAGI_LIVE_BASE_URL not set; skipping");
                return;
            }
        };
        let key = std::env::var("AKAGI_LIVE_API_KEY").expect("AKAGI_LIVE_API_KEY not set");
        let client = ApiClient::new(&base, &key, "").unwrap();

        let st = client.key_status().await.expect("GET /v3/key");
        println!(
            "key: plan={} expires={} reviews_today={}/{} topk={}",
            st.plan, st.expires_at, st.reviews_today, st.reviews_per_day, st.topk
        );

        let shares = client.shares().await.expect("GET /v3/shares");
        println!("shares: {}", shares.len());
        for s in &shares {
            println!(
                "  {} review={} created={} model={:?}",
                s.share_id, s.review_id, s.created_at, s.model
            );
        }

        if std::env::var("AKAGI_LIVE_SUBMIT").ok().as_deref() != Some("1") {
            println!("AKAGI_LIVE_SUBMIT != 1 — read-only smoke done");
            return;
        }
        let log_path = std::env::var("AKAGI_LIVE_MJAI_LOG").expect("AKAGI_LIVE_MJAI_LOG not set");
        let seat: u8 = std::env::var("AKAGI_LIVE_SEAT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let raw = std::fs::read_to_string(&log_path).expect("read mjai log");
        let events: Vec<crate::schema::MjaiEvent> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("parse mjai line"))
            .collect();
        let num_players = events
            .iter()
            .find_map(|e| match e {
                crate::schema::MjaiEvent::StartGame { num_players, .. } => Some(*num_players),
                _ => None,
            })
            .unwrap_or(4);
        let api_events = crate::bot::native::build_api_events(&events, seat, num_players);
        println!(
            "submitting {} events, seat {seat}, {num_players}p",
            api_events.len()
        );

        let submitted = client
            .submit_review(None, Some(seat), api_events)
            .await
            .expect("POST /v3/review");
        println!(
            "review_id={} status={}",
            submitted.review_id, submitted.status
        );

        let mut url = None;
        let mut share_id = None;
        for i in 0..180 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let st = client
                .review_status(&submitted.review_id)
                .await
                .expect("poll review");
            println!("poll {i}: status={} progress={:?}", st.status, st.progress);
            match st.status.as_str() {
                "done" => {
                    url = st.url;
                    share_id = st.share_id;
                    break;
                }
                "failed" => panic!("review failed: {:?}", st.error),
                _ => {}
            }
        }
        let share_id = share_id.expect("review did not finish in time");
        println!("done: url={url:?}");

        // Revoke, then re-issue: the fresh link must carry a NEW id.
        client.revoke_share(&share_id).await.expect("revoke");
        println!("revoked {share_id}");
        let reissued = client
            .review_share(&submitted.review_id)
            .await
            .expect("re-share");
        println!("re-issued: {} -> {}", reissued.share_id, reissued.url);
        assert_ne!(reissued.share_id, share_id, "revoked id must not come back");

        let shares = client.shares().await.expect("GET /v3/shares (after)");
        assert!(
            shares.iter().any(|s| s.share_id == reissued.share_id),
            "re-issued share must appear in the listing"
        );
        println!("live smoke OK");
    }
}
