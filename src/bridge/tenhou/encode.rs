//! mjai action → Tenhou client frame.
//!
//! The inverse of the parsing half: takes the action a bot chose and produces
//! the JSON frame the Tenhou client would have sent for it. Used by
//! [`super::TenhouBridge::build`].
//!
//! Autoplay does *not* send these frames — Tenhou's client owns its board
//! state and freezes if a discard reaches the server without going through
//! its own handler, so `autoplay::tenhou` drives the client's input instead.
//! The encoder stays because it is the executable statement of the protocol
//! and the only implementation of `Bridge::build` for this platform.
//!
//! # Frame inventory
//!
//! | mjai | frame |
//! |---|---|
//! | `dahai` | `{"tag":"D","p":<index>}` |
//! | `reach` | `{"tag":"REACH"}` |
//! | `chi` | `{"tag":"N","type":3,"hai0":<index>,"hai1":<index>}` |
//! | `pon` | `{"tag":"N","type":1,"hai0":<index>,"hai1":<index>}` |
//! | `daiminkan` | `{"tag":"N","type":2}` |
//! | `ankan` | `{"tag":"N","type":4,"hai":<type * 4>}` |
//! | `kakan` | `{"tag":"N","type":5,"hai":<index>}` |
//! | `kita` | `{"tag":"N","type":10}` (sanma; unverified — see below) |
//! | `hora` (tsumo) | `{"tag":"N","type":7}` |
//! | `hora` (ron) | `{"tag":"N","type":6}` |
//! | `ryukyoku` | `{"tag":"N","type":9}` |
//! | `none` | `{"tag":"N"}` |
//!
//! Riichi is genuinely two frames: `{"tag":"REACH"}` first, then — once the
//! server echoes `<REACH who=... step="1"/>` — the discard as its own
//! `{"tag":"D"}`. Callers drive that sequencing; this module only encodes
//! whatever single action it is handed.
//!
//! # Provenance
//!
//! The frame set is cross-checked against <https://github.com/tomohxx/mjai-gateway>,
//! a working mjai-server/Tenhou-client bridge. Every frame above matches it
//! except `kita`, which that gateway has no equivalent for — it is four-player
//! only.
//!
//! # Not encoded here
//!
//! Session control (`JOIN`, `GOK`, `NEXTREADY`) is deliberately absent. Those
//! frames belong to a client that seats *itself* in a lobby; Akagi observes a
//! real client which sends them on its own, and duplicating them would
//! double-confirm menus the user is driving.

use super::meld::{Meld, MeldKind};
use crate::schema::MjaiEvent;
use serde_json::json;

/// The subset of tracked state an encode needs: which physical tiles we hold
/// and which melds we have called. Tenhou addresses tiles by their index in
/// `0..=135` (the specific physical copy), so an mjai tile *string* can only
/// be resolved against a real hand.
#[derive(Debug, Clone, Copy)]
pub struct HandView<'a> {
    /// Our concealed hand as Tenhou tile indices.
    pub hand: &'a [u32],
    /// Melds we have called this kyoku.
    pub melds: &'a [Meld],
    /// True between our tsumo and our dahai — the drawn tile is the tail of
    /// `hand`.
    pub is_tsumo: bool,
}

/// Tile type (`0..=33`) and whether the label names a red five.
fn tile_type(label: &str) -> Option<(usize, bool)> {
    const SUITS: [char; 3] = ['m', 'p', 's'];
    const HONORS: [&str; 7] = ["E", "S", "W", "N", "P", "F", "C"];

    if let Some(pos) = HONORS.iter().position(|h| *h == label) {
        return Some((27 + pos, false));
    }
    let (body, red) = match label.strip_suffix('r') {
        Some(body) => (body, true),
        None => (label, false),
    };
    let mut chars = body.chars();
    let rank = chars.next()?.to_digit(10)?;
    let suit = chars.next()?;
    if chars.next().is_some() || !(1..=9).contains(&rank) {
        return None;
    }
    let suit_idx = SUITS.iter().position(|s| *s == suit)?;
    // Only the fives have a red variant.
    if red && rank != 5 {
        return None;
    }
    Some((suit_idx * 9 + (rank as usize - 1), red))
}

/// The red five of each suit is the 0-th physical copy of its type.
fn is_red_index(index: u32) -> bool {
    matches!(index, 16 | 52 | 88)
}

/// Resolve each mjai tile string to a distinct Tenhou index drawn from `pool`,
/// removing what it matches so two identical labels never collapse onto the
/// same physical tile — `hai0` and `hai1` of a pon must differ.
///
/// Red/plain is matched exactly first. If nothing matches exactly, any copy of
/// the same tile type will do: an exact match can only fail when the hand holds
/// no tile of the requested redness, so every remaining candidate has the
/// *same* redness as every other and the choice between them cannot be wrong.
/// That relaxation matters because some bots normalise red fives away and name
/// a held `5mr` as plain `5m`; refusing would stall autoplay over a distinction
/// that, by then, has only one possible answer.
///
/// Within either pass, candidates are scanned in descending index order, which
/// prefers a non-red copy for a plain request (the red five is always the 0-th
/// copy of its type).
fn take_one(pool: &mut Vec<u32>, label: &str) -> Option<u32> {
    let (ty, want_red) = tile_type(label)?;
    let same_type = |i: u32| i as usize / 4 == ty;
    let pick = |pool: &Vec<u32>, exact: bool| {
        pool.iter()
            .enumerate()
            .filter(|(_, &i)| same_type(i) && (!exact || is_red_index(i) == want_red))
            .map(|(p, _)| p)
            .max_by_key(|&p| pool[p])
    };
    let pos = pick(pool, true).or_else(|| pick(pool, false))?;
    Some(pool.remove(pos))
}

/// [`take_one`] across a list, threading one pool so repeated labels consume
/// distinct copies.
fn take_indices(pool: &mut Vec<u32>, labels: &[String]) -> Option<Vec<u32>> {
    labels.iter().map(|l| take_one(pool, l)).collect()
}

/// Resolve the physical copy a discard should throw.
///
/// Tenhou derives the tedashi/tsumogiri display from the index alone —
/// throwing the just-drawn copy *is* the tsumogiri — so which copy goes
/// matters even between functionally identical tiles: a tsumogiri must name
/// the drawn copy (the tail of `hand`), not whichever identical copy
/// [`take_one`]'s ranking would reach first. When the named tile is not the
/// drawn one, or nothing was drawn, the ordinary red-aware lookup applies.
///
/// Shared by [`encode`] and the autoplay planner so the frame the encoder
/// would build and the index the planner hands the client can never name
/// different copies.
pub fn dahai_index(view: HandView, pai: &str, tsumogiri: bool) -> Option<u32> {
    let drawn = view.is_tsumo.then(|| view.hand.last().copied()).flatten();
    match drawn {
        Some(d) if tsumogiri && super::tile::tenhou_to_mjai_one(d) == pai => Some(d),
        _ => tile_index(view.hand, pai),
    }
}

/// Tenhou tile class (`0..=33`) for an mjai label, exposed for the autoplay
/// planner: a kan is addressed by tile *type*, and red fives share the class
/// of their plain copies.
pub fn tile_class_public(label: &str) -> Option<u32> {
    tile_type(label).map(|(ty, _)| ty as u32)
}

/// Resolve a single mjai tile string against the hand.
fn tile_index(hand: &[u32], label: &str) -> Option<u32> {
    take_one(&mut hand.to_vec(), label)
}

/// Encode one mjai action as the Tenhou client frame that performs it.
///
/// Returns `None` when the action has no Tenhou frame (bridge-only events like
/// `tsumo` / `start_kyoku`) or when no tile of the named *type* is in
/// `view.hand` — the latter means our tracked hand and the bot's view disagree,
/// and sending a guessed index would discard the wrong tile.
pub fn encode(action: &MjaiEvent, view: HandView) -> Option<String> {
    let value = match action {
        MjaiEvent::Dahai { pai, tsumogiri, .. } => {
            json!({ "tag": "D", "p": dahai_index(view, pai, *tsumogiri)? })
        }
        MjaiEvent::Reach { .. } => json!({ "tag": "REACH" }),
        MjaiEvent::Chi { consumed, .. } => {
            let mut pool = view.hand.to_vec();
            let idx = take_indices(&mut pool, consumed)?;
            json!({ "tag": "N", "type": 3, "hai0": idx[0], "hai1": idx[1] })
        }
        MjaiEvent::Pon { consumed, .. } => {
            let mut pool = view.hand.to_vec();
            let idx = take_indices(&mut pool, consumed)?;
            json!({ "tag": "N", "type": 1, "hai0": idx[0], "hai1": idx[1] })
        }
        // Daiminkan consumes every remaining copy, so there is nothing to
        // disambiguate and the frame carries no tile.
        MjaiEvent::Daiminkan { .. } => json!({ "tag": "N", "type": 2 }),
        MjaiEvent::Ankan { consumed, .. } => {
            // The frame names the tile *type*, not a copy: index / 4 * 4.
            let index = tile_index(view.hand, &consumed[0])?;
            json!({ "tag": "N", "type": 4, "hai": index / 4 * 4 })
        }
        MjaiEvent::Kakan { pai, .. } => {
            // The added tile comes from hand; the existing pon supplies the
            // rest. Verify the pon is actually there so a desynced meld list
            // fails here instead of on the wire.
            let index = tile_index(view.hand, pai)?;
            let has_pon = view.melds.iter().any(|m| {
                m.kind == MeldKind::Pon && m.tiles.first().is_some_and(|t| t / 4 == index / 4)
            });
            if !has_pon {
                return None;
            }
            json!({ "tag": "N", "type": 5, "hai": index })
        }
        // Sanma only. Unlike every other frame here this one has a single
        // source and no cross-check: the reference client this module was
        // validated against (https://github.com/tomohxx/mjai-gateway) is
        // four-player only and never encodes a nukidora. If the type is
        // wrong the server ignores the frame and the turn times out — a
        // stall, not a misplay — but it is the one line to suspect first if
        // sanma autoplay never manages to declare kita.
        MjaiEvent::Kita { .. } => json!({ "tag": "N", "type": 10 }),
        // mjai emits both tsumo agari and ron as `hora`; Tenhou splits them.
        // Tsumo is the case where the winning tile came from our own draw,
        // which mjai marks by `actor == target`.
        MjaiEvent::Hora { actor, target, .. } => {
            let kind = if actor == target { 7 } else { 6 };
            json!({ "tag": "N", "type": kind })
        }
        // The only abortive draw a player declares is 九種九牌.
        MjaiEvent::Ryukyoku { .. } => json!({ "tag": "N", "type": 9 }),
        // Decline whatever the server offered. Callers must only send this
        // while a decision window is actually open.
        MjaiEvent::None => json!({ "tag": "N" }),
        _ => return None,
    };
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(hand: &'a [u32], melds: &'a [Meld]) -> HandView<'a> {
        HandView {
            hand,
            melds,
            is_tsumo: false,
        }
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn tile_type_covers_suits_honors_and_reds() {
        assert_eq!(tile_type("1m"), Some((0, false)));
        assert_eq!(tile_type("9m"), Some((8, false)));
        assert_eq!(tile_type("1p"), Some((9, false)));
        assert_eq!(tile_type("1s"), Some((18, false)));
        assert_eq!(tile_type("E"), Some((27, false)));
        assert_eq!(tile_type("C"), Some((33, false)));
        assert_eq!(tile_type("5mr"), Some((4, true)));
        assert_eq!(tile_type("5pr"), Some((13, true)));
        assert_eq!(tile_type("5sr"), Some((22, true)));
        // Only fives are red; anything else is malformed.
        assert_eq!(tile_type("4mr"), None);
        assert_eq!(tile_type("?"), None);
        assert_eq!(tile_type("10m"), None);
    }

    #[test]
    fn dahai_names_the_physical_tile() {
        let hand = [0, 4, 8];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("2m"),
            tsumogiri: false,
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"p":4,"tag":"D"}"#
        );
    }

    /// A plain five must never resolve to the red copy while a plain one is
    /// held, and asking for the red must return exactly index 16.
    #[test]
    fn plain_five_prefers_non_red_copy() {
        let hand = [16, 17, 18];
        let plain = MjaiEvent::Dahai {
            actor: 0,
            pai: s("5m"),
            tsumogiri: false,
        };
        assert_eq!(
            encode(&plain, view(&hand, &[])).unwrap(),
            r#"{"p":18,"tag":"D"}"#
        );
        let red = MjaiEvent::Dahai {
            actor: 0,
            pai: s("5mr"),
            tsumogiri: false,
        };
        assert_eq!(
            encode(&red, view(&hand, &[])).unwrap(),
            r#"{"p":16,"tag":"D"}"#
        );
    }

    /// Holding only the red five, a request for a plain 5m can only mean that
    /// tile — bots that normalise reds away name it this way. Falling back is
    /// unambiguous here precisely because no plain copy exists.
    #[test]
    fn plain_five_falls_back_to_red_when_it_is_the_only_copy() {
        let hand = [16];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("5m"),
            tsumogiri: false,
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"p":16,"tag":"D"}"#
        );
        // The fallback must not fire while an exact match exists.
        let both = [16, 17];
        assert_eq!(
            encode(&ev, view(&both, &[])).unwrap(),
            r#"{"p":17,"tag":"D"}"#,
            "a plain copy is held, so the red must be left alone"
        );
    }

    /// The reverse direction: a red named while only plain copies are held.
    #[test]
    fn red_five_falls_back_to_plain_when_no_red_is_held() {
        let hand = [17, 18];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("5mr"),
            tsumogiri: false,
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"p":18,"tag":"D"}"#
        );
    }

    /// The fallback is per tile *type* only — a tile we do not hold at all
    /// still refuses.
    #[test]
    fn fallback_never_crosses_tile_types() {
        let hand = [16, 17];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("6m"),
            tsumogiri: false,
        };
        assert!(encode(&ev, view(&hand, &[])).is_none());
    }

    /// Regression: with two identical copies in hand, a tsumogiri must name
    /// the drawn copy — the tail — even when the other copy ranks first in
    /// the ordinary lookup. Tenhou reads the tedashi/tsumogiri display off
    /// the index, so the wrong copy shows opponents a tedashi that never
    /// happened.
    #[test]
    fn tsumogiri_of_a_duplicated_tile_names_the_drawn_copy() {
        // Two plain 5m; index 17 was drawn last, 19 ranks first in take_one.
        let hand = [19, 17];
        let v = HandView {
            hand: &hand,
            melds: &[],
            is_tsumo: true,
        };
        assert_eq!(dahai_index(v, "5m", true), Some(17));
        // A tedashi of the same label keeps the ordinary ranking.
        assert_eq!(dahai_index(v, "5m", false), Some(19));
    }

    #[test]
    fn tsumogiri_sends_the_drawn_tile() {
        // Two 1m in hand; the drawn one is the tail (index 1).
        let hand = [0, 4, 1];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("1m"),
            tsumogiri: true,
        };
        let v = HandView {
            hand: &hand,
            melds: &[],
            is_tsumo: true,
        };
        assert_eq!(encode(&ev, v).unwrap(), r#"{"p":1,"tag":"D"}"#);
    }

    /// Regression against the reference implementation, which resolved each
    /// label against the untouched hand and so returned the same index twice
    /// for a pon of two identical plain tiles.
    #[test]
    fn pon_of_identical_tiles_uses_two_distinct_indices() {
        let hand = [4, 5, 6];
        let ev = MjaiEvent::Pon {
            actor: 0,
            target: 3,
            pai: s("2m"),
            consumed: [s("2m"), s("2m")],
        };
        let out = encode(&ev, view(&hand, &[])).unwrap();
        assert_eq!(out, r#"{"hai0":6,"hai1":5,"tag":"N","type":1}"#);
    }

    #[test]
    fn pon_with_red_five_keeps_both_copies_distinct() {
        let hand = [16, 17, 18];
        let ev = MjaiEvent::Pon {
            actor: 0,
            target: 2,
            pai: s("5m"),
            consumed: [s("5m"), s("5mr")],
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"hai0":18,"hai1":16,"tag":"N","type":1}"#
        );
    }

    #[test]
    fn chi_encodes_both_consumed_tiles() {
        // 3m (index 8) + 4m (index 12) to call a 2m.
        let hand = [8, 12, 20];
        let ev = MjaiEvent::Chi {
            actor: 0,
            target: 3,
            pai: s("2m"),
            consumed: [s("3m"), s("4m")],
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"hai0":8,"hai1":12,"tag":"N","type":3}"#
        );
    }

    #[test]
    fn daiminkan_carries_no_tile() {
        let hand = [0, 1, 2];
        let ev = MjaiEvent::Daiminkan {
            actor: 0,
            target: 1,
            pai: s("1m"),
            consumed: [s("1m"), s("1m"), s("1m")],
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"tag":"N","type":2}"#
        );
    }

    #[test]
    fn ankan_names_the_tile_type_not_a_copy() {
        // Four 5m including the red at 16 — the frame must say 16 (type * 4)
        // regardless of which copy `consumed[0]` names.
        let hand = [16, 17, 18, 19];
        let ev = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("5mr"), s("5m"), s("5m"), s("5m")],
        };
        assert_eq!(
            encode(&ev, view(&hand, &[])).unwrap(),
            r#"{"hai":16,"tag":"N","type":4}"#
        );
    }

    #[test]
    fn kakan_requires_the_matching_pon() {
        let hand = [19];
        let pon = Meld {
            kind: MeldKind::Pon,
            target_rel: 1,
            tiles: vec![16, 17, 18],
            unused: None,
            r: Some(0),
        };
        let ev = MjaiEvent::Kakan {
            actor: 0,
            pai: s("5m"),
            consumed: [s("5mr"), s("5m"), s("5m")],
        };
        assert_eq!(
            encode(&ev, view(&hand, std::slice::from_ref(&pon))).unwrap(),
            r#"{"hai":19,"tag":"N","type":5}"#
        );
        // Same hand, no pon of that type tracked → refuse.
        assert!(encode(&ev, view(&hand, &[])).is_none());
    }

    #[test]
    fn hora_splits_tsumo_and_ron() {
        let tsumo = MjaiEvent::Hora {
            actor: 1,
            target: 1,
            deltas: None,
            ura_markers: None,
        };
        assert_eq!(
            encode(&tsumo, view(&[], &[])).unwrap(),
            r#"{"tag":"N","type":7}"#
        );
        let ron = MjaiEvent::Hora {
            actor: 1,
            target: 2,
            deltas: None,
            ura_markers: None,
        };
        assert_eq!(
            encode(&ron, view(&[], &[])).unwrap(),
            r#"{"tag":"N","type":6}"#
        );
    }

    #[test]
    fn simple_declarations() {
        assert_eq!(
            encode(
                &MjaiEvent::Reach {
                    actor: 0,
                    pai: None
                },
                view(&[], &[])
            )
            .unwrap(),
            r#"{"tag":"REACH"}"#
        );
        assert_eq!(
            encode(&MjaiEvent::Ryukyoku { deltas: None }, view(&[], &[])).unwrap(),
            r#"{"tag":"N","type":9}"#
        );
        assert_eq!(
            encode(
                &MjaiEvent::Kita {
                    actor: 0,
                    pai: Some(s("N"))
                },
                view(&[], &[])
            )
            .unwrap(),
            r#"{"tag":"N","type":10}"#
        );
        assert_eq!(
            encode(&MjaiEvent::None, view(&[], &[])).unwrap(),
            r#"{"tag":"N"}"#
        );
    }

    /// Observation-only events have no client frame.
    #[test]
    fn bridge_only_events_encode_to_nothing() {
        assert!(encode(
            &MjaiEvent::Tsumo {
                actor: 0,
                pai: s("1m")
            },
            view(&[], &[])
        )
        .is_none());
        assert!(encode(&MjaiEvent::EndKyoku, view(&[], &[])).is_none());
        assert!(encode(&MjaiEvent::ReachAccepted { actor: 0 }, view(&[], &[])).is_none());
    }

    /// A tile the bot names but we do not hold means our tracked hand is out
    /// of sync; refuse rather than send a wrong index.
    #[test]
    fn unknown_tile_refuses_to_encode() {
        let hand = [0, 4];
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: s("9s"),
            tsumogiri: false,
        };
        assert!(encode(&ev, view(&hand, &[])).is_none());
    }
}
