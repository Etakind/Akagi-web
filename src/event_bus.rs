//! In-process broadcast buses connecting Akagi's subsystems.
//!
//! Broadcast buses; the game stream wraps its sender to distinguish live
//! events, atomic restoration batches and invalidation without changing MJAI JSON:
//!
//! - [`MjaiBus`]: every `MjaiEvent` parsed by a platform bridge is fanned
//!   out here. Producers: bridge → proxy handler. Consumers: `BotManager`,
//!   `ipc` forwarder, future HUD/storage/WS server.
//! - [`BotResponseBus`]: every `BotResponse` from the active `BotRunner`.
//!   Producer: `BotManager`. Consumers: `ipc` forwarder, future HUD /
//!   external WS / replay recorder.
//! - [`BotStatusBus`]: lifecycle of the active bot subprocess
//!   (`Idle/Loading/Ready/Error/Stopped`). Producer: `BotManager`.
//!   Consumer: `ipc` forwarder (UI loading spinner).
//! - [`CaptureStatusBus`]: lifecycle of the active capture backend
//!   (`Stopped/Starting/Running/Error` × `kind: Chromium`).
//!   Producer: `ipc::commands` / capture supervisor. Consumer: `ipc`
//!   forwarder.
//! - [`NotifyBus`]: ad-hoc toast notifications. Any subsystem may push;
//!   `ipc` forwards to the frontend as `notify` events.
//!
//! Channel capacity is fixed-size — slow consumers see `RecvError::Lagged`
//! rather than blocking the producer. That's the right trade-off for a
//! real-time analyzer: if the HUD falls behind, drop and resync rather
//! than stall the proxy.

use crate::analysis::result::AnalysisResult;
use crate::bot::BotResponse;
use crate::schema::{BotStatus, CaptureStatus, HistoryEvent, MjaiEvent, Notification};
use tokio::sync::broadcast;

/// Fan-out for `MjaiEvent`s from platform bridges.
/// Transport metadata is deliberately outside standard MJAI JSON.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventContext {
    pub window: Option<crate::autoplay::status::Window>,
    pub token: Option<std::sync::Arc<crate::capture::recovery::RestoreToken>>,
    pub offered: Vec<u32>,
    pub forced_tsumogiri: bool,
}
impl EventContext {
    pub fn valid(&self) -> bool {
        self.token.as_ref().is_none_or(|token| {
            token.current() && token.state.allows_input() && token.state.window_valid(self.window)
        })
    }
    pub fn permits(&self, event: &MjaiEvent) -> bool {
        if self.token.is_none() {
            return true;
        }
        let kind = match event {
            MjaiEvent::None => {
                return self
                    .offered
                    .iter()
                    .any(|k| matches!(k, 0 | 2 | 3 | 4 | 5 | 6 | 8 | 9 | 10 | 11))
            }
            MjaiEvent::Dahai { .. } => 1,
            MjaiEvent::Chi { .. } => 2,
            MjaiEvent::Pon { .. } => 3,
            MjaiEvent::Ankan { .. } => 4,
            MjaiEvent::Daiminkan { .. } => 5,
            MjaiEvent::Kakan { .. } => 6,
            MjaiEvent::Reach { .. } => 7,
            MjaiEvent::Hora { actor, target, .. } => {
                if actor == target {
                    8
                } else {
                    9
                }
            }
            MjaiEvent::Ryukyoku { .. } => 10,
            MjaiEvent::Kita { .. } => 11,
            _ => return false,
        };
        // After accepted riichi the client may offer only a prompt (kan,
        // kita, tsumo); a tsumogiri suggestion declines that prompt.
        self.offered.contains(&kind)
            || (self.forced_tsumogiri
                && matches!(
                    event,
                    MjaiEvent::Dahai {
                        tsumogiri: true,
                        ..
                    }
                )
                && self.offered.iter().any(|k| matches!(k, 4 | 8 | 11)))
    }
}
#[derive(Clone, Debug)]
pub enum GameUpdate {
    Live(MjaiEvent, EventContext),
    Restore(std::sync::Arc<Vec<MjaiEvent>>, EventContext),
    Invalidated,
}
impl GameUpdate {
    pub fn events(&self) -> &[MjaiEvent] {
        match self {
            Self::Live(event, _) => std::slice::from_ref(event),
            Self::Restore(events, _) => events,
            Self::Invalidated => &[],
        }
    }
}
#[derive(Clone)]
pub struct MjaiBus(broadcast::Sender<GameUpdate>);
impl MjaiBus {
    pub fn subscribe(&self) -> broadcast::Receiver<GameUpdate> {
        self.0.subscribe()
    }
    pub fn send(
        &self,
        event: MjaiEvent,
    ) -> Result<usize, Box<broadcast::error::SendError<GameUpdate>>> {
        self.send_update(GameUpdate::Live(event, EventContext::default()))
    }
    pub fn send_update(
        &self,
        update: GameUpdate,
    ) -> Result<usize, Box<broadcast::error::SendError<GameUpdate>>> {
        self.0.send(update).map_err(Box::new)
    }
}

/// Fan-out for `BotResponse`s from the active bot.
pub type BotResponseBus = broadcast::Sender<BotResponse>;

/// Fan-out for `BotStatus` lifecycle transitions.
pub type BotStatusBus = broadcast::Sender<BotStatus>;

/// Fan-out for `CaptureStatus` lifecycle transitions.
pub type CaptureStatusBus = broadcast::Sender<CaptureStatus>;

/// Fan-out for transient `Notification`s pushed at the user.
pub type NotifyBus = broadcast::Sender<Notification>;

/// Fan-out for `AnalysisResult`s produced after each game-state update.
/// Producer: `analysis::runner`. Consumers: `ipc` forwarder, future HUD.
pub type AnalysisBus = broadcast::Sender<AnalysisResult>;

/// One `MjaiEvent` as re-emitted after the `GameTracker` applied it.
///
/// `can_act` rides with the event rather than being read off the tracker
/// on arrival, and that is the whole point of the type: a subscriber that
/// pauses between events (the bot manager waits on inference) would
/// otherwise ask a tracker that has moved on, and get the answer for a
/// later event than the one it is holding. One frame can carry several
/// seats' actions, so that is not a rare race — it is most of them.
#[derive(Debug, Clone)]
pub struct TrackedEvent {
    pub event: MjaiEvent,
    /// Whether the riichi engine offers our seat a choice in the state this
    /// event produced — its own turn, or a claim on someone's discard.
    ///
    /// `None` when the engine has no opinion to give: no game in progress,
    /// or no seat tagged (observer / replay). Consumers treat that as "no
    /// opinion" and fall back to their own policy rather than going silent.
    pub can_act: Option<bool>,
    pub context: EventContext,
    pub restore: Option<std::sync::Arc<Vec<MjaiEvent>>>,
}

/// Post-tracker fan-out: each event re-emitted *after* the `GameTracker`
/// has applied it to the engine state, carrying what the engine then had
/// to say about our seat. Subscribers can rely on the live game-state
/// mirror being current when this fires (vs. the raw `MjaiBus` where
/// ordering against the tracker is racy).
pub type PostTrackerBus = broadcast::Sender<TrackedEvent>;

/// Fan-out for game-history lifecycle events. Producer:
/// `crate::history::recorder` (on each finalised game / deletion).
/// Consumer: `ipc` forwarder, which emits `history-recorded` to the
/// frontend.
pub type HistoryBus = broadcast::Sender<HistoryEvent>;

/// Default capacity. Live pacing produces ~1 second of mjai events at a time
/// (start_kyoku + 13 tehai + a few tsumo/dahai pairs), which is tiny. The
/// sizing constraint is the **one-shot GameRestore replay**: on reconnect the
/// bridge emits an entire kyoku's events in a single synchronous burst (the
/// CDP send loop does not yield between `send`s), and consumers treat overflow
/// as `Lagged → skip`. A skipped mid-hand `dahai`/`pon` would silently corrupt
/// the game-state tracker with no self-heal until the next kyoku, so the buffer
/// must comfortably exceed a worst-case full-kyoku event count (~a few hundred).
pub const DEFAULT_CAPACITY: usize = 1024;

/// Smaller buffer for status / notification streams — these are bursty
/// but low-rate; 64 is plenty.
pub const STATUS_CAPACITY: usize = 64;

pub fn mjai_bus() -> MjaiBus {
    // Drop the placeholder receiver — real consumers subscribe later via
    // `Sender::subscribe`. The sender stays alive as long as anyone holds
    // a clone of it.
    let (tx, _rx) = broadcast::channel(DEFAULT_CAPACITY);
    MjaiBus(tx)
}

pub fn bot_response_bus() -> BotResponseBus {
    let (tx, _rx) = broadcast::channel(DEFAULT_CAPACITY);
    tx
}

pub fn bot_status_bus() -> BotStatusBus {
    let (tx, _rx) = broadcast::channel(STATUS_CAPACITY);
    tx
}

pub fn capture_status_bus() -> CaptureStatusBus {
    let (tx, _rx) = broadcast::channel(STATUS_CAPACITY);
    tx
}

pub fn notify_bus() -> NotifyBus {
    let (tx, _rx) = broadcast::channel(STATUS_CAPACITY);
    tx
}

pub fn analysis_bus() -> AnalysisBus {
    let (tx, _rx) = broadcast::channel(DEFAULT_CAPACITY);
    tx
}

pub fn post_tracker_bus() -> PostTrackerBus {
    let (tx, _rx) = broadcast::channel(DEFAULT_CAPACITY);
    tx
}

pub fn history_bus() -> HistoryBus {
    let (tx, _rx) = broadcast::channel(STATUS_CAPACITY);
    tx
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::capture::recovery::{RecoveryPhase, RecoveryState};
    use std::sync::Arc;
    #[test]
    fn decision_keeps_original_window_and_capture_generation() {
        let state = Arc::new(RecoveryState::default());
        let ledger = crate::autoplay::status::AutoplayStatus::default();
        ledger.bind_game(1, 7);
        ledger.window(
            1,
            1,
            None,
            Some(&serde_json::json!({"seat":0,"time_fixed":10000})),
            0,
        );
        state.set(RecoveryPhase::Ready, None);
        let window = ledger.current_window();
        state.window(window);
        let context = EventContext {
            window,
            token: Some(state.stamp()),
            ..Default::default()
        };
        assert!(context.valid());
        ledger.window(
            1,
            2,
            None,
            Some(&serde_json::json!({"seat":0,"time_fixed":10000})),
            0,
        );
        state.window(ledger.current_window());
        assert!(!context.valid());
        let new = EventContext {
            window: ledger.current_window(),
            token: Some(state.stamp()),
            ..Default::default()
        };
        assert!(new.valid());
        state.invalidate(RecoveryPhase::Missing, None);
        assert!(!new.valid());
    }
    #[test]
    fn server_operations_and_riichi_prompt_must_match() {
        let state = Arc::new(RecoveryState::default());
        let mut context = EventContext {
            token: Some(state.stamp()),
            offered: vec![8],
            ..Default::default()
        };
        let discard = MjaiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: true,
        };
        assert!(!context.permits(&discard));
        context.forced_tsumogiri = true;
        assert!(context.permits(&discard));
        context.offered = vec![1];
        assert!(context.permits(&discard));
        assert!(!context.permits(&MjaiEvent::None));
        assert!(!context.permits(&MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: "1m".into(),
            consumed: ["1m".into(), "1m".into()]
        }));
        context.offered = vec![3];
        assert!(context.permits(&MjaiEvent::None));
    }
}
