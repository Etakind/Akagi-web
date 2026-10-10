//! 浏览器采集：通过 CDP 接收官方雀魂页面的网络事件，交给牌局解析层。

pub mod chromium;
pub mod flow;
pub mod recovery;

use crate::autoplay::AutoplayContext;
use crate::config::Platform;
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::logger::Session;
use anyhow::Result;
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Notify;

/// Discriminant exposed in [`CaptureStatus`] / IPC payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Chromium,
}

impl CaptureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureKind::Chromium => "chromium",
        }
    }
}

/// Static per-backend descriptor. The supervisor surfaces this in
/// status messages so the UI can show "Running (Chromium @ /usr/bin/google-chrome)"
/// or similar.
#[derive(Debug, Clone)]
pub struct CaptureDescriptor {
    pub kind: CaptureKind,
    pub label: String,
}

/// Long-lived shutdown signal. The supervisor holds the `Notify` and
/// fires it when the user (or a config swap) wants the backend gone.
/// Backends should treat the future returned by [`ShutdownToken::wait`]
/// as a cooperative cancellation point.
#[derive(Clone)]
pub struct ShutdownToken(Arc<Notify>);

impl ShutdownToken {
    pub fn new() -> (Self, Arc<Notify>) {
        let n = Arc::new(Notify::new());
        (Self(n.clone()), n)
    }

    /// Resolves once shutdown has been requested.
    pub fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.0.notified().await;
        })
    }
}

/// Everything a backend needs to push parsed mjai events into the rest of
/// the app. Cheap to clone (all `Arc`/`Sender`/`Notify`).
#[derive(Clone)]
pub struct CaptureCtx {
    pub session: Arc<Session>,
    pub platform: Platform,
    pub mjai_bus: MjaiBus,
    pub notify_bus: NotifyBus,
    pub autoplay: Option<Arc<AutoplayContext>>,
    /// What to record of the HTTP traffic this backend intercepts. Both
    /// backends honour it; `static_assets` is chromium-only.
    pub http: crate::config::HttpCaptureConfig,
}

/// A capture transport. Implementors run a long-lived I/O loop until
/// `shutdown` resolves or a fatal error occurs, pushing frames into the
/// bridge layer along the way.
// async_trait 0.1.89 adds #[must_use] to an already must-use boxed Future.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait CaptureBackend: Send {
    /// Run the backend. Must respect `shutdown` for graceful exit.
    /// Returns `Ok(())` on clean exit, `Err` otherwise.
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()>;

    /// Static description for status reporting. Called once at spawn time
    /// (and again after restart) — must not block.
    fn descriptor(&self) -> CaptureDescriptor;
}
