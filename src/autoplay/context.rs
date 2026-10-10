//! 浏览器采集与自动操作共用的页面、画布、时间预算及输入确认状态。

use chromiumoxide::page::Page;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct AutoplayContext {
    pub recovery: Arc<crate::capture::recovery::RecoveryState>,
    pub platform: std::sync::RwLock<crate::config::Platform>,
    pub generation: AtomicU64,
    pub autoplay_enabled: AtomicBool,
    pub enabled_changes: tokio::sync::watch::Sender<bool>,
    pub action_changes: tokio::sync::watch::Sender<u64>,
    action_cutoff: std::sync::RwLock<Option<std::time::Instant>>,
    pub tenhou_state: crate::autoplay::tenhou_state::SharedTenhouState,
    pub page: Arc<RwLock<Option<Page>>>,
    pub canvas_rect: Arc<RwLock<Option<CanvasRect>>>,
    /// Server-granted time budget for the current decision window.
    /// Written by the Majsoul bridge (see `autoplay::budget`), read by
    /// the manager's delay model. Uses a `std::sync::RwLock` (not tokio)
    /// because the writer is the bridge's synchronous `parse()` path.
    pub time_budget: crate::autoplay::budget::SharedTimeBudget,
    /// Counter of the client's own uplink input commands, bumped by the
    /// Majsoul bridge as it parses. The manager takes a ticket before a
    /// click and asks afterwards whether the count moved to control the
    /// existing retries. This is not server confirmation of an action.
    pub input_watch: crate::autoplay::verify::SharedInputWatch,
    pub status: Arc<crate::autoplay::status::AutoplayStatus>,
}

impl AutoplayContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel queued/in-flight actions without resetting observation state.
    pub fn invalidate_actions(&self) {
        self.status.invalidate();
        *self.action_cutoff.write().unwrap() = Some(std::time::Instant::now());
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.action_changes.send_replace(generation);
    }

    pub fn set_enabled(&self, enabled: bool) {
        // Serialize pause/user-toggle publications so the UI cannot observe
        // an older switch value after a concurrent update.
        let mut cutoff = self.action_cutoff.write().unwrap();
        if self
            .autoplay_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
            != enabled
        {
            *cutoff = Some(std::time::Instant::now());
            let generation = self
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            self.action_changes.send_replace(generation);
            self.autoplay_enabled
                .store(enabled, std::sync::atomic::Ordering::SeqCst);
            self.enabled_changes.send_replace(enabled);
        }
        self.status.set_enabled(
            enabled && *self.platform.read().unwrap() == crate::config::Platform::Majsoul,
        );
    }

    pub fn accepts_decision(&self, started: Option<std::time::Instant>) -> bool {
        self.recovery.allows_input()
            && self
                .autoplay_enabled
                .load(std::sync::atomic::Ordering::SeqCst)
            && started.is_some_and(|started| {
                self.action_cutoff
                    .read()
                    .unwrap()
                    .is_none_or(|cutoff| started >= cutoff)
            })
    }

    pub async fn page_allowed(&self, page: &Page) -> bool {
        let platform = *self.platform.read().unwrap();
        let game = match platform {
            crate::config::Platform::Majsoul => "\"majsoul\"",
            crate::config::Platform::Tenhou => "\"tenhou\"",
        };
        let expr = include_str!("official_page.js").replace("__AKAGI_GAME__", game);
        let check = page.evaluate(expr);
        matches!(tokio::time::timeout(std::time::Duration::from_secs(2), check).await,
            Ok(Ok(value)) if value.value().and_then(|v| v.as_bool()) == Some(true))
    }
}

/// CSS-pixel bounding rect for the game canvas, as reported by
/// `Element.getBoundingClientRect()`. `(x, y)` is the top-left of the
/// canvas relative to the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CanvasRect {
    /// Translate a 16:9 normalised point (the coordinate system used by
    /// `LOCATION` tables ported from the Python reference) to CSS pixels.
    pub fn pixel(&self, x_norm: f64, y_norm: f64) -> (f64, f64) {
        (
            self.x + (x_norm / 16.0) * self.width,
            self.y + (y_norm / 9.0) * self.height,
        )
    }

    /// Sanity check for a normalised point — clamps off-canvas requests
    /// before we hand them to CDP.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_invalidation_wakes_a_pending_action() {
        let ctx = AutoplayContext::new();
        let mut changes = ctx.action_changes.subscribe();
        ctx.invalidate_actions();
        changes.changed().await.unwrap();
        assert_eq!(
            *changes.borrow(),
            ctx.generation.load(std::sync::atomic::Ordering::SeqCst)
        );
    }
    #[test]
    fn toggle_cancels_old_decisions_without_clearing_observation() {
        let ctx = AutoplayContext::new();
        *ctx.canvas_rect.blocking_write() = Some(CanvasRect {
            x: 1.,
            y: 2.,
            width: 1600.,
            height: 900.,
        });
        let before_enable = std::time::Instant::now();
        ctx.set_enabled(true);
        let generation = ctx.generation.load(std::sync::atomic::Ordering::SeqCst);
        let first = std::time::Instant::now();
        assert!(!ctx.accepts_decision(Some(before_enable)));
        assert!(ctx.accepts_decision(Some(first)));
        ctx.set_enabled(false);
        assert!(!ctx.accepts_decision(Some(first)));
        ctx.set_enabled(true);
        assert!(ctx.generation.load(std::sync::atomic::Ordering::SeqCst) > generation);
        assert!(!ctx.accepts_decision(Some(first)));
        assert!(!ctx.accepts_decision(None)); // history/restored replies never drive input
        assert!(ctx.accepts_decision(Some(std::time::Instant::now())));
        assert!(ctx.canvas_rect.blocking_read().is_some());
        ctx.invalidate_actions(); // page/game change also invalidates queued replies
        assert!(!ctx.accepts_decision(Some(first)));
    }

    #[test]
    fn pixel_translation_centre() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert_eq!(rect.pixel(8.0, 4.5), (800.0, 450.0));
    }

    #[test]
    fn pixel_translation_with_offset() {
        let rect = CanvasRect {
            x: 100.0,
            y: 50.0,
            width: 1280.0,
            height: 720.0,
        };
        let (px, py) = rect.pixel(8.0, 4.5);
        assert!((px - (100.0 + 640.0)).abs() < 1e-9);
        assert!((py - (50.0 + 360.0)).abs() < 1e-9);
    }

    #[test]
    fn contains_inside() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert!(rect.contains(800.0, 450.0));
    }

    #[test]
    fn contains_outside() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert!(!rect.contains(-1.0, 0.0));
        assert!(!rect.contains(0.0, 1000.0));
    }
}
