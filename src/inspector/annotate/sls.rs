//! Recognizer for Alibaba Cloud SLS **web tracking** beacons — the
//! channel through which game clients report on themselves.
//!
//! This is not Akagi's telemetry; Akagi has none. It is the game's, and
//! it matters because it is the one place the client describes its own
//! runtime to the operator: client build, machine, and — on standalone
//! (non-browser) builds — every TLS certificate it was served.
//!
//! ## Wire format
//!
//! ```text
//! GET https://<project>.<region>.log.aliyuncs.com/logstores/<store>/track?<payload>
//! ```
//!
//! The query string *is* the payload — no request body, and the response
//! is `200` with zero bytes. Recurring parameters: `log_category` (what
//! the beacon is about) and `content` (a JSON blob whose shape depends on
//! the category), plus a fixed set of client and machine descriptors.
//!
//! Two Mahjong Soul deployments have been captured and they differ in
//! more than the hostname: the project component of the host, the
//! `server` parameter (a **deployment discriminator, not a constant**),
//! the gateway naming in `connect_lobby`, and both version fields. What
//! does *not* change is the parameter list for a given category — the
//! same category was byte-identical in field set and field order across
//! both deployments and across two client versions, which is why
//! [`SlsBeacon::params`] keeps its order.
//!
//! ## Recognition
//!
//! Keyed on the **path shape**, not on a host allowlist: any host serving
//! `/logstores/<store>/track` is doing SLS web tracking. A new region, a
//! new project, or a second game therefore needs no code change — which
//! is not hypothetical, the second deployment was decoded without one.
//!

use crate::schema::HttpAnnotation;

use super::RequestView;
use http::Uri;
use serde::{Deserialize, Serialize};

/// Stable identifier for this recognizer, as it appears on the wire and
/// in the UI.
pub const KIND: &str = "sls_beacon";

/// Path prefix of the SLS web-tracking API.
const LOGSTORE_PREFIX: &str = "/logstores/";

/// Endpoints under `/logstores/<store>/` that carry a beacon payload.
/// `track` is what the observed clients use; `track_ua.gif` is the same
/// API's image-beacon variant, accepted so a client that switches to it
/// does not silently vanish from the timeline.
const TRACK_ENDPOINTS: &[&str] = &["track", "track_ua.gif"];

/// One query-string parameter, in the order it appeared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlsParam {
    pub name: String,
    pub value: String,
}

/// A decoded beacon.
///
/// `params` and `content` are two readable views of the same bytes; the
/// exchange's own `url` field remains the ground truth, since parameter
/// order and percent-encoding are both part of what a beacon *is*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlsBeacon {
    /// SLS logstore from the path — `client`, `client-route-ab-test`, …
    pub logstore: String,
    /// The `log_category` parameter, lifted out because it is what the
    /// beacon is *about*. `None` if the beacon carried no such parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_category: Option<String>,
    pub params: Vec<SlsParam>,
    /// The `content` parameter parsed as JSON. `None` both when there is
    /// no `content` and when it does not parse — at least one category is
    /// built by string concatenation with unquoted values, so a parse
    /// failure is expected, not exceptional. The raw string stays in
    /// `params` either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

impl SlsBeacon {
    /// Value of a parameter, if present.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }
}

/// Recognizer entry point. See [`super::annotate_request`].
pub fn annotate(view: &RequestView<'_>) -> Option<HttpAnnotation> {
    let beacon = parse_url(view.url)?;
    let summary = match &beacon.log_category {
        Some(c) => format!("{}/{}", beacon.logstore, c),
        None => beacon.logstore.clone(),
    };
    Some(HttpAnnotation {
        kind: KIND.to_string(),
        summary,
        data: serde_json::to_value(&beacon).unwrap_or(serde_json::Value::Null),
    })
}

/// Decode `url` if it is an SLS web-tracking beacon.
pub fn parse_url(url: &str) -> Option<SlsBeacon> {
    let uri: Uri = url.parse().ok()?;
    parse_uri(&uri)
}

/// [`parse_url`] for call sites that already hold a parsed [`Uri`].
pub fn parse_uri(uri: &Uri) -> Option<SlsBeacon> {
    let logstore = logstore_of(uri.path())?;
    let query = uri.query().unwrap_or("");

    let params: Vec<SlsParam> = form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| SlsParam {
            name: name.into_owned(),
            value: value.into_owned(),
        })
        .collect();

    let find = |key: &str| {
        params
            .iter()
            .find(|p| p.name == key)
            .map(|p| p.value.as_str())
    };
    let log_category = find("log_category").map(str::to_owned);
    let content = find("content").and_then(|c| serde_json::from_str(c).ok());

    Some(SlsBeacon {
        logstore: logstore.to_owned(),
        log_category,
        params,
        content,
    })
}

/// The `<store>` in `/logstores/<store>/<endpoint>`, if the path is a
/// tracking endpoint at all.
fn logstore_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(LOGSTORE_PREFIX)?;
    let (store, endpoint) = rest.split_once('/')?;
    if store.is_empty() || !TRACK_ENDPOINTS.contains(&endpoint) {
        return None;
    }
    Some(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Identifiers here are fabricated. The *shapes* are real — parameter
    /// order, the nested-JSON `content`, the escaped forward slashes —
    /// because those are what the parser has to survive.
    const CERT_BEACON: &str = "https://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track?\
APIVersion=0.6.0&level=info&log_category=certificate_info&\
account_id=10000001&device_id=00000000-0000-4000-8000-000000000001&\
content=%5B%7B%22issuer%22%3A%22CN%3DExample%20CA%2C%20O%3DExample%22%2C%22version%22%3A3%2C\
%22subject%22%3A%22CN%3D*.example.com%22%2C%22url%22%3A%22wss%3A%5C%2F%5C%2Froute-2.example.com%5C%2Fgateway%22%7D%5D";

    #[test]
    fn certificate_beacon_is_decoded() {
        let b = parse_url(CERT_BEACON).expect("should recognize an SLS track URL");
        assert_eq!(b.logstore, "client");
        assert_eq!(b.log_category.as_deref(), Some("certificate_info"));

        // Order is preserved — it is part of the beacon's identity.
        let names: Vec<&str> = b.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "APIVersion",
                "level",
                "log_category",
                "account_id",
                "device_id",
                "content"
            ]
        );

        // The certificate report is the reason this recognizer exists: it
        // must come back structured, not as a percent-encoded blob.
        let content = b.content.expect("certificate_info content is valid JSON");
        let first = &content.as_array().expect("content is an array")[0];
        assert_eq!(first["issuer"], "CN=Example CA, O=Example");
        assert_eq!(first["subject"], "CN=*.example.com");
    }

    /// Regression guard for the one category whose `content` the game
    /// builds by concatenation, leaving values unquoted. It is not JSON,
    /// and the parser must keep the beacon rather than drop it.
    #[test]
    fn content_that_is_not_json_keeps_the_beacon() {
        let url = "https://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track?\
log_category=game_status&content=%7B%22type%22%3A%22unity_bundle_download%22%2C%22game_version%22%3A4.0.28%2C%22platform%22%3AStandaloneWindows%7D";
        let b = parse_url(url).expect("beacon should still be recognized");
        assert_eq!(b.log_category.as_deref(), Some("game_status"));
        assert!(b.content.is_none(), "unquoted values are not valid JSON");
        // …but the raw string survives in `params`, so nothing is lost.
        assert!(b
            .param("content")
            .unwrap()
            .contains("unity_bundle_download"));
    }

    #[test]
    fn second_logstore_is_reported_as_itself() {
        let url = "https://example-client.cn-hongkong.log.aliyuncs.com/logstores/client-route-ab-test/track?\
log_category=mj_connect_success&content=%7B%22time%22%3A1268%7D";
        let b = parse_url(url).unwrap();
        assert_eq!(b.logstore, "client-route-ab-test");
        assert_eq!(b.content.unwrap()["time"], 1268);
    }

    /// A second deployment, confirmed against a real capture: the host
    /// changes region *and* project, the versions lag, the gateway is a
    /// named host rather than a numbered route, and `server` is a
    /// **deployment discriminator, not a constant**.
    ///
    /// The parameter list — 18 fields, in exactly this order — was
    /// byte-identical to the other deployment's for the same category,
    /// across two client versions. That is why `params` is ordered.
    ///
    /// Identifiers below are fabricated; everything structural is real.
    #[test]
    fn second_deployment_is_recognized_with_its_own_server_id() {
        let url = "https://example2-client.ap-northeast-1.log.aliyuncs.com/logstores/client/track?\
APIVersion=0.6.0&session_id=00000000-0000-4000-8000-000000000002&account_id=10000002\
&log_category=game_status&level=info&server=2&app_runtime_id=00000000-0000-4000-8000-000000000003\
&device_model=Edge%20150.0.0.0&connect_lobby=gs.example.com:443&res_version=0.16.213\
&client_version=4.0.11&device_gpu_name=ANGLE%20(Example,%20Example%20GPU/PCIe/SSE2,%20OpenGL%20ES%203.2)\
&device_os=Unknown%20OS%20Unknown%20OS%20Version&channel=&device_id=00000000-0000-4000-8000-000000000004\
&content=%7B%22type%22:%22login_loading_end%22,%22load_time%22:4059,%22error_code%22:0%7D\
&client_type=web&device_type=pc";
        let b = parse_url(url).expect("a second deployment needs no code change");
        assert_eq!(b.logstore, "client");
        assert_eq!(b.log_category.as_deref(), Some("game_status"));
        assert_eq!(b.content.as_ref().unwrap()["type"], "login_loading_end");

        assert_eq!(b.param("server"), Some("2"));
        // Percent-decoding has to survive spaces, parentheses and commas —
        // `device_gpu_name` carries all three.
        assert_eq!(b.param("device_model"), Some("Edge 150.0.0.0"));
        assert!(b
            .param("device_gpu_name")
            .unwrap()
            .contains("OpenGL ES 3.2"));
        // An empty value is a value, not an absent key.
        assert_eq!(b.param("channel"), Some(""));

        assert_eq!(b.params.len(), 18);
        assert_eq!(b.params[0].name, "APIVersion");
        assert_eq!(b.params[1].name, "session_id");
        assert_eq!(b.params[17].name, "device_type");
    }

    #[test]
    fn image_beacon_variant_is_recognized() {
        assert!(parse_url(
            "http://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track_ua.gif?level=info"
        )
        .is_some());
    }

    #[test]
    fn plain_http_beacon_is_recognized() {
        // The startup beacons go out over plain HTTP, not TLS.
        let b = parse_url(
            "http://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track?log_category=game_status",
        )
        .unwrap();
        assert_eq!(b.log_category.as_deref(), Some("game_status"));
    }

    #[test]
    fn beacon_without_query_is_still_a_beacon() {
        let b =
            parse_url("https://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track")
                .unwrap();
        assert!(b.params.is_empty());
        assert!(b.log_category.is_none());
        assert!(b.content.is_none());
    }

    #[test]
    fn non_beacon_urls_are_ignored() {
        // Ordinary game traffic — by far the common case, and the reason
        // the check has to be cheap.
        for url in [
            "https://route-5.example.com/api/clientgate/routes?platform=Steam_Win",
            "https://example.com/logstores/client/index",
            "https://example.com/logstores//track",
            "https://example.com/logstores/client",
            "https://example.com/",
            "https://example.com/v1/logstores/client/track",
        ] {
            assert!(
                parse_url(url).is_none(),
                "{url} must not be treated as a beacon"
            );
        }
    }

    /// A CONNECT authority (`host:port`, no path) reaches the proxy
    /// handler before anything else does. It must not be mistaken for a
    /// beacon — and must not panic the parser.
    #[test]
    fn authority_form_uri_is_ignored() {
        assert!(parse_url("example-client.cn-hongkong.log.aliyuncs.com:443").is_none());
    }

    #[test]
    fn annotation_summary_names_store_and_category() {
        let view = RequestView::new("GET", CERT_BEACON, &[]);
        let a = annotate(&view).expect("beacon should be annotated");
        assert_eq!(a.kind, KIND);
        assert_eq!(a.summary, "client/certificate_info");
        assert_eq!(a.data["logstore"], "client");
    }
}
