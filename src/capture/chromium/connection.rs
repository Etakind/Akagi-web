//! Handshake diagnostics must distinguish rejection from game-page discovery.
//! Browser approval is per connection. Never retry a 403 automatically or
//! forward the endpoint, response body or arbitrary error into notifications.
use crate::{event_bus::NotifyBus, schema::Notification};
use anyhow::Result;
use chromiumoxide::{error::CdpError, Browser, Handler};
use std::time::Duration;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(120);
const TIMEOUT: &str = "CDP_APPROVAL_TIMEOUT: browser connection timed out. Keep the Edge/Chrome window open and approve its current debugging request. Then use Settings > Capture > Restart.";
const REJECTED: &str = "CDP_APPROVAL_REJECTED: Edge/Chrome rejected this debugging connection (HTTP 403). A previous approval does not approve a new connection. Keep its window open, use Settings > Capture > Restart once, and allow the new browser prompt. If rejection persists after Allow, toggle remote debugging off/on in the browser and retry.";
const FORBIDDEN: &str = "CDP_FORBIDDEN: browser refused this debugging connection (HTTP 403). Check the current browser authorization, then use Settings > Capture > Restart. The response contained no recognized rejection reason.";
const ORIGIN: &str = "CDP_ORIGIN_REJECTED: browser rejected the debugging client's Origin (HTTP 403). Check the client or local intermediary; do not disable browser origin checks.";

#[derive(Debug)]
pub(super) struct Disconnected;
impl std::fmt::Display for Disconnected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CDP_DISCONNECTED: browser transport or discovery stopped responding")
    }
}
impl std::error::Error for Disconnected {}

#[derive(Debug)]
struct HandshakeFailure(&'static str);
impl std::fmt::Display for HandshakeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for HandshakeFailure {}

pub(super) fn retry_allowed(error: &anyhow::Error, retries: u8) -> bool {
    retries < 3
        && (error.is::<Disconnected>()
            || error.downcast_ref::<HandshakeFailure>().is_some_and(|e| {
                e.0.starts_with("CDP_UNREACHABLE:") || e.0.starts_with("CDP_ENDPOINT_EXPIRED:")
            }))
}

fn failure_message(error: &CdpError) -> &'static str {
    match error {
        CdpError::Ws(tungstenite::Error::Http(response)) => match response.status().as_u16() {
            403 => match response.body().as_deref() {
                Some(b"Connection rejected") => REJECTED,
                Some(body) if body.starts_with(b"Rejected an incoming WebSocket connection from the ") => ORIGIN,
                _ => FORBIDDEN,
            },
            404 => "CDP_ENDPOINT_EXPIRED: browser debugging address is no longer available (HTTP 404). Use Settings > Capture > Restart to read its current address.",
            _ => "CDP_HANDSHAKE_HTTP_ERROR: browser refused the debugging handshake. Check the local debugging service, then restart capture.",
        },
        CdpError::Io(_) | CdpError::Ws(tungstenite::Error::Io(_)) =>
            "CDP_UNREACHABLE: cannot reach the local browser debugging service. Start Edge/Chrome, enable its loopback debugging service, then restart capture.",
        _ => "CDP_HANDSHAKE_FAILED: browser debugging handshake failed. Check its debugging service and current authorization, then restart capture.",
    }
}

pub(super) async fn connect(
    endpoint: &str,
    notify: &NotifyBus,
    attached: bool,
) -> Result<(Browser, Handler)> {
    connect_with_timeout(endpoint, notify, attached, HANDSHAKE_TIMEOUT).await
}

async fn connect_with_timeout(
    endpoint: &str,
    notify: &NotifyBus,
    attached: bool,
    timeout: Duration,
) -> Result<(Browser, Handler)> {
    if attached {
        let _ = notify.send(Notification::info("Waiting for browser connection")
            .body("Keep Edge/Chrome open. If it asks to allow debugging, approve this connection; earlier approvals may not carry over.")
            .sticky().id("cdp-connection"));
    }
    let message = match tokio::time::timeout(timeout, Browser::connect(endpoint)).await {
        Ok(Ok(connection)) => {
            tracing::info!(cdp_connected = true, "Browser handshake completed");
            if attached {
                let _ = notify.send(Notification::info("Browser connected; waiting for Majsoul")
                    .body("Open https://game.maj-soul.com/1/ in this browser. Capture attaches automatically.")
                    .id("cdp-connection"));
            }
            return Ok(connection);
        }
        Ok(Err(error)) => failure_message(&error),
        Err(_) => TIMEOUT,
    };
    tracing::warn!(
        cdp_connected = false,
        approval_rejected = message == REJECTED,
        origin_rejected = message == ORIGIN,
        forbidden_unknown_reason = message == FORBIDDEN,
        approval_timeout = message == TIMEOUT,
        "Browser handshake failed"
    );
    if attached {
        let _ = notify.send(
            Notification::error("Browser connection failed")
                .body(message)
                .id("cdp-connection"),
        );
    }
    Err(HandshakeFailure(message).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn retry_budget_never_retries_permission_rejection_or_approval_timeout() {
        for message in [REJECTED, FORBIDDEN, ORIGIN, TIMEOUT] {
            assert!(!retry_allowed(&HandshakeFailure(message).into(), 0));
        }
        assert!(retry_allowed(&Disconnected.into(), 0));
        assert!(!retry_allowed(&Disconnected.into(), 3));
        assert!(retry_allowed(
            &HandshakeFailure("CDP_UNREACHABLE: local service").into(),
            1
        ));
        assert!(!retry_allowed(&anyhow::anyhow!("unknown"), 0));
    }

    #[tokio::test]
    async fn rejected_handshake_is_not_retried_and_diagnostics_never_echo_payload() {
        for (status, body, code) in [
            (403, "Connection rejected", "CDP_APPROVAL_REJECTED"),
            (
                403,
                "Rejected an incoming WebSocket connection from the FAKE_SECRET origin.",
                "CDP_ORIGIN_REJECTED",
            ),
            (403, "FAKE_SECRET", "CDP_FORBIDDEN"),
            (404, "FAKE_SECRET", "CDP_ENDPOINT_EXPIRED"),
            (500, "FAKE_SECRET", "CDP_HANDSHAKE_HTTP_ERROR"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!(
                "ws://{}/devtools/browser/FAKE_ENDPOINT",
                listener.local_addr().unwrap()
            );
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                while !data.ends_with(b"\r\n\r\n") {
                    data.push(socket.read_u8().await.unwrap());
                }
                let request = String::from_utf8(data).unwrap().to_ascii_lowercase();
                assert!(!request.contains("\r\norigin:"));
                assert!(!request.contains("\r\ncookie:"));
                assert!(!request.contains("\r\nauthorization:"));
                let response = format!(
                    "HTTP/1.1 {status} Rejected\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                assert!(
                    tokio::time::timeout(Duration::from_millis(100), listener.accept())
                        .await
                        .is_err()
                );
            });
            let (notify, mut rx) = tokio::sync::broadcast::channel(8);
            let result =
                connect_with_timeout(&endpoint, &notify, true, Duration::from_secs(2)).await;
            let error = match result {
                Err(error) => format!("{error:#}"),
                Ok(_) => panic!("expected rejection"),
            };
            assert!(error.starts_with(code), "{error}");
            assert!(!error.contains("FAKE_"));
            let mut notices = 0;
            while let Ok(n) = rx.try_recv() {
                let serialized = serde_json::to_string(&n).unwrap();
                assert!(!serialized.contains("FAKE_"));
                assert!(!serialized.contains("Browser connected"));
                notices += 1;
            }
            assert_eq!(notices, 2);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn pending_approval_times_out_and_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/secret",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut request))
                .await
                .unwrap()
                .unwrap();
        });
        let (notify, _) = tokio::sync::broadcast::channel(8);
        let result =
            connect_with_timeout(&endpoint, &notify, true, Duration::from_millis(100)).await;
        assert!(matches!(result, Err(e) if e.to_string() == TIMEOUT));
        server.await.unwrap();
    }
}
