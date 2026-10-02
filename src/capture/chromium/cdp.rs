//! Per-page CDP subscription that routes WebSocket frames into the
//! platform bridge.
//!
//! Why per-page (not browser-level): chromiumoxide 0.9.1 does not deliver
//! page-scoped events to `Browser::event_listener` even with
//! `Target.setAutoAttach { flatten: true }`. The events arrive on the
//! browser connection but stay tagged with the originating page session;
//! `Browser::event_listener` only surfaces browser-level events. The
//! canonical pattern (see `chromiumoxide-0.9.1/examples/interception.rs`)
//! is to grab a `Page` and call `page.event_listener::<E>()` on it.
//!
//! Subscription lifecycle:
//! - Poll browser target metadata every ~1s; never block on unrelated pages.
//! - On a new `target_id`: enable Network domain on that page, subscribe
//!   to the four WS events, spawn a routing task.
//! - On a `target_id` disappearing from the snapshot (tab closed):
//!   `JoinHandle::abort` the routing task and drop our entry.
//!
//! Service-worker WebSockets are not subscribed in v1 — Majsoul uses
//! page-scoped WS today. If real-world testing shows otherwise, expand
//! the polling to include `browser.targets()` and filter on type.

use crate::autoplay::AutoplayContext;
use crate::bridge::Direction;
use crate::capture::flow::FlowBridges;
use crate::config::HttpCaptureConfig;
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::inspector::annotate::{self, RequestView};
use crate::inspector::InspectorWriter;
use crate::schema::{
    CaptureSource, FrameDirection, FrameRaw, HttpBody, HttpExchange, HttpHeader, HttpPhase,
    InspectorEntry, Notification,
};
use anyhow::{Context, Result};
use base64::Engine;
use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams as NetworkEnableParams, EventRequestWillBeSent, EventResponseReceived,
    EventWebSocketClosed, EventWebSocketCreated, EventWebSocketFrameReceived,
    EventWebSocketFrameSent, Headers, ResourceType,
};
use chromiumoxide::page::Page;
use chrono::Local;
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

const PAGE_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Per-flow key for `FlowBridges`. `target` is the page that owns the
/// WebSocket; `request` is the CDP request id that the page assigned to
/// `new WebSocket(...)`. The pair is unique across the browser session
/// even when two tabs both open a connection to the same Majsoul host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub target: String,
    pub request: String,
}

fn decode_payload(b64: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .ok()
}

#[derive(Debug, PartialEq, Eq)]
enum FrameDecode {
    Bytes(Vec<u8>),
    Skip,
    BadBase64,
}

fn decode_frame_payload(opcode: i64, payload_data: &str) -> FrameDecode {
    match opcode {
        1 => FrameDecode::Bytes(payload_data.as_bytes().to_vec()),
        2 => match decode_payload(payload_data) {
            Some(b) => FrameDecode::Bytes(b),
            None => FrameDecode::BadBase64,
        },
        _ => FrameDecode::Skip,
    }
}

/// Compute the symmetric difference between the previous and current
/// page snapshots. Returns `(adds, removes)` — target ids to subscribe
/// and target ids whose subscription tasks should be reaped. Pure so
/// the diff logic is unit-testable independent of the CDP loop.
pub fn diff_pages(prev: &HashSet<String>, current: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let adds: Vec<_> = current.difference(prev).cloned().collect();
    let removes: Vec<_> = prev.difference(current).cloned().collect();
    (adds, removes)
}

/// Decide whether the autoplay page handle — currently owned by tab
/// `owner` (its `TargetId`, if any) — must be cleared when the page-poll
/// loop reaps the `removed` tabs this tick.
///
/// The handle tracks the browser **tab**, not any single WebSocket, so it
/// is cleared only when its owning tab disappears from the snapshot. This
/// is the crux of the "autoplay silently stops mid-game" fix: Majsoul
/// opens and closes many short-lived Route-probe / lobby-reconnect sockets
/// to `*.maj-soul.com` while a game runs on a separate `game-gateway`
/// socket, and those socket closures must **not** drop the handle. Pure so
/// the decision is unit-testable without a live `Page`.
pub fn page_handle_cleared_by_removal(owner: Option<&str>, removed: &[String]) -> bool {
    matches!(owner, Some(o) if removed.iter().any(|r| r == o))
}

const AUTOPLAY_HOST_HINTS: &[&str] = &["maj-soul.com", "mahjongsoul.com"];

fn is_autoplay_target_url(ws_url: &str) -> bool {
    AUTOPLAY_HOST_HINTS.iter().any(|h| ws_url.contains(h))
}

/// Run the CDP loop until the browser disconnects or an unrecoverable
/// error occurs. Frames flow through `bridges` into `mjai_bus`, and each
/// frame is also recorded into `inspector` for the Logs → Inspector tab.
///
/// `autoplay` is `Some` only on the chromium backend when the autoplay
/// feature is wired (`AppState.autoplay_context`). On Majsoul WS open
/// we publish the page handle into it; autoplay reads it back to dispatch
/// `Input.dispatchMouseEvent`. Passing `None` makes the loop bridge-only.
// Explicit shared buses/context mirror attach_page; no independent ownership.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    endpoint: &str,
    bridges: Arc<FlowBridges<FlowKey>>,
    mjai_bus: MjaiBus,
    inspector: InspectorWriter,
    autoplay: Option<Arc<AutoplayContext>>,
    http_cfg: HttpCaptureConfig,
    notify: NotifyBus,
    official_majsoul_only: bool,
) -> Result<()> {
    if let Some(ctx) = &autoplay {
        ctx.official_majsoul_only
            .store(official_majsoul_only, std::sync::atomic::Ordering::Relaxed);
        *ctx.page.write().await = None;
        *ctx.canvas_rect.write().await = None;
    }
    info!("CDP connection starting");
    let (browser_owned, mut handler) =
        super::connection::connect(endpoint, &notify, official_majsoul_only).await?;
    // `Browser` is not `Clone`; share via Arc for the page-poll task.
    let browser = Arc::new(browser_owned);

    // Pump the chromiumoxide handler — required so its internal
    // request/response oneshots resolve. The handler also surfaces
    // `WS Invalid message` warnings when Chrome sends events
    // chromiumoxide doesn't have a typed binding for; those are
    // non-fatal noise and the stream keeps running.
    let mut pump = AbortTask(tokio::spawn(async move {
        while let Some(ev) = handler.next().await {
            if let Err(e) = ev {
                debug!("chromiumoxide handler event error: {e:?}");
                break;
            }
        }
    }));

    // Per-page subscription registry. Key: TargetId stringified.
    let mut subscribed: HashMap<String, AbortTask> = HashMap::new();

    let poll_loop = async {
        let mut failed_discovery = 0u8;
        let mut unavailable_game = 0u8;
        let mut last_counts = None;
        loop {
            let snapshot = match super::discovery::snapshot(&browser, official_majsoul_only).await {
                Ok(snapshot) => {
                    failed_discovery = 0;
                    snapshot
                }
                Err(_) => {
                    failed_discovery += 1;
                    warn!(
                        discovery_failures = failed_discovery as u64,
                        "CDP discovery failed"
                    );
                    if failed_discovery >= 3 {
                        break;
                    }
                    tokio::time::sleep(PAGE_POLL_INTERVAL).await;
                    continue;
                }
            };
            let current = snapshot.current;
            let pages = snapshot.pages;
            let counts = (current.len(), pages.len());
            if last_counts != Some(counts) {
                info!(
                    game_targets = counts.0 as u64,
                    available_pages = counts.1 as u64,
                    "CDP discovery state"
                );
                last_counts = Some(counts);
            }
            // A routing task can end independently of the tab; subscribe again.
            subscribed.retain(|_, task| !task.0.is_finished());
            let prev: HashSet<String> = subscribed.keys().cloned().collect();
            let (adds, removes) = diff_pages(&prev, &current);

            // Reap closed tabs first so we don't leak resources during
            // long sessions where users open + close many tabs.
            for id in &removes {
                if let Some(h) = subscribed.remove(id) {
                    h.0.abort();
                    debug!("CDP: dropped subscription for closed target {id}");
                }
            }

            // The autoplay page handle tracks the browser *tab*, not any
            // single WebSocket, so it is cleared here — when its owning tab
            // is actually gone — rather than on `webSocketClosed`. Majsoul
            // opens and closes many short-lived Route-probe / lobby-reconnect
            // sockets to *.maj-soul.com while a game runs on a separate
            // game-gateway socket; clearing the handle on those closes was
            // silently stopping autoplay mid-game.
            if let (Some(ctx), false) = (&autoplay, removes.is_empty()) {
                // Hold the write lock across the check + clear so a
                // concurrent rebind from another tab's task can't slip
                // between reading the owner and nulling the handle.
                let mut guard = ctx.page.write().await;
                let owner = guard.as_ref().map(|p| p.target_id().inner().clone());
                if page_handle_cleared_by_removal(owner.as_deref(), &removes) {
                    *guard = None;
                    drop(guard);
                    *ctx.canvas_rect.write().await = None;
                    info!("autoplay: page handle cleared — owning Majsoul tab closed");
                }
            }

            // Subscribe new tabs.
            for page in &pages {
                let id = page.target_id().inner().clone();
                if !adds.contains(&id) {
                    continue;
                }
                match tokio::time::timeout(
                    Duration::from_secs(8),
                    attach_page(
                        page.clone(),
                        id.clone(),
                        bridges.clone(),
                        mjai_bus.clone(),
                        inspector.clone(),
                        autoplay.clone(),
                        http_cfg.clone(),
                        notify.clone(),
                        official_majsoul_only,
                    ),
                )
                .await
                {
                    Ok(Ok(handle)) => {
                        info!("CDP: attached to page target {id}");
                        if official_majsoul_only {
                            let _ = notify.send(
                                Notification::success("Majsoul capture attached")
                                    .body("Official game page connected. Local analysis is ready for a new game.")
                                    .id("cdp-connection"),
                            );
                        }
                        subscribed.insert(id, AbortTask(handle));
                    }
                    Ok(Err(_)) | Err(_) => {
                        warn!(
                            page_attach_failed = true,
                            "CDP page subscription unavailable; retrying"
                        );
                    }
                }
            }

            if official_majsoul_only {
                if let Some(ctx) = &autoplay {
                    // Do not depend on a future WebSocketCreated event, and
                    // never guess which of multiple game tabs should be clicked.
                    let desired = if current.len() == 1 {
                        pages
                            .iter()
                            .find(|p| subscribed.contains_key(p.target_id().inner()))
                            .cloned()
                    } else {
                        None
                    };
                    let mut bound = ctx.page.write().await;
                    if bound.as_ref().map(|p| p.session_id())
                        != desired.as_ref().map(|p| p.session_id())
                    {
                        *bound = desired;
                        *ctx.canvas_rect.write().await = None;
                        info!(
                            autoplay_page_bound = bound.is_some(),
                            "Official game input target updated"
                        );
                    }
                }
            }

            if official_majsoul_only && !current.is_empty() && subscribed.is_empty() {
                unavailable_game += 1;
                if unavailable_game >= 3 {
                    warn!(
                        game_page_unavailable = true,
                        "CDP game subscription stalled"
                    );
                    break;
                }
            } else {
                unavailable_game = 0;
            }

            tokio::time::sleep(PAGE_POLL_INTERVAL).await;
        }
        // unreachable, but type-check the future as `()` for select arm
        #[allow(unreachable_code)]
        ()
    };

    tokio::select! {
        _ = &mut pump.0 => info!("CDP handler pump exited"),
        _ = poll_loop => info!("CDP page poll exited"),
    }
    // Abort any still-live page subscriptions before tearing down.
    for (_id, h) in subscribed {
        h.0.abort();
    }
    if let Some(ctx) = &autoplay {
        *ctx.page.write().await = None;
        *ctx.canvas_rect.write().await = None;
    }
    drop(browser);
    Err(super::connection::Disconnected.into())
}

// Task cancellation must detach from a user-owned browser, even if the
// capture future is dropped while its child routing tasks are still running.
struct AbortTask(JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct GameReadiness {
    started: bool,
    ready: bool,
    warned: bool,
}
impl GameReadiness {
    fn observe(&mut self, result: &crate::bridge::ParseResult) -> Option<Notification> {
        use crate::schema::MjaiEvent;
        if !result
            .parsed
            .as_ref()
            .is_some_and(|p| p.method.starts_with(".lq."))
        {
            return None;
        }
        for event in &result.events {
            match event {
                MjaiEvent::StartGame { .. } => {
                    self.started = true;
                    self.ready = false;
                }
                MjaiEvent::StartKyoku { .. } if self.started && !self.ready => {
                    self.ready = true;
                    return Some(
                        Notification::success("Majsoul game state ready")
                            .body(
                                "Seat and starting hand received. Local game analysis can now run.",
                            )
                            .id("majsoul-game-state"),
                    );
                }
                _ => {}
            }
        }
        if !self.started
            && !self.warned
            && result
                .parsed
                .as_ref()
                .is_some_and(|p| p.method == ".lq.ActionPrototype")
        {
            self.warned = true;
            return Some(Notification::warn("Majsoul connected; game state missing")
                .body("Capture started after game entry. Keep Akagi running and refresh the game page when a brief reconnect is safe, so the game can restore your seat and hand.")
                .sticky().id("majsoul-game-state"));
        }
        None
    }
}
pub(super) fn official_majsoul_page(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("game.maj-soul.com")
        && url.path().starts_with("/1/")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

/// Enable Network on the page, subscribe to the four WS events, and
/// spawn a routing task. Returns the task handle so the poll loop can
/// abort it when the tab closes.
#[allow(clippy::too_many_arguments)]
async fn attach_page(
    page: Page,
    target_id: String,
    bridges: Arc<FlowBridges<FlowKey>>,
    mjai_bus: MjaiBus,
    inspector: InspectorWriter,
    autoplay: Option<Arc<AutoplayContext>>,
    http_cfg: HttpCaptureConfig,
    notify: NotifyBus,
    official_majsoul_only: bool,
) -> Result<JoinHandle<()>> {
    let mut on_created = page
        .event_listener::<EventWebSocketCreated>()
        .await
        .context("subscribe webSocketCreated")?;
    let mut on_recv = page
        .event_listener::<EventWebSocketFrameReceived>()
        .await
        .context("subscribe webSocketFrameReceived")?;
    let mut on_sent = page
        .event_listener::<EventWebSocketFrameSent>()
        .await
        .context("subscribe webSocketFrameSent")?;
    let mut on_closed = page
        .event_listener::<EventWebSocketClosed>()
        .await
        .context("subscribe webSocketClosed")?;
    // Every HTTP request the page makes, so we can pick the game's
    // analytics beacons out of it. `Network.enable` above already turns
    // this event on; the filtering is ours, in the select arm.
    let mut on_request = page
        .event_listener::<EventRequestWillBeSent>()
        .await
        .context("subscribe requestWillBeSent")?;
    let mut on_response = page
        .event_listener::<EventResponseReceived>()
        .await
        .context("subscribe responseReceived")?;

    // Install every listener before enabling traffic delivery.
    page.execute(NetworkEnableParams::default())
        .await
        .context("Network.enable")?;

    let handle = tokio::spawn(async move {
        let mut readiness: HashMap<String, GameReadiness> = HashMap::new();
        loop {
            tokio::select! {
                Some(ev) = on_created.next() => {
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let label = format!("ws {}", crate::privacy::url(&ev.url));
                    let slug = "websocket";
                    let _ = bridges.acquire(key, slug, &label);
                    debug!("ws created: {} (target {target_id} request {})", ev.url, ev.request_id.inner());

                    // If this is the platform's WS (Majsoul), capture
                    // the owning page so autoplay can dispatch input
                    // into it. The handle is bound to the *tab* and lives
                    // until the tab closes (see the poll loop); a new WS on
                    // the same tab just refreshes it. Multi-tab user:
                    // most-recent wins, per the plan.
                    if let Some(ctx) = &autoplay {
                        if !official_majsoul_only && is_autoplay_target_url(&ev.url) {
                            let mut guard = ctx.page.write().await;
                            let prev_target =
                                guard.as_ref().map(|p| p.target_id().inner().clone());
                            let same_tab = prev_target.as_deref() == Some(target_id.as_str());
                            *guard = Some(page.clone());
                            drop(guard);
                            if same_tab {
                                // Majsoul re-opens sockets (Route probes,
                                // lobby reconnects) constantly on the same
                                // tab; refreshing the handle is a no-op and
                                // must not spam warnings.
                                debug!(
                                    "autoplay: page handle refreshed on new WS for target {target_id} ({})",
                                    ev.url
                                );
                            } else {
                                if let Some(prev) = &prev_target {
                                    warn!(
                                        "autoplay: page handle moving from tab {prev} to {target_id}"
                                    );
                                    // The cached canvas rect belonged to the
                                    // old tab; a different tab may have
                                    // different geometry, so drop it and let
                                    // the manager re-query against the new page.
                                    *ctx.canvas_rect.write().await = None;
                                }
                                info!(
                                    "autoplay: page handle bound to target {target_id} via WS {}",
                                    ev.url
                                );
                            }
                        }
                    }
                }
                Some(ev) = on_recv.next() => {
                    let opcode = ev.response.opcode as i64;
                    let payload = match decode_frame_payload(opcode, &ev.response.payload_data) {
                        FrameDecode::Bytes(b) => b,
                        FrameDecode::BadBase64 => {
                            warn!("base64 decode failed for inbound WS frame");
                            continue;
                        }
                        FrameDecode::Skip => continue,
                    };
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let flow_id = format_flow_id(&key);
                    let bridge = bridges.acquire(key, "ws", "ws frame");
                    let result = {
                        let mut b = bridge.lock().expect("bridge mutex poisoned");
                        b.parse(Direction::Down, &payload)
                    };
                    if let Some(message) = readiness.entry(ev.request_id.inner().clone()).or_default().observe(&result) {
                        let _ = notify.send(message);
                    }
                    record_frame(
                        &inspector,
                        FrameDirection::Down,
                        flow_id,
                        opcode,
                        &payload,
                        &ev.response.payload_data,
                        &result,
                    );
                    for e in result.events {
                        let _ = mjai_bus.send(e);
                    }
                }
                Some(ev) = on_sent.next() => {
                    let opcode = ev.response.opcode as i64;
                    let payload = match decode_frame_payload(opcode, &ev.response.payload_data) {
                        FrameDecode::Bytes(b) => b,
                        FrameDecode::BadBase64 => {
                            warn!("base64 decode failed for outbound WS frame");
                            continue;
                        }
                        FrameDecode::Skip => continue,
                    };
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let flow_id = format_flow_id(&key);
                    let bridge = bridges.acquire(key, "ws", "ws frame");
                    let result = {
                        let mut b = bridge.lock().expect("bridge mutex poisoned");
                        b.parse(Direction::Up, &payload)
                    };
                    record_frame(
                        &inspector,
                        FrameDirection::Up,
                        flow_id,
                        opcode,
                        &payload,
                        &ev.response.payload_data,
                        &result,
                    );
                    for e in result.events {
                        let _ = mjai_bus.send(e);
                    }
                }
                Some(ev) = on_closed.next() => {
                    readiness.remove(ev.request_id.inner());
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    debug!("ws closed: target={target_id} request={}", ev.request_id.inner());
                    // Synthetic empty bridge ref so we can call release.
                    // FlowBridges::release reaps the entry when no other
                    // direction's task is holding a clone.
                    let bridge = bridges.acquire(key.clone(), "ws", "ws frame");
                    bridges.release(&key, bridge);

                    // NB: we deliberately do NOT touch the autoplay page
                    // handle here. Majsoul closes short-lived Route-probe /
                    // lobby-reconnect sockets to *.maj-soul.com throughout a
                    // game while the real game-gateway socket stays open;
                    // dropping the handle on those closes silently stopped
                    // autoplay mid-game. The handle is tied to the tab and
                    // cleared by the poll loop when the tab itself closes.
                }
                Some(ev) = on_request.next() => {
                    if is_static_asset(ev.r#type.as_ref()) && !http_cfg.static_assets {
                        continue;
                    }
                    let headers = headers_of(&ev.request.headers);
                    let annotations = annotate::annotate_request(&RequestView::new(
                        &ev.request.method,
                        &ev.request.url,
                        &headers,
                    ));
                    if let Some(a) = annotations.first() {
                        info!(
                            target: "akagi::capture::http",
                            "recognized {} {}", a.kind, a.summary,
                        );
                    }
                    if !http_cfg.record_all && annotations.is_empty() {
                        continue;
                    }
                    inspector.record(InspectorEntry::Http {
                        ts_ms: Local::now().timestamp_millis(),
                        source: CaptureSource::Chromium,
                        exchange: HttpExchange {
                            exchange_id: Some(ev.request_id.inner().clone()),
                            phase: HttpPhase::Request,
                            method: ev.request.method.clone(),
                            url: ev.request.url.clone(),
                            host: host_of(&ev.request.url),
                            version: String::new(),
                            status: None,
                            headers,
                            body: None,
                            annotations,
                        },
                    });
                }
                Some(ev) = on_response.next() => {
                    if !http_cfg.record_all {
                        continue;
                    }
                    if is_static_asset(Some(&ev.r#type)) && !http_cfg.static_assets {
                        continue;
                    }
                    inspector.record(InspectorEntry::Http {
                        ts_ms: Local::now().timestamp_millis(),
                        source: CaptureSource::Chromium,
                        exchange: HttpExchange {
                            exchange_id: Some(ev.request_id.inner().clone()),
                            phase: HttpPhase::Response,
                            method: String::new(),
                            url: ev.response.url.clone(),
                            host: host_of(&ev.response.url),
                            version: String::new(),
                            status: Some(ev.response.status as u16),
                            headers: headers_of(&ev.response.headers),
                            body: Some(HttpBody {
                                text: None,
                                bytes: None,
                                skipped: Some(
                                    "not captured on the chromium backend".to_string(),
                                ),
                            }),
                            annotations: Vec::new(),
                        },
                    });
                }
                else => break,
            }
        }
    });
    Ok(handle)
}

fn is_static_asset(kind: Option<&ResourceType>) -> bool {
    matches!(
        kind,
        Some(
            ResourceType::Image
                | ResourceType::Font
                | ResourceType::Media
                | ResourceType::Stylesheet
        )
    )
}

fn headers_of(headers: &Headers) -> Vec<HttpHeader> {
    let Some(map) = headers.inner().as_object() else {
        return Vec::new();
    };
    let mut out: Vec<HttpHeader> = map
        .iter()
        .map(|(name, value)| HttpHeader {
            name: name.clone(),
            value: match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Host of an absolute URL, or empty when it has none (`data:`, `blob:`).
fn host_of(url: &str) -> String {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_string))
        .unwrap_or_default()
}

/// Build a stable flow id for the inspector. Uses just the request id
/// (truncated, since CDP request ids are opaque hashes ~10 chars) for
/// brevity — the timeline already implicitly groups by flow because
/// frames from one connection arrive interleaved.
fn format_flow_id(key: &FlowKey) -> String {
    let req = &key.request;
    let trim = if req.len() > 10 { &req[..10] } else { req };
    format!("ws:{trim}")
}

/// Record one inspector `WsFrame` entry for a parsed frame. For text
/// frames (`opcode == 1`) `payload_data` is the original UTF-8 string —
/// we use it verbatim so the JSONL stays human-readable. For binary
/// frames (`opcode == 2`) we re-emit the original base64 (`payload_data`)
/// rather than re-encoding `payload`, which is identical content but
/// avoids a copy.
fn record_frame(
    inspector: &InspectorWriter,
    direction: FrameDirection,
    flow_id: String,
    opcode: i64,
    payload: &[u8],
    payload_data: &str,
    result: &crate::bridge::ParseResult,
) {
    let raw = if opcode == 1 {
        FrameRaw::Text(payload_data.to_string())
    } else {
        FrameRaw::Binary(payload_data.to_string())
    };
    inspector.record(InspectorEntry::WsFrame {
        ts_ms: Local::now().timestamp_millis(),
        direction,
        flow_id,
        size: payload.len(),
        raw,
        parsed: result.parsed.clone(),
        emitted: result.events.len(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_payload_ok() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"hello");
        assert_eq!(decode_payload(&b64), Some(b"hello".to_vec()));
    }

    #[test]
    fn decode_payload_bad() {
        assert_eq!(decode_payload("not-base64-!@#"), None);
    }

    #[test]
    fn diff_adds_and_removes() {
        let prev: HashSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let current: HashSet<String> = ["b", "c", "d"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&prev, &current);
        let mut adds = adds;
        let mut removes = removes;
        adds.sort();
        removes.sort();
        assert_eq!(adds, vec!["d"]);
        assert_eq!(removes, vec!["a"]);
    }

    #[test]
    fn diff_empty_when_unchanged() {
        let s: HashSet<String> = ["x", "y"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&s, &s);
        assert!(adds.is_empty());
        assert!(removes.is_empty());
    }

    #[test]
    fn diff_initial_subscribe() {
        let prev: HashSet<String> = HashSet::new();
        let current: HashSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&prev, &current);
        let mut adds = adds;
        adds.sort();
        assert_eq!(adds, vec!["a", "b"]);
        assert!(removes.is_empty());
    }

    /// Regression: a Majsoul Route-probe / lobby-reconnect socket closing
    /// must NOT clear the autoplay page handle — that was making autoplay
    /// silently stop mid-game. The handle is tied to the browser tab, so
    /// only the owning tab's removal from the page snapshot clears it.
    #[test]
    fn page_handle_cleared_only_when_owning_tab_closes() {
        let owner = Some("TAB_A");
        // A *different* tab closing (or a WS closing, which never reaches
        // this predicate at all) leaves our handle intact.
        assert!(!page_handle_cleared_by_removal(owner, &["TAB_B".into()]));
        // Nothing reaped this tick — keep the handle.
        assert!(!page_handle_cleared_by_removal(owner, &[]));
        // The owning tab itself disappearing is the only trigger.
        assert!(page_handle_cleared_by_removal(
            owner,
            &["TAB_B".into(), "TAB_A".into()]
        ));
        // No handle bound → nothing to clear regardless of what closed.
        assert!(!page_handle_cleared_by_removal(None, &["TAB_A".into()]));
    }

    #[test]
    fn text_frame_passes_through_as_utf8_bytes() {
        let payload = r#"{"tag":"INIT","seed":"1,0,0,2,5,134"}"#;
        assert_eq!(
            decode_frame_payload(1, payload),
            FrameDecode::Bytes(payload.as_bytes().to_vec())
        );
    }

    #[test]
    fn text_heartbeat_passes_through() {
        assert_eq!(
            decode_frame_payload(1, "<Z/>"),
            FrameDecode::Bytes(b"<Z/>".to_vec())
        );
    }

    #[test]
    fn binary_frame_base64_decodes() {
        let raw = b"\x00\x01\x02hello";
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        assert_eq!(
            decode_frame_payload(2, &b64),
            FrameDecode::Bytes(raw.to_vec())
        );
    }

    #[test]
    fn binary_frame_bad_base64_signals_decode_error() {
        // Distinguished from `Skip` so the inline branch can WARN —
        // legit malformed CDP from Chrome shouldn't be confused with
        // an intentionally-ignored control frame.
        assert_eq!(
            decode_frame_payload(2, "not base64!@#"),
            FrameDecode::BadBase64
        );
    }

    #[test]
    fn control_and_continuation_frames_are_skipped() {
        for opcode in [0i64, 8, 9, 10] {
            assert_eq!(
                decode_frame_payload(opcode, "irrelevant"),
                FrameDecode::Skip,
                "opcode {opcode} should skip"
            );
        }
    }
}

#[cfg(test)]
mod attached_browser_tests {
    use super::*;
    #[test]
    fn midgame_warning_is_once_and_recovery_requires_start_game_and_round() {
        use crate::bridge::ParseResult;
        use crate::schema::ParsedFrame;
        let mut state = GameReadiness::default();
        let mut result = ParseResult {
            events: vec![],
            parsed: Some(ParsedFrame {
                method: ".lq.ActionPrototype".into(),
                args: serde_json::json!({}),
            }),
        };
        assert!(state.observe(&result).unwrap().sticky);
        assert!(state.observe(&result).is_none());
        result.events = vec![serde_json::from_value(
            serde_json::json!({"type":"start_game","names":["","","",""],"id":0}),
        )
        .unwrap()];
        assert!(state.observe(&result).is_none());
        result.events = vec![serde_json::from_value(serde_json::json!({"type":"start_kyoku","bakaze":"E","dora_marker":"1m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[[],[],[],[]]})).unwrap()];
        assert_eq!(
            state.observe(&result).unwrap().title,
            "Majsoul game state ready"
        );
        assert!(state.observe(&result).is_none());
    }
    #[test]
    fn attached_browser_scope_excludes_other_tabs() {
        assert!(official_majsoul_page("https://game.maj-soul.com/1/"));
        for url in [
            "http://game.maj-soul.com/1/",
            "https://game.maj-soul.com.evil.test/1/",
            "https://mail.example.test/",
            "https://game.maj-soul.com:8443/1/",
            "https://game.maj-soul.com/other/",
        ] {
            assert!(!official_majsoul_page(url));
        }
    }
    #[tokio::test]
    async fn dropping_capture_aborts_child_tasks() {
        let task = tokio::spawn(std::future::pending::<()>());
        let handle = task.abort_handle();
        drop(AbortTask(task));
        tokio::task::yield_now().await;
        assert!(handle.is_finished());
    }
}
