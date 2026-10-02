//! Generic HTTP capture — the policy both backends share.
//!
//! Akagi recorded WebSocket frames and threw away everything else, which
//! meant every HTTP request the game made was invisible: route topology,
//! version and CDN endpoints, and the analytics beacons through which the
//! client reports on itself. This module is the other half of that: what
//! to keep, what to skip, and how to say so.
//!
//! Nothing here understands any protocol above HTTP. Meaning is added
//! separately by [`crate::inspector::annotate`], which keeps capture
//! platform- and vendor-agnostic by construction.
//!
//! ## Bodies
//!
//! Raw body capture is disabled, including for legacy `bodies = true`.
//! The forwarding path therefore never buffers a body for logging.
//!
//! ## Pairing
//!
//! `hudsucker`'s `HttpContext` carries only the client's socket address —
//! no request identifier. Over HTTP/1.x that is enough: one request is in
//! flight per connection at a time, so a FIFO queue per client address
//! pairs a response with its request exactly. Over HTTP/2 it is not —
//! many streams share one connection and responses may interleave — so
//! [`ExchangePairing::close`] refuses to guess and the response is
//! recorded unpaired. The chromium backend has no such problem; CDP hands
//! out a real request id.
//!
//! **Only queue a request that will actually produce a response here.**
//! hudsucker answers a `CONNECT` from `process_connect`, a WebSocket
//! upgrade from `upgrade_websocket`, and a failed forward from
//! `handle_error` — none of which call the response hook. Queueing one of
//! those leaves an entry nothing ever claims, and every later response on
//! that connection is attributed to the request before it. A live capture
//! caught exactly that: JSON responses recorded as `CONNECT`, which has no
//! body at all. The proxy handler decides what may be queued; see
//! `will_be_answered_by_handle_response` there.

use crate::schema::{HttpBody, HttpHeader};
use http::{HeaderMap, HeaderValue};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// How much of an exchange to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpCapturePolicy {
    /// Record every intercepted exchange. When `false`, only exchanges a
    /// recognizer annotated are recorded — the interesting traffic stays
    /// visible without the session file accumulating everything else
    /// (including credentials) by default.
    pub record_all: bool,
    /// Buffer and keep textual bodies.
    pub bodies: bool,
    pub max_body_bytes: usize,
}

impl Default for HttpCapturePolicy {
    fn default() -> Self {
        Self {
            record_all: false,
            bodies: false,
            max_body_bytes: 256 * 1024,
        }
    }
}

/// Outcome of the header-only body decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPlan {
    /// There is no body to speak of.
    None,
    /// Buffer it; `len` bytes are expected.
    Capture { len: usize },
    /// Do not touch it. Carries the reason and the length if known.
    Skip { reason: String, len: Option<usize> },
}

impl BodyPlan {
    /// The record to store when a body is not being captured.
    pub fn into_skipped(self) -> Option<HttpBody> {
        match self {
            BodyPlan::None => None,
            BodyPlan::Capture { len } => Some(HttpBody {
                text: None,
                bytes: Some(len),
                skipped: Some("body capture failed".to_string()),
            }),
            BodyPlan::Skip { reason, len } => Some(HttpBody {
                text: None,
                bytes: len,
                skipped: Some(reason),
            }),
        }
    }
}

fn header_str<'a>(headers: &'a HeaderMap<HeaderValue>, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Decide what to do with a body, from headers alone.
pub fn plan_body(headers: &HeaderMap<HeaderValue>, _policy: &HttpCapturePolicy) -> BodyPlan {
    let len = header_str(headers, "content-length").and_then(|v| v.parse::<usize>().ok());
    if len == Some(0) || (len.is_none() && !headers.contains_key("transfer-encoding")) {
        BodyPlan::None
    } else {
        BodyPlan::Skip {
            reason: crate::privacy::OMITTED.into(),
            len,
        }
    }
}

pub fn body_record(bytes: &[u8], _headers: &HeaderMap<HeaderValue>) -> HttpBody {
    HttpBody {
        text: None,
        bytes: Some(bytes.len()),
        skipped: Some(crate::privacy::OMITTED.into()),
    }
}

/// Headers in wire order. Order is kept because it is a client
/// fingerprint in its own right, and a normalized view would erase it.
pub fn headers_of(headers: &HeaderMap<HeaderValue>) -> Vec<HttpHeader> {
    headers
        .iter()
        .map(|(name, value)| HttpHeader {
            name: name.as_str().to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect()
}

/// The in-flight request bookkeeping described in the module docs.
#[derive(Default)]
pub struct ExchangePairing {
    next_id: AtomicU64,
    inflight: Mutex<HashMap<SocketAddr, VecDeque<InFlight>>>,
}

/// What a response needs to know about the request it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub id: String,
    pub method: String,
    pub url: String,
    pub host: String,
}

impl ExchangePairing {
    /// Register a request and get its exchange id.
    pub fn open(&self, client: SocketAddr, method: &str, url: &str, host: &str) -> String {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let entry = InFlight {
            id: id.clone(),
            method: method.to_string(),
            url: url.to_string(),
            host: host.to_string(),
        };
        self.inflight
            .lock()
            .expect("inflight mutex poisoned")
            .entry(client)
            .or_default()
            .push_back(entry);
        id
    }

    /// Claim the request this response answers.
    ///
    /// `multiplexed` must be true for HTTP/2 and later, where FIFO order
    /// says nothing about which stream a response belongs to. In that
    /// case nothing is claimed and `None` comes back — an unattributed
    /// response is honest, a wrongly attributed one is not.
    pub fn close(&self, client: SocketAddr, multiplexed: bool) -> Option<InFlight> {
        if multiplexed {
            return None;
        }
        let mut map = self.inflight.lock().expect("inflight mutex poisoned");
        let queue = map.get_mut(&client)?;
        let entry = queue.pop_front();
        if queue.is_empty() {
            map.remove(&client);
        }
        entry
    }

    /// Drop any state for a closed connection so long sessions don't leak
    /// entries for requests that never got a response.
    pub fn forget(&self, client: SocketAddr) {
        self.inflight
            .lock()
            .expect("inflight mutex poisoned")
            .remove(&client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap<HeaderValue> {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn legacy_body_capture_never_buffers_secrets() {
        let h = headers(&[
            ("content-type", "application/json"),
            ("content-length", "42"),
        ]);
        let p = HttpCapturePolicy {
            record_all: true,
            bodies: true,
            max_body_bytes: 1024,
        };
        assert!(matches!(
            plan_body(&h, &p),
            BodyPlan::Skip { len: Some(42), .. }
        ));
        assert!(body_record(b"secret", &h).text.is_none());
        assert_eq!(plan_body(&headers(&[]), &p), BodyPlan::None);
        assert!(matches!(
            plan_body(&headers(&[("transfer-encoding", "chunked")]), &p),
            BodyPlan::Skip { .. }
        ));
    }

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    #[test]
    fn http1_pairing_is_fifo_per_connection() {
        let p = ExchangePairing::default();
        let a = addr(1);
        let b = addr(2);
        let id_a1 = p.open(a, "GET", "http://x/1", "x");
        let id_b1 = p.open(b, "GET", "http://y/1", "y");
        let id_a2 = p.open(a, "GET", "http://x/2", "x");

        // Each connection has its own queue.
        assert_eq!(p.close(a, false).unwrap().id, id_a1);
        assert_eq!(p.close(b, false).unwrap().id, id_b1);
        assert_eq!(p.close(a, false).unwrap().id, id_a2);
        assert!(p.close(a, false).is_none());
    }

    #[test]
    fn paired_response_learns_the_request_it_answers() {
        let p = ExchangePairing::default();
        let a = addr(3);
        p.open(a, "POST", "https://x/api", "x");
        let claimed = p.close(a, false).expect("should pair");
        assert_eq!(claimed.method, "POST");
        assert_eq!(claimed.url, "https://x/api");
        assert_eq!(claimed.host, "x");
    }

    /// The reason this is not a plain FIFO: over HTTP/2 many requests
    /// share one connection and responses interleave, so the front of the
    /// queue is not the request being answered. Guessing would attribute
    /// a response to the wrong URL, which is worse than not attributing it.
    #[test]
    fn multiplexed_responses_are_left_unpaired() {
        let p = ExchangePairing::default();
        let a = addr(4);
        p.open(a, "GET", "http://x/1", "x");
        assert!(p.close(a, true).is_none());
        // …and the request is still pending, not consumed by the attempt.
        assert!(p.close(a, false).is_some());
    }

    #[test]
    fn forgetting_a_connection_drops_its_pending_requests() {
        let p = ExchangePairing::default();
        let a = addr(5);
        p.open(a, "GET", "http://x/1", "x");
        p.forget(a);
        assert!(p.close(a, false).is_none());
    }
}
