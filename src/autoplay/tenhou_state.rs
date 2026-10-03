//! Tenhou hand + decision-window slot shared from the bridge to autoplay.
//!
//! Tenhou addresses tiles by index in `0..=135` — the specific physical copy —
//! so encoding a bot action into a client frame needs the hand at that
//! resolution. The riichi engine works in mjai tile strings and cannot supply
//! it, and the autoplay manager holds no bridge handle, so the bridge
//! publishes a snapshot here on every frame it parses.
//!
//! Same shape as [`crate::autoplay::budget`]: bridge writes, manager reads, a
//! `std::sync::RwLock` because the writer is the bridge's synchronous
//! `parse()` path and the reader only takes a copy.
//!
//! # The decision window
//!
//! Tenhou's server marks what we may do with a `t` attribute on the frame that
//! opens a window — the same role Majsoul's `OptionalOperationList` plays. Its
//! bits mean different things depending on which frame carried it, but the two
//! sets do not overlap, so one mask covers both:
//!
//! | bit | on our draw (`T<n>`) | on a discard (`D`/`E`/`F`/`G<n>`) |
//! |---|---|---|
//! | 1 | — | pon |
//! | 2 | — | daiminkan |
//! | 4 | — | chi |
//! | 8 | — | ron |
//! | 16 | tsumo agari | — |
//! | 32 | riichi | — |
//! | 64 | 九種九牌 | — |
//!
//! An opponent's kakan `N` frame carries the same claim bits when the kan
//! can be robbed (chankan) — the client builds its ron menu straight from
//! that attribute, and so does the bridge.
//!
//! Ankan / kakan are not in the mask; the client derives them from the hand,
//! and so does Akagi (via the riichi engine's legal actions). A window opened
//! by our own draw, our own call, or an accepted riichi declaration carries
//! `ops == 0` when the server offered nothing beyond the discard we owe.

use crate::bridge::tenhou::meld::Meld;
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub const OP_PON: u32 = 1;
pub const OP_DAIMINKAN: u32 = 2;
pub const OP_CHI: u32 = 4;
pub const OP_RON: u32 = 8;
pub const OP_TSUMO: u32 = 16;
pub const OP_REACH: u32 = 32;
pub const OP_KYUUSHU: u32 = 64;

/// An open decision window for our seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionWindow {
    /// Raw `t` bitmask from the frame that opened the window. Zero when the
    /// server offered nothing beyond a discard we already owe (post-draw,
    /// post-call, post-riichi-declaration).
    pub ops: u32,
    /// When we decoded the frame that opened the window. Doubles as the
    /// window's identity: if the slot no longer holds the same instant, the
    /// window we planned against has been replaced.
    pub opened_at: Instant,
}

impl DecisionWindow {
    /// Whether the server offered any *optional* claim. False for a window
    /// that only owes a discard, which is what gates sending a decline
    /// (`{"tag":"N"}`) — Tenhou has nothing to decline in that case.
    pub fn has_claim(&self) -> bool {
        self.ops != 0
    }

    /// Whether the window offers a claim on *someone else's* discard, which
    /// is the only kind that can be declined.
    ///
    /// The own-turn offers (tsumo agari, riichi, 九種九牌) share the same
    /// mask but are not declinable: not taking one just means discarding
    /// normally, and the client renders no pass button for them. Treating
    /// `ops != 0` as declinable made autoplay hunt for a pass button that
    /// was never going to exist.
    pub fn has_declinable_claim(&self) -> bool {
        self.ops & (OP_PON | OP_DAIMINKAN | OP_CHI | OP_RON) != 0
    }

    pub fn allows(&self, op: u32) -> bool {
        self.ops & op != 0
    }

    /// Milliseconds since the window opened, saturating.
    pub fn elapsed_ms(&self) -> u32 {
        u32::try_from(self.opened_at.elapsed().as_millis()).unwrap_or(u32::MAX)
    }
}

/// Tenhou's per-turn allowance, in milliseconds.
///
/// Unlike Majsoul, Tenhou never states this on the wire — it is a property of
/// the lobby's rules. The standard allowance is 5 seconds per decision plus a
/// 10-second bank for the whole hand, and running past it does not merely
/// look inhuman: the client auto-discards and the decision is gone. The first
/// live run lost every turn that way, because with no budget at all the delay
/// model ran to its no-budget ceiling of 15s.
pub const TURN_BASE_MS: u32 = 5_000;
pub const TURN_BANK_MS: u32 = 10_000;

/// What the Tenhou bridge knows that the riichi engine cannot express.
#[derive(Debug, Clone, Default)]
pub struct TenhouState {
    /// Our mjai-absolute seat.
    pub seat: u8,
    /// Concealed hand as Tenhou tile indices. The tile drawn this turn, when
    /// there is one, is the tail.
    pub hand: Vec<u32>,
    /// Melds we have called this kyoku.
    pub melds: Vec<Meld>,
    /// True between our tsumo and our dahai.
    pub is_tsumo: bool,
    /// Currently open decision window for our seat, if any.
    pub window: Option<DecisionWindow>,
}

impl TenhouState {
    /// Borrow the parts [`crate::bridge::tenhou::encode::encode`] needs.
    pub fn hand_view(&self) -> crate::bridge::tenhou::encode::HandView<'_> {
        crate::bridge::tenhou::encode::HandView {
            hand: &self.hand,
            melds: &self.melds,
            is_tsumo: self.is_tsumo,
        }
    }
}

/// Shared slot: bridge writes, autoplay manager reads.
pub type SharedTenhouState = Arc<RwLock<Option<TenhouState>>>;

/// Fresh empty slot.
pub fn new_shared() -> SharedTenhouState {
    Arc::new(RwLock::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(ops: u32) -> DecisionWindow {
        DecisionWindow {
            ops,
            opened_at: Instant::now(),
        }
    }

    #[test]
    fn claim_bits_decode_independently() {
        let w = window(OP_PON | OP_CHI);
        assert!(w.allows(OP_PON));
        assert!(w.allows(OP_CHI));
        assert!(!w.allows(OP_DAIMINKAN));
        assert!(!w.allows(OP_RON));
        assert!(w.has_claim());
    }

    /// Own-turn bits live above the claim bits and must not alias them.
    #[test]
    fn draw_bits_do_not_alias_claim_bits() {
        let w = window(OP_TSUMO | OP_REACH | OP_KYUUSHU);
        assert!(w.allows(OP_TSUMO));
        assert!(w.allows(OP_REACH));
        assert!(w.allows(OP_KYUUSHU));
        assert!(!w.allows(OP_PON));
        assert!(!w.allows(OP_CHI));
        assert!(!w.allows(OP_DAIMINKAN));
        assert!(!w.allows(OP_RON));
    }

    /// A window that only owes a discard has nothing to decline.
    #[test]
    fn zero_ops_window_has_no_claim() {
        assert!(!window(0).has_claim());
        assert!(!window(0).has_declinable_claim());
    }

    /// Own-turn offers are not declinable: declining one is just discarding,
    /// and the client draws no pass button for them.
    #[test]
    fn own_turn_offers_are_not_declinable() {
        for ops in [
            OP_TSUMO,
            OP_REACH,
            OP_KYUUSHU,
            OP_TSUMO | OP_REACH | OP_KYUUSHU,
        ] {
            let w = window(ops);
            assert!(w.has_claim(), "still an offer");
            assert!(!w.has_declinable_claim(), "but not one to decline: {ops}");
        }
    }

    /// Claims on another seat's discard are.
    #[test]
    fn claims_on_a_discard_are_declinable() {
        for ops in [OP_PON, OP_CHI, OP_DAIMINKAN, OP_RON, OP_CHI | OP_RON] {
            assert!(window(ops).has_declinable_claim(), "{ops}");
        }
    }
}
