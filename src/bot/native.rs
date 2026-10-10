//! In-process inference using bundled four-player and three-player models.
//! No network client, external runtime or remote fallback exists here.
use crate::bot::runner::BotRunner;
use crate::bot::types::BotResponse;
use crate::config::AppConfig;
use crate::event_bus::NotifyBus;
use crate::game_state::convert;
use crate::schema::MjaiEvent;
use anyhow::Result;
use async_trait::async_trait;
use native_bot::engine::{BotAction, Decision, Engine};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

pub const NATIVE_4P: &str = "akagi-native";
pub const NATIVE_3P: &str = "akagi-native3p";
pub fn is_native(name: &str) -> bool {
    name == NATIVE_4P || name == NATIVE_3P
}

pub fn display_name(name: &str) -> Option<&'static str> {
    match name {
        NATIVE_4P => Some("Akagi (built-in, 4p)"),
        NATIVE_3P => Some("Akagi (built-in, 3p)"),
        _ => None,
    }
}

pub async fn build(
    actor_id: u8,
    num_players: u8,
    config: Arc<RwLock<AppConfig>>,
    notify: NotifyBus,
) -> Result<Box<dyn BotRunner>> {
    Ok(Box::new(NativeBot::new(
        actor_id,
        num_players,
        config,
        notify,
    )?))
}
pub struct NativeBot {
    engine: Engine,
    seat: u8,
}
impl NativeBot {
    pub fn encoded_observation(&mut self) -> Vec<f32> {
        self.engine.encoded_observation()
    }
    pub fn new(
        actor_id: u8,
        num_players: u8,
        _config: Arc<RwLock<AppConfig>>,
        _notify: NotifyBus,
    ) -> Result<Self> {
        Ok(Self {
            engine: native_bot::defaults::engine(num_players, actor_id)?,
            seat: actor_id,
        })
    }
}
#[async_trait]
impl BotRunner for NativeBot {
    async fn restore(&mut self, events: &[MjaiEvent]) -> Result<()> {
        self.feed_events(events)
    }
    async fn suggest_restored(&mut self) -> Result<BotResponse> {
        self.decide(&[])
    }
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse> {
        self.feed_events(events)?;
        self.decide(events)
    }
    async fn reset(&mut self) -> Result<()> {
        self.engine.reset();
        Ok(())
    }
}
impl NativeBot {
    fn feed_events(&mut self, events: &[MjaiEvent]) -> Result<()> {
        for ev in events {
            // Keep our seat current if a start_game tags a (possibly new) seat.
            if let MjaiEvent::StartGame { id: Some(seat), .. } = ev {
                self.seat = *seat;
                self.engine.set_seat(*seat);
            }
            if let Some(ri) = convert::to_riichienv(ev)? {
                self.engine.feed(ri);
            }
        }
        Ok(())
    }
    fn decide(&mut self, events: &[MjaiEvent]) -> Result<BotResponse> {
        // These events close or postpone a decision; they do not open one.
        // BotManager normally buffers them, but guard direct BotRunner users as
        // well because riichienv may retain the preceding legal-action set.
        let waits_for_rinshan = matches!(
            events.last(),
            Some(
                MjaiEvent::Daiminkan { actor, .. }
                    | MjaiEvent::Ankan { actor, .. }
                    | MjaiEvent::Kakan { actor, .. }
            ) if *actor == self.seat
        );
        if matches!(events.last(), Some(MjaiEvent::ReachAccepted { .. })) || waits_for_rinshan {
            return Ok(BotResponse {
                decision_context: Default::default(),
                decision_started: None,
                action: MjaiEvent::None,
                meta: None,
            });
        }

        // Local gate: nothing to decide ⇒ reply `none`, spend no API call, and
        // leave whatever card is on screen alone.
        let local = match self.engine.decide()? {
            Some(d) if is_decision_point(&d.candidates) => d,
            _ => {
                return Ok(BotResponse {
                    decision_context: Default::default(),
                    decision_started: None,
                    action: MjaiEvent::None,
                    meta: None,
                })
            }
        };

        let (action, meta) = local_reply(&local, self.seat);
        Ok(BotResponse {
            action,
            meta,
            decision_context: Default::default(),
            decision_started: None,
        })
    }
}
fn is_decision_point(candidates: &[(BotAction, f32)]) -> bool {
    !matches!(candidates, [] | [(BotAction::Pass, _)])
}

fn local_reply(local: &Decision, seat: u8) -> (MjaiEvent, Option<Value>) {
    let meta = build_show_meta(&local.candidates);
    (bot_action_to_mjai(local.action.clone(), seat), meta)
}

fn with_lead(lead: &str, rest: &[String]) -> Vec<String> {
    std::iter::once(lead.to_string())
        .chain(rest.iter().cloned())
        .collect()
}

fn make_show_item(label: &str, pais: &[String], prob: Option<f64>) -> Value {
    use serde_json::json;
    let mut item = serde_json::Map::new();
    item.insert("label".into(), json!(label));
    if pais.iter().any(|p| !p.is_empty()) {
        item.insert("pais".into(), json!(pais));
    }
    if let Some(p) = prob {
        item.insert("value".into(), json!(format!("{:.0}%", p * 100.0)));
        // Raw probability for machine consumers (`autoplay::delay::probs`);
        // the formatted `value` above only has integer-percent resolution.
        item.insert("prob".into(), json!(p));
    }
    Value::Object(item)
}

fn wrap_show(items: Vec<Value>, title: &str) -> Option<Value> {
    use serde_json::json;
    if items.is_empty() {
        return None;
    }
    Some(json!({ "show": { "title": title, "items": items } }))
}

fn take_n<const N: usize>(v: Vec<String>) -> [String; N] {
    let mut it = v.into_iter();
    std::array::from_fn(|_| it.next().unwrap_or_default())
}

fn build_show_meta(candidates: &[(BotAction, f32)]) -> Option<serde_json::Value> {
    let items: Vec<Value> = candidates
        .iter()
        .map(|(a, p)| {
            let (label, pais) = label_pais_bot_action(a);
            make_show_item(label, &pais, Some(*p as f64))
        })
        .collect();
    wrap_show(items, SHOW_TITLE_LOCAL)
}

fn label_pais_bot_action(a: &BotAction) -> (&'static str, Vec<String>) {
    match a {
        BotAction::Dahai { pai, .. } => ("Discard", vec![pai.clone()]),
        BotAction::Reach { pai } => (
            "Riichi",
            if pai.is_empty() {
                vec![]
            } else {
                vec![pai.clone()]
            },
        ),
        BotAction::Pon { pai, consumed, .. } => ("Pon", with_lead(pai, consumed)),
        BotAction::Chi { pai, consumed, .. } => ("Chi", with_lead(pai, consumed)),
        BotAction::Daiminkan { pai, consumed, .. } => ("Kan", with_lead(pai, consumed)),
        BotAction::Ankan { consumed } => ("Ankan", consumed.clone()),
        BotAction::Kakan { pai, consumed } => ("Kakan", with_lead(pai, consumed)),
        BotAction::Hora { .. } => ("Hora", vec![]),
        BotAction::Kyushu => ("Ryukyoku", vec![]),
        BotAction::Kita => ("Kita", vec!["N".into()]),
        // No tiles: the row is about *declining* the discard on the table, and
        // drawing that tile here would read as a recommendation to play it.
        BotAction::Pass => (PASS_LABEL, vec![]),
    }
}

fn bot_action_to_mjai(a: BotAction, actor: u8) -> MjaiEvent {
    match a {
        BotAction::Dahai { pai, tsumogiri } => MjaiEvent::Dahai {
            actor,
            pai,
            tsumogiri,
        },
        BotAction::Reach { pai } => MjaiEvent::Reach {
            actor,
            pai: Some(pai),
        },
        BotAction::Pon {
            target,
            pai,
            consumed,
        } => MjaiEvent::Pon {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Chi {
            target,
            pai,
            consumed,
        } => MjaiEvent::Chi {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Daiminkan {
            target,
            pai,
            consumed,
        } => MjaiEvent::Daiminkan {
            actor,
            target,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Ankan { consumed } => MjaiEvent::Ankan {
            actor,
            consumed: take_n(consumed),
        },
        BotAction::Kakan { pai, consumed } => MjaiEvent::Kakan {
            actor,
            pai,
            consumed: take_n(consumed),
        },
        BotAction::Hora { target } => MjaiEvent::Hora {
            actor,
            target,
            deltas: None,
            ura_markers: None,
        },
        BotAction::Kyushu => MjaiEvent::Ryukyoku { deltas: None },
        BotAction::Kita => MjaiEvent::Kita {
            actor,
            pai: Some("N".into()),
        },
        BotAction::Pass => MjaiEvent::None,
    }
}

const SHOW_TITLE_LOCAL: &str = "Akagi · Local";
const PASS_LABEL: &str = "Pass";
#[cfg(test)]
mod tests {
    use super::*;
    fn cfg_off() -> Arc<RwLock<AppConfig>> {
        Arc::new(RwLock::new(AppConfig::default()))
    }

    fn opening() -> Vec<MjaiEvent> {
        vec![
            start_game_4p(0),
            start_kyoku_4p(),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "5p".into(),
            },
        ]
    }

    fn start_game_4p(seat: u8) -> MjaiEvent {
        MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(seat),
            num_players: 4,
            game_meta: None,
        }
    }

    fn start_kyoku_4p() -> MjaiEvent {
        let hand: Vec<String> = [
            "1m", "1m", "3m", "5m", "7m", "9m", "2p", "4p", "6p", "8p", "1s", "3s", "5s",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000, 25000, 25000, 25000],
            tehais: vec![hand.clone(), hand.clone(), hand.clone(), hand],
            num_players: 4,
        }
    }

    fn start_kyoku_4p_hidden(our_hand: [&str; 13]) -> MjaiEvent {
        // The bridge's placeholder for a tile we cannot see (mjai's `?`).
        let hidden = vec!["?".to_string(); 13];
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000; 4],
            tehais: vec![
                our_hand.iter().map(|s| s.to_string()).collect(),
                hidden.clone(),
                hidden.clone(),
                hidden,
            ],
            num_players: 4,
        }
    }

    fn seat1_discards(pai: &str) -> [MjaiEvent; 2] {
        [
            MjaiEvent::Tsumo {
                actor: 1,
                pai: pai.into(),
            },
            MjaiEvent::Dahai {
                actor: 1,
                pai: pai.into(),
                tsumogiri: true,
            },
        ]
    }

    async fn bot_with(config: Arc<RwLock<AppConfig>>, notify: NotifyBus) -> NativeBot {
        NativeBot::new(0, 4, config, notify).unwrap()
    }
    #[tokio::test]
    async fn a_declined_call_still_produces_a_card_ranking_the_pass() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        let mut events = vec![
            start_game_4p(0),
            start_kyoku_4p_hidden([
                "9s", "9s", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p",
            ]),
        ];
        events.extend(seat1_discards("9s"));

        let resp = bot.react(&events).await.unwrap();

        let meta = resp
            .meta
            .as_ref()
            .expect("a pon window is a decision — it must refresh the card");
        let items = meta["show"]["items"].as_array().unwrap();
        let labels: Vec<&str> = items
            .iter()
            .map(|i| i["label"].as_str().unwrap_or_default())
            .collect();
        assert!(
            labels.contains(&"Pass"),
            "the pass must be a ranked row, got {labels:?}"
        );
        assert!(
            labels.contains(&"Pon"),
            "…alongside the call it is weighed against, got {labels:?}"
        );
        assert!(
            items.iter().all(|i| i["value"].is_string()),
            "every row carries the probability the player is here to read"
        );
    }

    #[tokio::test]
    async fn an_uncallable_opponent_discard_leaves_the_card_alone() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        let mut events = vec![
            start_game_4p(0),
            // No 9s in hand, and seat 1 is our shimocha so a chi is impossible.
            start_kyoku_4p_hidden([
                "1m", "3m", "5m", "7m", "9m", "2p", "4p", "6p", "8p", "1s", "3s", "5s", "7s",
            ]),
        ];
        events.extend(seat1_discards("9s"));

        let resp = bot.react(&events).await.unwrap();

        assert!(matches!(resp.action, MjaiEvent::None));
        assert!(
            resp.meta.is_none(),
            "nothing was decided, so nothing should replace the card on screen"
        );
    }

    #[tokio::test]
    async fn native_bot_returns_legal_discard_on_own_tsumo() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        // Feed the opening up to our first draw in one batch (as the manager would).
        let resp = bot.react(&opening()).await.unwrap();
        // On our own tsumo we must act — a discard (or riichi/kan/hora), never None.
        assert!(
            !matches!(resp.action, MjaiEvent::None),
            "bot must act on its own tsumo, got None"
        );
        match resp.action {
            MjaiEvent::Dahai { actor, .. } | MjaiEvent::Reach { actor, .. } => {
                assert_eq!(actor, 0)
            }
            MjaiEvent::Ankan { .. } | MjaiEvent::Kakan { .. } | MjaiEvent::Hora { .. } => {}
            other => panic!("unexpected reply on own turn: {other:?}"),
        }
    }

    #[tokio::test]
    async fn restored_suggestion_decides_from_fed_history_without_feeding_it_again() {
        let events = opening();
        let mut restored = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        restored.restore(&events).await.unwrap();
        let suggestion = restored.suggest_restored().await.unwrap();

        let mut ordinary = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        let reaction = ordinary.react(&events).await.unwrap();
        assert_eq!(suggestion.action, reaction.action);
        assert_eq!(suggestion.meta, reaction.meta);
    }

    #[tokio::test]
    async fn native_bot_passes_when_not_its_turn() {
        let mut bot = bot_with(cfg_off(), crate::event_bus::notify_bus()).await;
        // Opponent (seat 1) draws and discards; we (seat 0) usually can't act.
        let resp = bot
            .react(&[
                start_game_4p(0),
                start_kyoku_4p(),
                MjaiEvent::Tsumo {
                    actor: 1,
                    pai: "9s".into(),
                },
                MjaiEvent::Dahai {
                    actor: 1,
                    pai: "9s".into(),
                    tsumogiri: true,
                },
            ])
            .await
            .unwrap();
        // Either None (nothing to do) or a legal call — must not be one of our
        // own-turn-only actions.
        assert!(
            !matches!(
                resp.action,
                MjaiEvent::Dahai { .. } | MjaiEvent::Reach { .. }
            ),
            "must not discard on someone else's turn: {:?}",
            resp.action
        );
    }

    #[test]
    fn local_show_meta_lists_top_candidates_with_probs() {
        let cands = vec![
            (
                BotAction::Dahai {
                    pai: "1m".into(),
                    tsumogiri: false,
                },
                0.6f32,
            ),
            (BotAction::Reach { pai: "2p".into() }, 0.3f32),
            (
                BotAction::Dahai {
                    pai: "9s".into(),
                    tsumogiri: true,
                },
                0.1f32,
            ),
        ];
        let meta = build_show_meta(&cands).unwrap();
        assert_eq!(
            meta["show"]["title"], SHOW_TITLE_LOCAL,
            "a local decision must be titled as such"
        );
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3, "should surface all three candidates");
        assert_eq!(items[0]["label"], "Discard");
        assert_eq!(items[0]["pais"][0], "1m");
        assert_eq!(items[0]["value"], "60%");
        assert_eq!(items[1]["label"], "Riichi");
        assert_eq!(items[1]["pais"][0], "2p");
        assert_eq!(items[1]["value"], "30%");
        assert_eq!(items[2]["pais"][0], "9s");
        assert_eq!(items[2]["value"], "10%");
    }

    #[test]
    fn local_show_meta_ranks_the_pass_against_the_call_it_declined() {
        let cands = vec![
            (BotAction::Pass, 0.9f32),
            (
                BotAction::Pon {
                    target: 0,
                    pai: "1m".into(),
                    consumed: vec!["1m".into(), "1m".into()],
                },
                0.1f32,
            ),
        ];

        let meta = build_show_meta(&cands).expect("a declined call still has a card to show");
        let items = meta["show"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "both the pass and the pon it beat");
        assert_eq!(items[0]["label"], "Pass");
        assert_eq!(items[0]["value"], "90%");
        assert!(
            items[0].get("pais").is_none(),
            "the pass row must draw no tile — it would read as a discard suggestion"
        );
        assert_eq!(items[1]["label"], "Pon");
        assert_eq!(items[1]["value"], "10%");
    }

    #[test]
    fn a_lone_pass_is_not_a_decision_point() {
        assert!(
            !is_decision_point(&[]),
            "no legal action at all is not a decision"
        );
        assert!(
            !is_decision_point(&[(BotAction::Pass, 1.0)]),
            "someone else's call window is not our decision"
        );
    }

    #[test]
    fn a_choice_is_a_decision_point() {
        let call_window = [
            (BotAction::Pass, 0.9f32),
            (
                BotAction::Pon {
                    target: 0,
                    pai: "1m".into(),
                    consumed: vec!["1m".into(), "1m".into()],
                },
                0.1f32,
            ),
        ];
        assert!(
            is_decision_point(&call_window),
            "pass vs pon is exactly the decision the card exists to explain"
        );

        // Our own turn: no pass on offer, and even a single forced discard is a
        // decision the player wants to see.
        let forced_discard = [(
            BotAction::Dahai {
                pai: "1m".into(),
                tsumogiri: true,
            },
            1.0f32,
        )];
        assert!(is_decision_point(&forced_discard));
    }
}
