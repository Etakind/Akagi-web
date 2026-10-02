//! With `block_telemetry` on, an Aliyun SLS web-tracking beacon must be
//! answered by the proxy itself and never reach the upstream — while
//! ordinary traffic on the same proxy still forwards untouched.
//!
//! The stub upstream returns a distinctive body (`UPSTREAM_BODY`). A
//! forwarded request carries that body back to the client; a blocked one
//! cannot, because it never reached the upstream. That difference is the
//! whole proof, so no request counter is needed. Its own binary because
//! `Session::init` installs a process-global tracing subscriber — see
//! `tests/common/mod.rs`.

mod common;

use akagi::config::HttpCaptureConfig;
use common::{get_through_proxy, Harness, BEACON_QUERY, UPSTREAM_BODY};

#[tokio::test(flavor = "multi_thread")]
async fn a_beacon_is_blocked_while_ordinary_traffic_forwards() {
    let h = Harness::start_with(HttpCaptureConfig::default(), true).await;

    // The beacon: recognized by path, so it must be dropped. The client
    // still gets a 200 (the real endpoint answers the same), but the
    // upstream body cannot be there because the upstream never saw it.
    let beacon = h.url(&format!("/logstores/client/track?{BEACON_QUERY}"));
    let blocked = get_through_proxy(h.proxy_port, &beacon, &h.host()).await;
    assert!(
        blocked.contains("200 OK"),
        "a blocked beacon should still be answered 200: {blocked}"
    );
    assert!(
        !blocked.contains(UPSTREAM_BODY),
        "a blocked beacon must be answered locally, not from upstream: {blocked}"
    );

    // The control: an ordinary request down the same proxy must still be
    // forwarded and come back with the upstream's body. This is what rules
    // out "the upstream was simply unreachable" as the reason the beacon
    // body was missing.
    let routes = h.url("/api/clientgate/routes?platform=Steam_Win");
    let forwarded = get_through_proxy(h.proxy_port, &routes, &h.host()).await;
    assert!(
        forwarded.contains(UPSTREAM_BODY),
        "ordinary traffic must still forward untouched: {forwarded}"
    );

    // The drop is recorded, not silent: the beacon is on the timeline with
    // both its own `sls_beacon` annotation (so we still see what it was) and
    // the `akagi_blocked` marker (so the action is visible).
    let entries = h.finish().await;
    let beacon_row = entries
        .iter()
        .find(|e| e["kind"] == "http" && e["url"].as_str().is_some_and(|u| u.contains("/track")))
        .expect("the blocked beacon must be on the timeline");

    let kinds: Vec<&str> = beacon_row["annotations"]
        .as_array()
        .expect("annotations array")
        .iter()
        .map(|a| a["kind"].as_str().unwrap_or_default())
        .collect();
    assert!(
        kinds.contains(&"sls_beacon"),
        "the beacon's own annotation must survive so we can see what was dropped: {beacon_row}"
    );
    assert!(
        kinds.contains(&"akagi_blocked"),
        "the drop must announce itself with an akagi_blocked annotation: {beacon_row}"
    );

    // A dropped request is answered locally, so it never pairs with an
    // upstream response.
    assert_eq!(beacon_row["phase"], "request");
    assert_eq!(beacon_row["status"], serde_json::Value::Null);
}
