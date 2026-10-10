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
        *self.window.lock().unwrap() = window;
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
        self.window(None);
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.set_locked(phase, reason);
        epoch
    }
    pub fn reset(&self) {
        *self.reload_navigation.lock().unwrap() = None;
        self.page_available.store(false, Ordering::SeqCst);
        self.lease.fetch_add(1, Ordering::SeqCst);
        *self.owner.lock().unwrap() = None;
        self.invalidate(RecoveryPhase::Inactive, None);
        self.changes.send_modify(|s| s.method = None);
        self.batch(0, None, None);
    }
    pub fn expect_reload_navigation(&self) {
        *self.reload_navigation.lock().unwrap() =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
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
        state.expect_reload_navigation();
        assert!(!state.page_navigated());
        assert_eq!(state.lease(), lease);
        assert!(state.page_navigated());
        assert_ne!(state.lease(), lease);
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
