//! Differential checks against normalized, anonymous real game recordings.
use akagi::{
    bot::{native::NativeBot, runner::BotRunner},
    config::AppConfig,
    event_bus,
    game_state::GameTracker,
    schema::MjaiEvent,
};
use std::sync::Arc;
use tokio::sync::RwLock;

async fn check_corpus(source: &str) {
    let events: Vec<MjaiEvent> = source
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let (seat, players) = match &events[0] {
        MjaiEvent::StartGame {
            id: Some(seat),
            num_players,
            ..
        } => (*seat, *num_players),
        _ => panic!("missing identity"),
    };
    let config = Arc::new(RwLock::new(AppConfig::default()));
    let mut live = NativeBot::new(seat, players, config.clone(), event_bus::notify_bus()).unwrap();
    let mut live_tracker = GameTracker::new();
    let mut round_start = 1;
    let mut checked = 0;
    for (index, event) in events.iter().enumerate() {
        if matches!(event, MjaiEvent::StartKyoku { .. }) {
            round_start = index;
        }
        live_tracker.handle(event).unwrap();
        let decision = live.react(std::slice::from_ref(event)).await.unwrap();
        // Every own draw and every actual choice, including claims, kans and kita.
        if !matches!(event, MjaiEvent::Tsumo { actor, .. } if *actor == seat)
            && matches!(decision.action, MjaiEvent::None)
        {
            continue;
        }
        let mut prefix = vec![events[0].clone()];
        prefix.extend_from_slice(&events[round_start..=index]);
        let mut restored_tracker = GameTracker::new();
        for event in &prefix {
            restored_tracker.handle(event).unwrap();
        }
        assert_eq!(
            serde_json::to_value(restored_tracker.snapshot()).unwrap(),
            serde_json::to_value(live_tracker.snapshot()).unwrap(),
            "{players}p snapshot at {index}"
        );
        assert_eq!(
            restored_tracker.our_seat_can_act(),
            live_tracker.our_seat_can_act()
        );
        let mut restored =
            NativeBot::new(seat, players, config.clone(), event_bus::notify_bus()).unwrap();
        restored.restore(&prefix[..prefix.len() - 1]).await.unwrap();
        let actual = restored.react(&prefix[prefix.len() - 1..]).await.unwrap();
        assert_eq!(
            restored.encoded_observation(),
            live.encoded_observation(),
            "{players}p encoded model input at {index}"
        );
        assert_eq!(
            actual.action, decision.action,
            "{players}p action at {index}"
        );
        assert_eq!(actual.meta, decision.meta, "{players}p ranking at {index}");
        checked += 1;
    }
    assert!(checked > 20, "insufficient real decision coverage");
}

#[tokio::test]
async fn restored_four_player_decisions_match_continuous_capture() {
    check_corpus(include_str!("fixtures/midgame_4p.jsonl")).await;
}
#[tokio::test]
async fn restored_three_player_decisions_match_continuous_capture() {
    check_corpus(include_str!("fixtures/midgame_3p.jsonl")).await;
}
