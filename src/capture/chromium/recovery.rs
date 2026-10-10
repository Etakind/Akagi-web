//! User-triggered recovery: exactly one game-socket reconnect, then at most
//! one page reload. Normal client authentication owns all protocol requests.
use crate::{
    autoplay::AutoplayContext,
    capture::recovery::{RecoveryMethod, RecoveryPhase},
    event_bus::{GameUpdate, MjaiBus},
};
use anyhow::{bail, Result};
use chromiumoxide::{
    cdp::js_protocol::runtime::{
        CallFunctionOnParams, EvaluateParams, QueryObjectsParams, ReleaseObjectGroupParams,
    },
    Page,
};
use std::{sync::Arc, time::Duration};

const GROUP: &str = "akagi-recovery-probe";
const CLEANUP: &str = "(() => { const r=window.__akagiRecoveryV1; if(r){for(const [s,l] of r.sockets)s.removeEventListener('message',l);delete window.__akagiRecoveryV1;} })()";
const CLOSE: &str = include_str!("recovery_close.js");

pub async fn prepare_probe(page: &Page) -> Result<()> {
    let eval = page
        .execute(
            EvaluateParams::builder()
                .expression("WebSocket.prototype")
                .object_group(GROUP)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await?;
    let prototype = eval
        .result
        .result
        .object_id
        .ok_or_else(|| anyhow::anyhow!("No websocket prototype"))?;
    let objects = page
        .execute(
            QueryObjectsParams::builder()
                .prototype_object_id(prototype)
                .object_group(GROUP)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await?;
    let id = objects
        .result
        .objects
        .object_id
        .ok_or_else(|| anyhow::anyhow!("No websocket objects"))?;
    let result = page
        .execute(
            CallFunctionOnParams::builder()
                .object_id(id)
                .function_declaration(include_str!("recovery_probe.js"))
                .return_by_value(true)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await;
    let _ = page.execute(ReleaseObjectGroupParams::new(GROUP)).await;
    result?;
    Ok(())
}

pub async fn cleanup_probe(page: &Page) {
    let _ = tokio::time::timeout(Duration::from_secs(2), page.evaluate(CLEANUP)).await;
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        page.execute(ReleaseObjectGroupParams::new(GROUP)),
    )
    .await;
}

async fn wait_ready(ctx: &AutoplayContext, page: &Page, limit: Duration) -> bool {
    let mut rx = ctx.recovery.changes.subscribe();
    tokio::time::timeout(limit, async {
        loop {
            // Runtime evaluation can temporarily fail while a reload
            // replaces the execution context. Binding/navigation lifecycle
            // still cancels a changed page; validate the URL again at Ready.
            if !bound_page(ctx, page).await {
                return false;
            }
            let status = rx.borrow_and_update().clone();
            match status.phase {
                RecoveryPhase::Ready => return same_page(ctx, page).await,
                RecoveryPhase::Error | RecoveryPhase::Inactive => return false,
                _ => {}
            }
            if rx.changed().await.is_err() {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false)
}
async fn bound_page(ctx: &AutoplayContext, page: &Page) -> bool {
    ctx.page
        .read()
        .await
        .as_ref()
        .is_some_and(|p| p.session_id() == page.session_id())
}
async fn same_page(ctx: &AutoplayContext, page: &Page) -> bool {
    bound_page(ctx, page).await && ctx.page_allowed(page).await
}

pub async fn recover_game(ctx: Arc<AutoplayContext>, bus: MjaiBus) -> Result<()> {
    let _attempt = ctx
        .recovery
        .attempt
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Recovery already running"))?;
    if !ctx.recovery.snapshot().can_recover {
        bail!("Recovery is only available when game state is missing");
    }
    let page = ctx
        .page
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Keep exactly one official game page open"))?;
    if !same_page(&ctx, &page).await {
        bail!("Official game page unavailable");
    }
    let lease = ctx.recovery.lease();
    run_attempt(&BrowserRecovery {
        ctx: ctx.clone(),
        page,
        bus,
        lease,
    })
    .await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReconnectOutcome {
    Closed,
    Unidentified,
    Ambiguous,
}

#[async_trait::async_trait]
trait RecoveryDriver: Sync {
    fn begin(&self, method: RecoveryMethod);
    fn fail(&self, reason: &str);
    async fn reconnect(&self) -> ReconnectOutcome;
    async fn reload(&self) -> bool;
    async fn ready(&self, limit: Duration) -> bool;
    async fn current(&self) -> bool;
}

async fn run_attempt(driver: &impl RecoveryDriver) -> Result<()> {
    driver.begin(RecoveryMethod::Reconnect);
    match driver.reconnect().await {
        ReconnectOutcome::Ambiguous => {
            driver.fail("ambiguous_game_connections");
            bail!("Multiple game connections: keep exactly one game open");
        }
        ReconnectOutcome::Closed if driver.ready(Duration::from_secs(15)).await => return Ok(()),
        _ => {}
    }
    if !driver.current().await {
        bail!("Recovery cancelled: game page changed");
    }
    driver.begin(RecoveryMethod::Reload);
    if driver.reload().await && driver.ready(Duration::from_secs(60)).await {
        return Ok(());
    }
    if !driver.current().await {
        bail!("Recovery cancelled: game page changed");
    }
    driver.fail("reconnect_and_reload_failed");
    bail!("Could not recover this game after reconnect and one refresh. Automatic actions remain suspended.")
}

struct BrowserRecovery {
    ctx: Arc<AutoplayContext>,
    page: Page,
    bus: MjaiBus,
    lease: u64,
}
#[async_trait::async_trait]
impl RecoveryDriver for BrowserRecovery {
    fn begin(&self, method: RecoveryMethod) {
        self.ctx.invalidate_actions();
        self.ctx
            .recovery
            .invalidate(RecoveryPhase::Recovering, None);
        self.ctx.recovery.method(method);
        let _ = self.bus.send_update(GameUpdate::Invalidated);
    }
    fn fail(&self, reason: &str) {
        self.ctx
            .recovery
            .invalidate(RecoveryPhase::Error, Some(reason));
        self.ctx.invalidate_actions();
    }
    async fn current(&self) -> bool {
        self.ctx.recovery.lease() == self.lease && same_page(&self.ctx, &self.page).await
    }
    async fn reconnect(&self) -> ReconnectOutcome {
        *self.ctx.canvas_rect.write().await = None;
        let probe = tokio::time::timeout(Duration::from_secs(3), prepare_probe(&self.page)).await;
        if !matches!(probe, Ok(Ok(()))) || !self.current().await {
            return ReconnectOutcome::Unidentified;
        }
        let result = tokio::time::timeout(Duration::from_secs(2), self.page.evaluate(CLOSE)).await;
        match result
            .ok()
            .and_then(|v| v.ok())
            .and_then(|v| v.value().cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .as_deref()
        {
            Some("closed") => ReconnectOutcome::Closed,
            Some("ambiguous") => ReconnectOutcome::Ambiguous,
            _ => ReconnectOutcome::Unidentified,
        }
    }
    async fn reload(&self) -> bool {
        if !self.current().await {
            return false;
        }
        self.ctx.recovery.expect_reload_navigation();
        matches!(
            tokio::time::timeout(Duration::from_secs(3), self.page.reload()).await,
            Ok(Ok(_))
        )
    }
    async fn ready(&self, limit: Duration) -> bool {
        self.ctx.recovery.lease() == self.lease
            && wait_ready(&self.ctx, &self.page, limit).await
            && self.current().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Mock {
        outcome: ReconnectOutcome,
        reconnect_ready: bool,
        reload_ready: bool,
        current: bool,
        calls: Mutex<Vec<String>>,
    }
    impl Mock {
        fn new(
            outcome: ReconnectOutcome,
            reconnect_ready: bool,
            reload_ready: bool,
            current: bool,
        ) -> Self {
            Self {
                outcome,
                reconnect_ready,
                reload_ready,
                current,
                calls: Mutex::new(Vec::new()),
            }
        }
        fn record(&self, s: &str) {
            self.calls.lock().unwrap().push(s.to_owned());
        }
    }
    #[async_trait::async_trait]
    impl RecoveryDriver for Mock {
        fn begin(&self, method: RecoveryMethod) {
            self.record(match method {
                RecoveryMethod::Reconnect => "begin_reconnect",
                RecoveryMethod::Reload => "begin_reload",
            });
        }
        fn fail(&self, reason: &str) {
            self.record(reason);
        }
        async fn reconnect(&self) -> ReconnectOutcome {
            self.record("close_once");
            self.outcome
        }
        async fn reload(&self) -> bool {
            self.record("reload_once");
            true
        }
        async fn ready(&self, limit: Duration) -> bool {
            self.record(&format!("wait_{}", limit.as_secs()));
            if limit == Duration::from_secs(15) {
                self.reconnect_ready
            } else {
                self.reload_ready
            }
        }
        async fn current(&self) -> bool {
            self.current
        }
    }
    #[tokio::test]
    async fn reconnect_success_never_refreshes() {
        let mock = Mock::new(ReconnectOutcome::Closed, true, false, true);
        run_attempt(&mock).await.unwrap();
        assert_eq!(
            *mock.calls.lock().unwrap(),
            ["begin_reconnect", "close_once", "wait_15"]
        );
    }
    #[tokio::test]
    async fn fallback_refreshes_once_and_waits_sixty_seconds() {
        let mock = Mock::new(ReconnectOutcome::Closed, false, true, true);
        run_attempt(&mock).await.unwrap();
        assert_eq!(
            *mock.calls.lock().unwrap(),
            [
                "begin_reconnect",
                "close_once",
                "wait_15",
                "begin_reload",
                "reload_once",
                "wait_60"
            ]
        );
    }
    #[tokio::test]
    async fn failed_fallback_has_no_second_refresh() {
        let mock = Mock::new(ReconnectOutcome::Unidentified, false, false, true);
        assert!(run_attempt(&mock).await.is_err());
        assert_eq!(
            *mock.calls.lock().unwrap(),
            [
                "begin_reconnect",
                "close_once",
                "begin_reload",
                "reload_once",
                "wait_60",
                "reconnect_and_reload_failed"
            ]
        );
    }
    #[tokio::test]
    async fn ambiguous_or_cancelled_attempt_never_refreshes() {
        let ambiguous = Mock::new(ReconnectOutcome::Ambiguous, false, false, true);
        assert!(run_attempt(&ambiguous).await.is_err());
        assert_eq!(
            *ambiguous.calls.lock().unwrap(),
            [
                "begin_reconnect",
                "close_once",
                "ambiguous_game_connections"
            ]
        );
        let cancelled = Mock::new(ReconnectOutcome::Closed, false, false, false);
        assert!(run_attempt(&cancelled).await.is_err());
        assert_eq!(
            *cancelled.calls.lock().unwrap(),
            ["begin_reconnect", "close_once", "wait_15"]
        );
    }
}
