//! Recovery integration checks for stale authenticated Majsoul flows.
use akagi::bridge::{Bridge, Direction, MajsoulBridge};
use akagi::capture::recovery::{RecoveryPhase, RecoveryState};
use serde_json::Value as JsonValue;
use std::sync::Arc;

fn wire(kind: u8, name: &str, message: &str, value: JsonValue) -> Vec<u8> {
    use prost::Message;

    #[derive(prost::Message)]
    struct Wrapper {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(bytes = "vec", tag = "2")]
        data: Vec<u8>,
    }

    let descriptor = akagi::bridge::majsoul::parser::POOL
        .get_message_by_name(message)
        .expect("test message descriptor");
    let payload = prost_reflect::DynamicMessage::deserialize(descriptor, value)
        .expect("test payload matches descriptor");
    let wrapper = Wrapper {
        name: name.to_owned(),
        data: payload.encode_to_vec(),
    };
    let mut frame = if kind == 1 {
        vec![kind]
    } else {
        vec![kind, 1, 0]
    };
    frame.extend(wrapper.encode_to_vec());
    frame
}

fn authenticate(bridge: &mut MajsoulBridge, account_id: u64) {
    bridge.parse(
        Direction::Up,
        &wire(
            2,
            ".lq.FastTest.authGame",
            "lq.ReqAuthGame",
            serde_json::json!({
                "account_id": account_id,
                "game_uuid": "recovery-test-game",
            }),
        ),
    );
    let result = bridge.parse(
        Direction::Down,
        &wire(
            3,
            "",
            "lq.ResAuthGame",
            serde_json::json!({"seat_list": [account_id, 1, 2, 3]}),
        ),
    );
    assert_eq!(result.events.len(), 1, "auth must identify the test seat");
}

#[test]
fn superseded_flow_cannot_end_the_current_game() {
    let state = Arc::new(RecoveryState::default());
    let mut old_flow = MajsoulBridge::default().with_recovery(Some(state.clone()));
    authenticate(&mut old_flow, 100);
    assert!(state.owns(1), "first flow should own the recovery state");

    let mut current_flow = MajsoulBridge::default().with_recovery(Some(state.clone()));
    authenticate(&mut current_flow, 100);
    assert!(
        !state.owns(1),
        "new authentication must supersede the old flow"
    );
    assert_eq!(state.snapshot().phase, RecoveryPhase::WaitingRound);

    let stale = old_flow.parse(
        Direction::Down,
        &wire(
            1,
            ".lq.NotifyGameEndResult",
            "lq.NotifyGameEndResult",
            serde_json::json!({"result": {}}),
        ),
    );

    assert!(
        stale.events.is_empty(),
        "a superseded flow must not terminate the current game"
    );
    assert_eq!(state.snapshot().phase, RecoveryPhase::WaitingRound);

    let current = current_flow.parse(
        Direction::Down,
        &wire(
            1,
            ".lq.NotifyGameEndResult",
            "lq.NotifyGameEndResult",
            serde_json::json!({"result": {}}),
        ),
    );
    assert_eq!(
        current.events.len(),
        1,
        "the current owner must still receive game-end notifications"
    );
}
