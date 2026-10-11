//! Protocol -> tracker -> bundled bot -> HUD payload -> autoplay plan regressions.
//! Server confirmation is simulated; these tests do not operate a live game.
use akagi::{
    autoplay::{
        majsoul::{
            coords::{action_button_pos, MajsoulOpType},
            MajsoulAutoplay,
        },
        platform::{ActionContext, PlatformAutoplay, Step},
        status::AutoplayStatus,
    },
    bot::{BotManager, BotResponse},
    bridge::{Bridge, Direction, MajsoulBridge, ParseResult},
    capture::recovery::{RecoveryPhase, RecoveryState},
    config::AppConfig,
    event_bus::{self, AutomaticTsumogiri, EventContext, TrackedEvent},
    game_state::GameTracker,
    inspector::InspectorWriter,
    schema::MjaiEvent,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use prost::Message;
use riichienv_core::{
    action::Action, state::legal_actions::GameStateLegalActions,
    state_3p::legal_actions::GameState3PLegalActions,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

fn wire(kind: u8, name: &str, message: &str, value: Value) -> Vec<u8> {
    #[derive(prost::Message)]
    struct Wrapper {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(bytes = "vec", tag = "2")]
        data: Vec<u8>,
    }
    let descriptor = akagi::bridge::majsoul::parser::POOL
        .get_message_by_name(message)
        .unwrap();
    let payload = prost_reflect::DynamicMessage::deserialize(descriptor, value).unwrap();
    let mut frame = if kind == 1 { vec![1] } else { vec![kind, 1, 0] };
    frame.extend(
        Wrapper {
            name: name.into(),
            data: payload.encode_to_vec(),
        }
        .encode_to_vec(),
    );
    frame
}

fn action_wire(step: u32, name: &str, value: Value) -> Vec<u8> {
    let descriptor = akagi::bridge::majsoul::parser::POOL
        .get_message_by_name(&format!("lq.{name}"))
        .unwrap();
    let mut bytes = prost_reflect::DynamicMessage::deserialize(descriptor, value)
        .unwrap()
        .encode_to_vec();
    // Live ActionPrototype uses position-dependent XOR; restoration does not.
    let keys = [0x84, 0x5e, 0x4e, 0x42, 0x39, 0xa2, 0x1f, 0x60, 0x1c];
    let base = 23 ^ bytes.len();
    for (i, b) in bytes.iter_mut().enumerate() {
        *b ^= (base + 5 * i + keys[i % keys.len()]) as u8;
    }
    wire(
        1,
        ".lq.ActionPrototype",
        "lq.ActionPrototype",
        json!({"step":step,"name":name,"data":STANDARD.encode(bytes)}),
    )
}

struct Game {
    bridge: MajsoulBridge,
    tracker: GameTracker,
    manager: BotManager,
    responses: broadcast::Receiver<BotResponse>,
    recovery: Arc<RecoveryState>,
    status: Arc<AutoplayStatus>,
    step: u32,
    _dir: tempfile::TempDir,
}
impl Game {
    async fn new(players: u8) -> Self {
        let recovery = Arc::new(RecoveryState::default());
        let status = Arc::new(AutoplayStatus::default());
        status.set_enabled(true);
        let bridge = MajsoulBridge::default()
            .with_recovery(Some(recovery.clone()))
            .with_autoplay_status(Some(status.clone()));
        let dir = tempfile::tempdir().unwrap();
        let inspector = InspectorWriter::open(&dir.path().join("inspector.jsonl"), 128)
            .unwrap()
            .0;
        let bus = event_bus::bot_response_bus();
        let responses = bus.subscribe();
        let mut config = AppConfig::default();
        config.autoplay.enabled = true;
        let manager = BotManager::new(
            Arc::new(RwLock::new(config)),
            bus,
            event_bus::bot_status_bus(),
            event_bus::notify_bus(),
            inspector,
        );
        let mut game = Self {
            bridge,
            tracker: GameTracker::new(),
            manager,
            responses,
            recovery,
            status,
            step: 0,
            _dir: dir,
        };
        game.bridge.parse(
            Direction::Up,
            &wire(
                2,
                ".lq.FastTest.authGame",
                "lq.ReqAuthGame",
                json!({"account_id":100,"game_uuid":"riichi-regression"}),
            ),
        );
        let seats = if players == 3 {
            vec![1, 2, 100]
        } else {
            vec![1, 2, 100, 3]
        };
        let auth = game.bridge.parse(
            Direction::Down,
            &wire(3, "", "lq.ResAuthGame", json!({"seat_list":seats})),
        );
        game.feed(auth).await;
        game.action(
            "ActionNewRound",
            json!({"chang":0,"ju":0,"ben":0,
            "scores":vec![25000;players as usize],"doras":["1p"],
            "tiles":["1p","2p","3p","4p","5p","6p","7p","8p","9p","2s","3s","4s","5s"]}),
        )
        .await;
        game.action(
            "ActionDealTile",
            json!({"seat":2,"tile":"9s","operation":{
            "seat":2,"time_fixed":10000,"operation_list":[{"type":1},{"type":7}]}}),
        )
        .await;
        game.drain(); // Discard/riichi recommendation before server confirmation.
        game.action(
            "ActionDiscardTile",
            json!({"seat":2,"tile":"9s","is_liqi":true}),
        )
        .await;
        game.action("ActionDealTile", json!({"seat":0})).await;
        assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
        assert!(game.tracker.snapshot().unwrap().players[2].riichi_declared);
        game
    }
    async fn feed(&mut self, parsed: ParseResult) {
        assert!(!parsed.invalidated);
        assert!(!parsed.is_restore);
        for event in parsed.events {
            self.tracker.handle(&event).unwrap();
            self.manager
                .handle_tracked(TrackedEvent {
                    event,
                    can_act: self.tracker.our_seat_can_act(),
                    context: parsed.context.clone(),
                    restore: None,
                })
                .await
                .unwrap();
        }
    }
    async fn action(&mut self, name: &str, value: Value) {
        let parsed = self
            .bridge
            .parse(Direction::Down, &action_wire(self.step, name, value));
        self.step += 1;
        self.feed(parsed).await;
    }
    fn drain(&mut self) -> Vec<BotResponse> {
        let mut out = Vec::new();
        while let Ok(response) = self.responses.try_recv() {
            out.push(response);
        }
        out
    }
    fn plan(&self, response: &BotResponse) -> Vec<Step> {
        let snapshot = self.tracker.snapshot().unwrap();
        let legal: Vec<Action> = match snapshot.num_players {
            3 => self
                .tracker
                .state_3p()
                .unwrap()
                ._get_legal_actions_internal(2),
            _ => self.tracker.state().unwrap()._get_legal_actions_internal(2),
        };
        MajsoulAutoplay::new()
            .plan(&ActionContext {
                tenhou: None,
                action: &response.action,
                snapshot: &snapshot,
                legal_actions: &legal,
                our_seat: 2,
                last_kawa_tile: Some("5s"),
                last_self_tsumo: Some("5s"),
                self_riichi_accepted: true,
                num_players: snapshot.num_players,
                cfg: &Default::default(),
                delay_cfg: Default::default(),
                budget: None,
                probs: None,
                delay_script: None,
            })
            .steps
    }
}

async fn riichi_draws_then_win(players: u8, tsumo: bool, empty_operation: bool) {
    let mut game = Game::new(players).await;
    for tile in ["1z", "7p", "1s"] {
        let mut deal = json!({"seat":2,"tile":tile});
        if empty_operation {
            deal["operation"] = json!({"seat":2,"time_fixed":10000,"operation_list":[]});
        }
        game.action("ActionDealTile", deal).await;
        let draws = game.drain();
        assert_eq!(draws.len(), 1, "every automatic draw must update the HUD");
        let draw = &draws[0];
        let pai = akagi::bridge::majsoul::tile::ms_to_mjai(tile).unwrap();
        assert_eq!(
            draw.action,
            MjaiEvent::Dahai {
                actor: 2,
                pai: pai.into(),
                tsumogiri: true
            }
        );
        assert_eq!(
            draw.meta.as_ref().unwrap()["show"]["items"][0]["pais"],
            json!([pai])
        );
        assert!(draw.decision_context.valid_for_display());
        assert!(!draw.decision_context.valid());
        assert!(draw.decision_started.is_none());
        assert!(game
            .plan(draw)
            .iter()
            .all(|s| !matches!(s, Step::Click { .. })));
        assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
        // The client's automatic input closes the old window without putting
        // the restored model into Error or disabling the next win window.
        game.bridge.parse(
            Direction::Up,
            &wire(
                2,
                ".lq.FastTest.inputOperation",
                "lq.ReqSelfOperation",
                json!({"type":1,"tile":tile,"moqie":true,"auto_operation":true,"timeuse":1}),
            ),
        );
        game.action(
            "ActionDiscardTile",
            json!({"seat":2,"tile":tile,"moqie":true}),
        )
        .await;
        assert!(
            !draw.decision_context.valid_for_display(),
            "server echo expires the old draw"
        );
    }
    let op = if tsumo { 8 } else { 9 };
    let operation = json!({"seat":2,"time_fixed":10000,"operation_list":[{"type":op}]});
    if tsumo {
        game.action(
            "ActionDealTile",
            json!({"seat":2,"tile":"5s","operation":operation}),
        )
        .await;
    } else {
        game.action("ActionDealTile", json!({"seat":1})).await;
        game.action(
            "ActionDiscardTile",
            json!({"seat":1,"tile":"5s","operation":operation}),
        )
        .await;
    }
    let responses = game.drain();
    assert_eq!(responses.len(), 1);
    let response = &responses[0];
    assert!(
        matches!(response.action, MjaiEvent::Hora { actor:2, target,.. }
        if target == if tsumo { 2 } else { 1 })
    );
    assert!(response.decision_context.valid());
    assert!(response.decision_context.permits(&response.action));
    assert!(response.decision_started.is_some());
    assert!(response.decision_context.automatic_tsumogiri.is_none());
    let clicks: Vec<_> = game
        .plan(response)
        .into_iter()
        .filter_map(|s| match s {
            Step::Click { x_norm, y_norm } => Some((x_norm, y_norm)),
            _ => None,
        })
        .collect();
    let win_button = if tsumo {
        MajsoulOpType::Zimo
    } else {
        MajsoulOpType::Ron
    };
    assert_eq!(
        clicks,
        vec![action_button_pos(&[MajsoulOpType::None, win_button], win_button).unwrap()],
        "autoplay must press the correct win button, not Pass or a hand tile"
    );

    // Simulate dispatch followed by the matching client request and server win.
    let observation = game
        .status
        .begin(&response.action, response.decision_context.window, 2);
    assert!(observation.can_press());
    observation.pressed();
    observation.released();
    observation.finish();
    let (method, message) = if tsumo {
        (".lq.FastTest.inputOperation", "lq.ReqSelfOperation")
    } else {
        (".lq.FastTest.inputChiPengGang", "lq.ReqChiPengGang")
    };
    game.bridge.parse(
        Direction::Up,
        &wire(2, method, message, json!({"type":op,"timeuse":1})),
    );
    game.action("ActionHule", json!({"hules":[{"seat":2,"zimo":tsumo}]}))
        .await;
    assert_eq!(
        serde_json::to_value(game.status.snapshot()).unwrap()["records"][0]["phase"],
        "succeeded"
    );
    assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
}

#[tokio::test]
async fn four_player_riichi_draws_remain_visible_and_later_ron_clicks() {
    for empty in [false, true] {
        riichi_draws_then_win(4, false, empty).await;
    }
}
#[tokio::test]
async fn four_player_riichi_draws_remain_visible_and_later_tsumo_clicks() {
    for empty in [false, true] {
        riichi_draws_then_win(4, true, empty).await;
    }
}
#[tokio::test]
async fn three_player_riichi_draws_remain_visible_and_later_ron_clicks() {
    for empty in [false, true] {
        riichi_draws_then_win(3, false, empty).await;
    }
}
#[tokio::test]
async fn three_player_riichi_draws_remain_visible_and_later_tsumo_clicks() {
    for empty in [false, true] {
        riichi_draws_then_win(3, true, empty).await;
    }
}

#[tokio::test]
async fn three_player_riichi_kita_prompt_keeps_its_executable_window() {
    let mut game = Game::new(3).await;
    game.action(
        "ActionDealTile",
        json!({"seat":2,"tile":"4z","operation":{
        "seat":2,"time_fixed":10000,"operation_list":[{"type":11}]}}),
    )
    .await;
    let responses = game.drain();
    assert_eq!(responses.len(), 1);
    assert!(responses[0].decision_context.automatic_tsumogiri.is_none());
    assert!(responses[0].decision_context.valid());
    assert!(responses[0].decision_context.permits(&responses[0].action));
    assert!(responses[0].decision_started.is_some());
    assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
}

#[tokio::test]
async fn expired_riichi_ron_window_cannot_authorize_a_response_or_click() {
    let mut game = Game::new(4).await;
    game.action("ActionDealTile", json!({"seat":1})).await;
    let ron = game.bridge.parse(
        Direction::Down,
        &action_wire(
            game.step,
            "ActionDiscardTile",
            json!({"seat":1,"tile":"5s","operation":{
            "seat":2,"time_fixed":1,"operation_list":[{"type":9}]}}),
        ),
    );
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert!(!ron.context.valid());
    game.feed(ron).await;
    assert_eq!(game.tracker.our_seat_can_act(), Some(true));
    assert!(game.drain().is_empty());
    assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
}

#[tokio::test]
async fn stale_riichi_draw_cannot_be_shown_or_consumed_by_the_model() {
    let mut game = Game::new(4).await;
    let draw = game.bridge.parse(
        Direction::Down,
        &action_wire(game.step, "ActionDealTile", json!({"seat":2,"tile":"1z"})),
    );
    game.step += 1;
    assert!(draw.context.automatic_tsumogiri.is_some());
    game.recovery.window(None); // A later frame supersedes this draw before consumption.
    assert!(!draw.context.valid_for_display());
    // The stale event still keeps model state current via the pending batch,
    // but must not produce a suggestion or a click.
    game.feed(draw).await;
    assert!(game.drain().is_empty());
    assert_eq!(game.recovery.snapshot().phase, RecoveryPhase::Ready);
}

#[tokio::test]
async fn recorded_failed_riichi_rounds_show_every_draw_and_resume_all_three_ron_windows() {
    // Anonymous Oct 11 recordings: the original runs had no bot response at
    // any of these three ron windows after one or more automatic draws.
    let events: Vec<MjaiEvent> = include_str!("fixtures/riichi_after_draw_4p.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let inspector = InspectorWriter::open(&dir.path().join("inspector.jsonl"), 128)
        .unwrap()
        .0;
    let bus = event_bus::bot_response_bus();
    let mut responses = bus.subscribe();
    let mut manager = BotManager::new(
        Arc::new(RwLock::new(AppConfig::default())),
        bus,
        event_bus::bot_status_bus(),
        event_bus::notify_bus(),
        inspector,
    );
    let state = Arc::new(RecoveryState::default());
    let status = AutoplayStatus::default();
    status.bind_game(1, 1);
    state.set(RecoveryPhase::Ready, None);
    let mut tracker = GameTracker::new();
    let mut seat = 0;
    let mut reached = false;
    let mut draws = 0;
    let mut wins = 0;
    for (index, event) in events.into_iter().enumerate() {
        if let MjaiEvent::StartGame {
            id: Some(actor), ..
        } = event
        {
            seat = actor;
        }
        if matches!(event, MjaiEvent::StartKyoku { .. }) {
            reached = false;
        }
        if matches!(event, MjaiEvent::ReachAccepted { actor } if actor == seat) {
            reached = true;
        }
        tracker.handle(&event).unwrap();
        let legal = tracker.state().unwrap()._get_legal_actions_internal(seat);
        let ron = reached
            && legal
                .iter()
                .any(|a| a.action_type == riichienv_core::action::ActionType::Ron);
        let draw = if reached {
            match &event {
                MjaiEvent::Tsumo { actor, pai } if *actor == seat => Some(pai.clone()),
                _ => None,
            }
        } else {
            None
        };
        let context = if reached {
            // Exercise the formerly failing case: an empty operation object
            // on automatic draws, then a fresh operation 9 on the real ron.
            let op = json!({"seat":seat,"time_fixed":10000});
            status.window(
                1,
                index as u64,
                None,
                (ron || draw.is_some()).then_some(&op),
                seat,
            );
            let window = status.current_window();
            state.window(window);
            EventContext {
                window,
                token: Some(state.stamp()),
                offered: if ron { vec![9] } else { vec![] },
                forced_tsumogiri: true,
                automatic_tsumogiri: draw.as_ref().map(|pai| AutomaticTsumogiri {
                    revision: state.window_revision(),
                    actor: seat,
                    pai: pai.clone(),
                }),
                advisory_revision: None,
            }
        } else {
            EventContext::default()
        };
        manager
            .handle_tracked(TrackedEvent {
                event,
                can_act: tracker.our_seat_can_act(),
                context,
                restore: None,
            })
            .await
            .unwrap();
        let mut emitted = Vec::new();
        while let Ok(response) = responses.try_recv() {
            emitted.push(response);
        }
        if let Some(pai) = draw {
            draws += 1;
            assert_eq!(emitted.len(), 1, "recorded draw {index}");
            assert_eq!(
                emitted[0].action,
                MjaiEvent::Dahai {
                    actor: seat,
                    pai: pai.clone(),
                    tsumogiri: true
                }
            );
            assert_eq!(
                emitted[0].meta.as_ref().unwrap()["show"]["items"][0]["pais"],
                json!([pai])
            );
            assert!(emitted[0].decision_context.valid_for_display());
            assert!(!emitted[0].decision_context.valid());
        }
        if ron {
            wins += 1;
            assert_eq!(emitted.len(), 1, "recorded ron {index}");
            assert!(matches!(emitted[0].action, MjaiEvent::Hora { actor,.. } if actor == seat));
            assert!(emitted[0].decision_context.valid());
            assert!(emitted[0].decision_started.is_some());
        }
        assert_eq!(
            state.snapshot().phase,
            RecoveryPhase::Ready,
            "recorded event {index}"
        );
    }
    assert_eq!(draws, 15);
    assert_eq!(wins, 3);
}
