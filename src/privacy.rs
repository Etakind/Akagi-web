//! Privacy applies to observation copies only, never live protocol data.
use crate::schema::{FrameRaw, InspectorEntry};
use serde_json::json;

pub const OMITTED: &str = "omitted by privacy policy";

pub fn url(input: &str) -> String {
    match reqwest::Url::parse(input) {
        Ok(mut u) if matches!(u.scheme(), "http" | "https" | "ws" | "wss") => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        // CONNECT authorities are not URLs. Do not echo arbitrary input.
        _ => "[endpoint omitted]".into(),
    }
}

pub fn inspector(mut entry: InspectorEntry) -> InspectorEntry {
    match &mut entry {
        InspectorEntry::WsFrame { raw, parsed, .. } => {
            *raw = FrameRaw::Redacted(OMITTED.into());
            if let Some(p) = parsed {
                let id = p.args.get("msg_id").filter(|v| v.is_number()).cloned();
                // Protocol identifiers are locally known; unknown strings could
                // be attacker-controlled and contain credentials.
                p.method = safe_method(&p.method);
                let kind = p
                    .args
                    .get("kind")
                    .or_else(|| p.args.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let kind = match kind.as_str() {
                    "request" => "request",
                    "response" => "response",
                    "notify" | "notification" => "notify",
                    _ => "unknown",
                };
                p.args = json!({"msg_id": id, "kind": kind});
            }
        }
        InspectorEntry::Http { exchange, .. } => {
            exchange.url = url(&exchange.url);
            exchange.headers.retain(|h| {
                matches!(
                    h.name.to_ascii_lowercase().as_str(),
                    "content-type" | "content-length"
                )
            });
            for h in &mut exchange.headers {
                h.value = if h.name.eq_ignore_ascii_case("content-length") {
                    h.value
                        .parse::<u64>()
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                } else {
                    // Do not retain arbitrary header parameters / boundaries.
                    match h.value.split(';').next().unwrap_or("").trim() {
                        "application/json" => "application/json",
                        "text/html" => "text/html",
                        "text/plain" => "text/plain",
                        "application/octet-stream" => "application/octet-stream",
                        _ => "[type omitted]",
                    }
                    .into()
                };
            }
            if let Some(body) = &mut exchange.body {
                body.text = None;
                body.skipped = Some(OMITTED.into());
            }
            for annotation in &mut exchange.annotations {
                annotation.summary = "HTTP observation; details omitted".into();
                annotation.data = json!({});
            }
        }
        _ => {}
    }
    entry
}

pub fn safe_method(method: &str) -> String {
    // Exact local schema membership, not a prefix test on attacker input.
    if crate::bridge::majsoul::parser::ROUTES.get(method).is_some()
        || method == ".lq.ActionPrototype"
        || method == ".lq.NotifyGameEndResult"
    {
        method.into()
    } else {
        "protocol_message".into()
    }
}

/// Free-form diagnostics from I/O libraries can contain entire frames, URLs,
/// error response bodies or credentials. Their text is never a safe log API.
pub fn sensitive_target(target: &str) -> bool {
    !target.starts_with("akagi::")
        || [
            "akagi::bridge",
            "akagi::capture",
            "akagi::proxy",
            "akagi::bot",
            "akagi::github",
            "akagi::ipc",
            "akagi::config",
            "akagi::autoplay",
            "akagi::updater",
            "akagi::logger::flow",
            "akagi::logger::binary",
        ]
        .iter()
        .any(|prefix| target.starts_with(prefix))
}
