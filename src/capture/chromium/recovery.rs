//! User-triggered recovery: at most one reload of the uniquely bound page.
//! Normal client authentication owns all protocol requests; no socket probing.
use crate::{
    autoplay::AutoplayContext,
    capture::recovery::{RecoveryMethod, RecoveryPhase},
    event_bus::{GameUpdate, MjaiBus},
};
use anyhow::{bail, Result};
use chromiumoxide::Page;
use std::{sync::Arc, time::Duration};

async fn wait_ready(ctx: &AutoplayContext, page: &Page, lease: u64, limit: Duration) -> bool {
    let mut rx = ctx.recovery.changes.subscribe();
    tokio::time::timeout(limit, async {
        loop {
            // Runtime evaluation can temporarily fail while a reload
            // replaces the execution context. Binding/navigation lifecycle
            // still cancels a changed page; validate the URL again at Ready.
            if !bound_page(ctx, page).await || ctx.recovery.lease() != lease {
                return false;
            }
            let status = rx.borrow_and_update().clone();
            match status.phase {
                RecoveryPhase::Ready => {
                    return same_page(ctx, page).await && ctx.recovery.lease() == lease;
                }
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
    let lease = ctx.recovery.lease();
    let page = ctx
        .page
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Keep exactly one official game page open"))?;
    if !same_page(&ctx, &page).await || ctx.recovery.lease() != lease {
        bail!("Official game page unavailable");
    }
    // Drop the driver (and its navigation allowance) before releasing the
    // attempt lock, including when this future is cancelled.
    let driver = BrowserRecovery {
        ctx: ctx.clone(),
        page,
        bus,
        lease,
    };
    run_attempt(&driver).await
}

// async-trait adds #[must_use] to methods returning already must-use futures.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
trait RecoveryDriver: Sync {
    fn begin(&self, method: RecoveryMethod) -> bool;
    fn fail(&self, reason: &str);
    async fn reload(&self) -> bool;
    async fn ready(&self, limit: Duration) -> bool;
    async fn current(&self) -> bool;
}

async fn run_attempt(driver: &impl RecoveryDriver) -> Result<()> {
    if !driver.current().await || !driver.begin(RecoveryMethod::Reload) {
        bail!("Recovery cancelled: game page changed");
    }
    if !driver.reload().await {
        if !driver.current().await {
            bail!("Recovery cancelled: game page changed");
        }
        driver.fail("reload_failed");
        bail!("Could not refresh this game. Automatic actions remain suspended.");
    }
    if driver.ready(Duration::from_secs(60)).await {
        return Ok(());
    }
    if !driver.current().await {
        bail!("Recovery cancelled: game page changed");
    }
    driver.fail("reload_restore_timeout");
    bail!("Could not restore this game after one refresh. Automatic actions remain suspended.")
}

struct BrowserRecovery {
    ctx: Arc<AutoplayContext>,
    page: Page,
    bus: MjaiBus,
    lease: u64,
}
impl Drop for BrowserRecovery {
    fn drop(&mut self) {
        // Includes cancellation of the recovery future. A stale attempt must
        // not clear a navigation allowance belonging to a newer capture.
        self.ctx.recovery.clear_reload_navigation(self.lease);
    }
}
#[async_trait::async_trait]
impl RecoveryDriver for BrowserRecovery {
    fn begin(&self, _method: RecoveryMethod) -> bool {
        self.ctx.recovery.begin_reload(self.lease, || {
            self.ctx.invalidate_actions();
            let _ = self.bus.send_update(GameUpdate::Invalidated);
        })
    }
    fn fail(&self, reason: &str) {
        self.ctx.recovery.fail_reload(self.lease, reason, || {
            self.ctx.invalidate_actions();
        });
    }
    async fn current(&self) -> bool {
        self.ctx.recovery.lease() == self.lease
            && same_page(&self.ctx, &self.page).await
            && self.ctx.recovery.lease() == self.lease
    }
    async fn reload(&self) -> bool {
        if !self.current().await {
            return false;
        }
        // Match the binding loop's page -> canvas lock order. Holding the
        // page read lock prevents rebinding while its canvas is cleared.
        let bound = self.ctx.page.read().await;
        if !bound
            .as_ref()
            .is_some_and(|page| page.session_id() == self.page.session_id())
        {
            return false;
        }
        let mut canvas = self.ctx.canvas_rect.write().await;
        if !self.ctx.recovery.expect_reload_navigation(self.lease) {
            return false;
        }
        *canvas = None;
        drop(canvas);
        drop(bound);
        matches!(
            tokio::time::timeout(Duration::from_secs(3), self.page.reload()).await,
            Ok(Ok(_))
        )
    }
    async fn ready(&self, limit: Duration) -> bool {
        self.ctx.recovery.lease() == self.lease
            && wait_ready(&self.ctx, &self.page, self.lease, limit).await
            && self.current().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct Mock {
        begin_ok: bool,
        reload_ok: bool,
        reload_ready: bool,
        current_results: Mutex<VecDeque<bool>>,
        calls: Mutex<Vec<String>>,
    }
    impl Mock {
        fn new(
            current_results: impl IntoIterator<Item = bool>,
            begin_ok: bool,
            reload_ok: bool,
            reload_ready: bool,
        ) -> Self {
            Self {
                begin_ok,
                reload_ok,
                reload_ready,
                current_results: Mutex::new(current_results.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn record(&self, s: &str) {
            self.calls.lock().unwrap().push(s.to_owned());
        }
    }
    #[async_trait::async_trait]
    impl RecoveryDriver for Mock {
        fn begin(&self, method: RecoveryMethod) -> bool {
            assert_eq!(method, RecoveryMethod::Reload);
            self.record("begin_reload");
            self.begin_ok
        }
        fn fail(&self, reason: &str) {
            self.record(reason);
        }
        async fn reload(&self) -> bool {
            self.record("reload_once");
            self.reload_ok
        }
        async fn ready(&self, limit: Duration) -> bool {
            self.record(&format!("wait_{}", limit.as_secs()));
            assert_eq!(limit, Duration::from_secs(60));
            self.reload_ready
        }
        async fn current(&self) -> bool {
            self.record("current");
            self.current_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(false)
        }
    }
    #[tokio::test]
    async fn reload_success_waits_sixty_seconds_without_second_refresh() {
        let mock = Mock::new([true], true, true, true);
        run_attempt(&mock).await.unwrap();
        assert_eq!(
            *mock.calls.lock().unwrap(),
            ["current", "begin_reload", "reload_once", "wait_60"]
        );
    }
    #[tokio::test]
    async fn reload_failure_reports_reload_failed_without_second_refresh() {
        let mock = Mock::new([true, true], true, false, false);
        let error = run_attempt(&mock).await.unwrap_err();
        assert!(error.to_string().contains("Could not refresh"));
        assert_eq!(
            *mock.calls.lock().unwrap(),
            [
                "current",
                "begin_reload",
                "reload_once",
                "current",
                "reload_failed"
            ]
        );
    }
    #[tokio::test]
    async fn reload_restore_timeout_reports_timeout_without_second_refresh() {
        let mock = Mock::new([true, true], true, true, false);
        let error = run_attempt(&mock).await.unwrap_err();
        assert!(error.to_string().contains("Could not restore"));
        assert_eq!(
            *mock.calls.lock().unwrap(),
            [
                "current",
                "begin_reload",
                "reload_once",
                "wait_60",
                "current",
                "reload_restore_timeout"
            ]
        );
    }
    #[tokio::test]
    async fn initial_page_change_or_duplicate_attempt_never_refreshes() {
        let changed = Mock::new([false], true, true, true);
        assert!(run_attempt(&changed).await.is_err());
        assert_eq!(*changed.calls.lock().unwrap(), ["current"]);
        let duplicate = Mock::new([true], false, true, true);
        assert!(run_attempt(&duplicate).await.is_err());
        assert_eq!(
            *duplicate.calls.lock().unwrap(),
            ["current", "begin_reload"]
        );
    }
    #[tokio::test]
    async fn page_change_after_reload_cancels_without_failure_transition() {
        let mock = Mock::new([true, false], true, false, false);
        let error = run_attempt(&mock).await.unwrap_err();
        assert!(error.to_string().contains("page changed"));
        assert_eq!(
            *mock.calls.lock().unwrap(),
            ["current", "begin_reload", "reload_once", "current"]
        );
    }
}
