//! Recovery lifecycle shared by capture and the three state consumers.
//! A replay is a transaction: no input is allowed until every consumer has
//! rebuilt its view. Tokens from an older capture generation cannot commit.
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc, Mutex,
};
use tokio::sync::watch;

pub const TRACKER: u8 = 1;
pub const BOT: u8 = 2;
pub const AUTOPLAY: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Inactive,
    Missing,
    Recovering,
    WaitingRound,
    Ready,
    Error,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMethod {
    Reconnect,
    Reload,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStatus {
    pub phase: RecoveryPhase,
    pub method: Option<RecoveryMethod>,
    pub reason: Option<String>,
    pub epoch: u64,
    pub can_recover: bool,
    pub event_count: usize,
    pub first_step: Option<u64>,
    pub last_step: Option<u64>,
}

pub struct RecoveryState {
    pub changes: watch::Sender<RecoveryStatus>,
    epoch: AtomicU64,
    consumers: AtomicU8,
    pub attempt: tokio::sync::Mutex<()>,
    owner: Mutex<Option<u64>>,
    window: Mutex<Option<crate::autoplay::status::Window>>,
    window_revision: AtomicU64,
    transition: Mutex<()>,
    lease: AtomicU64,
    page_available: AtomicBool,
    reload_navigation: Mutex<Option<std::time::Instant>>,
}
impl Default for RecoveryState {
    fn default() -> Self {
        let (changes, _) = watch::channel(RecoveryStatus {
            phase: RecoveryPhase::Inactive,
            method: None,
            reason: None,
            epoch: 0,
            can_recover: false,
            event_count: 0,
            first_step: None,
            last_step: None,
        });
        Self {
            changes,
            epoch: AtomicU64::new(0),
            consumers: AtomicU8::new(TRACKER),
            attempt: tokio::sync::Mutex::new(()),
            owner: Mutex::new(None),
            window: Mutex::new(None),
            window_revision: AtomicU64::new(0),
            transition: Mutex::new(()),
            lease: AtomicU64::new(0),
            page_available: AtomicBool::new(false),
            reload_navigation: Mutex::new(None),
        }
    }
}
impl RecoveryState {
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    pub fn snapshot(&self) -> RecoveryStatus {
        self.changes.borrow().clone()
    }
    pub fn register(&self, consumer: u8) {
        self.consumers.fetch_or(consumer, Ordering::SeqCst);
    }
    pub fn page_available(&self, available: bool) {
        self.page_available.store(available, Ordering::SeqCst);
        self.changes.send_if_modified(|s| {
            let can_recover = available
                && matches!(
                    s.phase,
                    RecoveryPhase::Inactive | RecoveryPhase::Missing | RecoveryPhase::Error
                );
            if s.can_recover == can_recover {
                return false;
            }
            s.can_recover = can_recover;
            true
        });
    }

    pub fn batch(&self, event_count: usize, first_step: Option<u64>, last_step: Option<u64>) {
        self.changes.send_modify(|s| {
            s.event_count = event_count;
            s.first_step = first_step;
            s.last_step = last_step;
        });
    }
    pub fn allows_input(&self) -> bool {
        matches!(
            self.snapshot().phase,
            RecoveryPhase::Inactive | RecoveryPhase::Ready
        )
    }
    pub fn owns(&self, flow: u64) -> bool {
        *self.owner.lock().unwrap() == Some(flow)
    }
    pub fn bind(&self, flow: u64) {
        *self.owner.lock().unwrap() = Some(flow);
    }
    pub fn window(&self, window: Option<crate::autoplay::status::Window>) {
        let mut current = self.window.lock().unwrap();
        *current = window;
        // Even None -> None invalidates a restored advisory after a manual
        // input or a new live event. It never grants an executable window.
        self.window_revision.fetch_add(1, Ordering::SeqCst);
    }
    pub fn window_revision(&self) -> u64 {
        self.window_revision.load(Ordering::SeqCst)
    }
    pub fn window_valid(&self, window: Option<crate::autoplay::status::Window>) -> bool {
        window.is_some_and(|window| window.unexpired()) && *self.window.lock().unwrap() == window
    }
    pub fn set(&self, phase: RecoveryPhase, reason: Option<&str>) {
        let _transition = self.transition.lock().unwrap();
        self.set_locked(phase, reason);
    }
    fn set_locked(&self, phase: RecoveryPhase, reason: Option<&str>) {
        self.changes.send_modify(|s| {
            s.phase = phase;
            s.reason = reason.map(str::to_owned);
            s.epoch = self.epoch();
            s.can_recover = self.page_available.load(Ordering::SeqCst)
                && matches!(
                    phase,
                    RecoveryPhase::Inactive | RecoveryPhase::Missing | RecoveryPhase::Error
                );
        });
    }
    pub fn invalidate(&self, phase: RecoveryPhase, reason: Option<&str>) -> u64 {
        let _transition = self.transition.lock().unwrap();
        self.invalidate_locked(phase, reason)
    }
    fn invalidate_locked(&self, phase: RecoveryPhase, reason: Option<&str>) -> u64 {
        self.window(None);
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.set_locked(phase, reason);
        epoch
    }
    pub fn reset(&self) {
        let _transition = self.transition.lock().unwrap();
        *self.reload_navigation.lock().unwrap() = None;
        self.page_available.store(false, Ordering::SeqCst);
        self.lease.fetch_add(1, Ordering::SeqCst);
        *self.owner.lock().unwrap() = None;
        self.invalidate_locked(RecoveryPhase::Inactive, None);
        self.changes.send_modify(|s| s.method = None);
        self.batch(0, None, None);
    }
    /// Serialize the lease check and action invalidation with capture reset.
    /// The callback must not call back into recovery transitions.
    pub fn begin_reload(&self, lease: u64, invalidate_actions: impl FnOnce()) -> bool {
        let _transition = self.transition.lock().unwrap();
        if self.lease() != lease || !self.snapshot().can_recover {
            return false;
        }
        invalidate_actions();
        self.invalidate_locked(RecoveryPhase::Recovering, None);
        self.changes
            .send_modify(|s| s.method = Some(RecoveryMethod::Reload));
        true
    }
    /// A failed old attempt cannot invalidate the newly bound capture.
    /// The callback must not call back into recovery transitions.
    pub fn fail_reload(&self, lease: u64, reason: &str, invalidate_actions: impl FnOnce()) -> bool {
        let _transition = self.transition.lock().unwrap();
        if self.lease() != lease {
            return false;
        }
        *self.reload_navigation.lock().unwrap() = None;
        invalidate_actions();
        self.invalidate_locked(RecoveryPhase::Error, Some(reason));
        true
    }
    pub fn expect_reload_navigation(&self, lease: u64) -> bool {
        let _transition = self.transition.lock().unwrap();
        if self.lease() != lease {
            return false;
        }
        *self.reload_navigation.lock().unwrap() =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
        true
    }
    pub fn clear_reload_navigation(&self, lease: u64) {
        let _transition = self.transition.lock().unwrap();
        if self.lease() == lease {
            *self.reload_navigation.lock().unwrap() = None;
        }
    }
    /// The one reload initiated by recovery may keep the capture lease;
    /// every other main-frame navigation cancels the attempt.
    pub fn page_navigated(&self) -> bool {
        let expected = self.reload_navigation.lock().unwrap().take();
        if expected.is_some_and(|deadline| std::time::Instant::now() <= deadline) {
            return false;
        }
        self.reset();
        true
    }
    pub fn lease(&self) -> u64 {
        self.lease.load(Ordering::SeqCst)
    }
    pub fn method(&self, method: RecoveryMethod) {
        self.changes.send_modify(|s| s.method = Some(method));
    }
    pub fn token(self: &Arc<Self>) -> Arc<RestoreToken> {
        self.set(RecoveryPhase::Recovering, None);
        self.stamp()
    }
    pub fn stamp(self: &Arc<Self>) -> Arc<RestoreToken> {
        Arc::new(RestoreToken {
            state: self.clone(),
            epoch: self.epoch(),
            expected: self.consumers.load(Ordering::SeqCst),
            acknowledged: AtomicU8::new(0),
        })
    }
}
pub struct RestoreToken {
    pub state: Arc<RecoveryState>,
    pub epoch: u64,
    expected: u8,
    acknowledged: AtomicU8,
}
impl std::fmt::Debug for RestoreToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreToken")
            .field("epoch", &self.epoch)
            .finish()
    }
}
impl RestoreToken {
    pub fn current(&self) -> bool {
        self.state.epoch() == self.epoch
    }
    pub fn ack(&self, consumer: u8) {
        let acknowledged = self.acknowledged.fetch_or(consumer, Ordering::SeqCst) | consumer;
        let _transition = self.state.transition.lock().unwrap();
        if self.current() && acknowledged & self.expected == self.expected {
            self.state.set_locked(RecoveryPhase::Ready, None);
        }
    }
    pub fn fail(&self, reason: &str) {
        let _transition = self.state.transition.lock().unwrap();
        if self.current() {
            self.state.epoch.fetch_add(1, Ordering::SeqCst);
            self.state.window(None);
            self.state.set_locked(RecoveryPhase::Error, Some(reason));
        }
    }
}
impl PartialEq for RestoreToken {
    fn eq(&self, other: &Self) -> bool {
        self.epoch == other.epoch && Arc::ptr_eq(&self.state, &other.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_expected_reload_preserves_the_lease() {
        let state = RecoveryState::default();
        let lease = state.lease();
        assert!(state.expect_reload_navigation(lease));
        assert!(!state.page_navigated());
        assert_eq!(state.lease(), lease);
        assert!(state.page_navigated());
        assert_ne!(state.lease(), lease);
    }
    #[test]
    fn reload_begin_is_single_use_and_stale_failure_cannot_cross_a_reset() {
        let state = RecoveryState::default();
        state.page_available(true);
        let lease = state.lease();
        let mut invalidations = 0;
        assert!(state.begin_reload(lease, || invalidations += 1));
        assert_eq!(invalidations, 1);
        assert_eq!(state.snapshot().phase, RecoveryPhase::Recovering);
        assert_eq!(state.snapshot().method, Some(RecoveryMethod::Reload));
        assert!(!state.begin_reload(lease, || invalidations += 1));
        assert_eq!(
            invalidations, 1,
            "duplicate recovery must not invalidate again"
        );

        state.reset();
        assert!(!state.fail_reload(lease, "stale_reload", || invalidations += 1));
        assert_eq!(
            invalidations, 1,
            "stale failure must not touch the new lease"
        );
        assert_eq!(state.snapshot().phase, RecoveryPhase::Inactive);
    }
    #[test]
    fn reload_failure_cleans_its_navigation_permit_and_reports_reason() {
        let state = RecoveryState::default();
        state.page_available(true);
        let lease = state.lease();
        assert!(state.begin_reload(lease, || {}));
        assert!(state.expect_reload_navigation(lease));
        let mut invalidations = 0;
        assert!(state.fail_reload(lease, "reload_restore_timeout", || invalidations += 1));
        assert_eq!(invalidations, 1);
        assert_eq!(state.snapshot().phase, RecoveryPhase::Error);
        assert_eq!(
            state.snapshot().reason.as_deref(),
            Some("reload_restore_timeout")
        );
        assert!(
            state.page_navigated(),
            "failed reload must clear its permit"
        );
    }
    #[test]
    fn stale_drop_cannot_clear_a_new_reload_navigation_permit() {
        let state = RecoveryState::default();
        let old_lease = state.lease();
        assert!(state.expect_reload_navigation(old_lease));
        state.reset();
        let new_lease = state.lease();
        assert!(state.expect_reload_navigation(new_lease));

        state.clear_reload_navigation(old_lease);
        assert!(
            !state.page_navigated(),
            "old cleanup must not clear new permit"
        );
        state.clear_reload_navigation(new_lease);
        assert!(state.page_navigated());
    }
    #[test]
    fn page_and_capture_reset_control_recovery_availability() {
        let state = Arc::new(RecoveryState::default());
        assert!(!state.snapshot().can_recover);
        state.page_available(true);
        assert!(state.snapshot().can_recover);
        let lease = state.lease();
        let token = state.token();
        assert!(!state.snapshot().can_recover);
        state.reset();
        assert_ne!(state.lease(), lease);
        token.fail("stale_failure");
        assert_eq!(state.snapshot().phase, RecoveryPhase::Inactive);
        assert!(!state.snapshot().can_recover);
    }
    #[test]
    fn all_consumers_must_commit_and_old_tokens_cannot_resume() {
        let state = Arc::new(RecoveryState::default());
        state.register(BOT | AUTOPLAY);
        let token = state.token();
        token.ack(TRACKER);
        token.ack(BOT);
        assert!(!state.allows_input());
        token.ack(AUTOPLAY);
        assert!(state.allows_input());
        let stale = state.token();
        state.invalidate(RecoveryPhase::Missing, None);
        stale.ack(TRACKER | BOT | AUTOPLAY);
        assert!(!state.allows_input());
    }
}
