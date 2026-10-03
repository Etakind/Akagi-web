//! Platform-specific protocol bridges.
//!
//! A `Bridge` translates between a game platform's wire protocol and the
//! mjai JSONL event stream consumed by AI bots. One bridge instance per
//! independent game session (e.g. one Majsoul WebSocket flow).

pub mod majsoul;
pub mod tenhou;

pub use majsoul::MajsoulBridge;

use crate::{
    logger::{FlowLogger, Session},
    schema::{MjaiEvent, ParsedFrame},
};
use std::sync::Arc;

/// Direction of a parsed frame relative to the proxied client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Client → server (uplink, e.g. requests).
    Up,
    /// Server → client (downlink, e.g. responses, notifies).
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub events: Vec<MjaiEvent>,
    pub parsed: Option<ParsedFrame>,
}

impl ParseResult {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn just_events(events: Vec<MjaiEvent>) -> Self {
        Self {
            events,
            parsed: None,
        }
    }
}

/// Translates raw platform frames to mjai events and vice-versa.
pub trait Bridge: Send {
    /// Parse a raw platform frame into zero or more mjai events plus an
    /// optional structured view for the inspector.
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult;

    /// Build a raw platform frame from an mjai command, if applicable.
    fn build(&mut self, command: &MjaiEvent) -> Option<Vec<u8>>;
}

#[derive(Clone, Default)]
pub struct BridgeHooks {
    pub tenhou_state: Option<crate::autoplay::tenhou_state::SharedTenhouState>,
    /// Majsoul: the server's per-decision-window time budget, taken from
    /// `OptionalOperationList` (see `autoplay::budget`).
    pub time_budget: Option<crate::autoplay::budget::SharedTimeBudget>,
    /// Majsoul: counter bumped on every uplink input command, so autoplay can
    /// verify a click registered (see `autoplay::verify`).
    pub input_watch: Option<crate::autoplay::verify::SharedInputWatch>,
    pub notify: Option<crate::event_bus::NotifyBus>,
}

/// Construct a bridge for the given platform.
///
/// - `flow_log`: per-WS-flow text dump (one JSON line per parsed message).
/// - `session`: passed through to bridges that open additional log files
///   on demand (e.g. Majsoul rotates a fresh `*.mjai.jsonl` per game).
/// - `hooks`: autoplay's shared slots — see [`BridgeHooks`].
pub fn for_platform(
    platform: crate::config::Platform,
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    hooks: BridgeHooks,
) -> Box<dyn Bridge> {
    match platform {
        crate::config::Platform::Tenhou => Box::new(
            tenhou::TenhouBridge::new(flow_log, session)
                .with_shared_state(hooks.tenhou_state)
                .with_notify(hooks.notify),
        ),
        crate::config::Platform::Majsoul => Box::new(
            MajsoulBridge::new(flow_log, session)
                .with_time_budget(hooks.time_budget)
                .with_input_watch(hooks.input_watch),
        ),
    }
}
