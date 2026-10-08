//! Behavioral coverage for the Majsoul autoplay operation ledger.
//!
//! These tests intentionally drive the public status API rather than a real
//! browser.  A client request only associates a click with a record; server
//! feedback is the evidence that can turn it into a success.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use akagi::autoplay::status::{AutoplayStatus, Phase};
use akagi::schema::MjaiEvent;
use serde_json::{json, Value};

const FLOW: u64 = 41;
const GAME: u64 = 9001;

fn status_with_window() -> Arc<AutoplayStatus> {
    let status = Arc::new(AutoplayStatus::default());
    status.bind_game(FLOW, GAME);
    status.set_enabled(true);
    let operation = json!({"seat": 0, "time_fixed": 5000, "time_add": 0});
    status.window(FLOW, 10, Some("E1:0".into()), Some(&operation), 0);
    status
}

fn records(status: &AutoplayStatus) -> Vec<Value> {
    serde_json::to_value(status.snapshot())
        .expect("status snapshot serializes")
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .expect("records array")
}

fn phase(status: &AutoplayStatus, index: usize) -> Phase {
    match records(status)[index]["phase"]
        .as_str()
        .expect("phase string")
    {
        "preparing" => Phase::Preparing,
        "scheduled" => Phase::Scheduled,
        "executing" => Phase::Executing,
        "awaiting_feedback" => Phase::AwaitingFeedback,
        "succeeded" => Phase::Succeeded,
        "failed" => Phase::Failed,
        "cancelled" => Phase::Cancelled,
        "unconfirmed" => Phase::Unconfirmed,
        other => panic!("unknown phase {other}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn action_record(
    status: &Arc<AutoplayStatus>,
    action: MjaiEvent,
    request_id: u16,
    request: Value,
    step: u64,
    feedback_name: &str,
    feedback_data: Value,
    events: Vec<MjaiEvent>,
) {
    let operation = json!({"seat": 0, "time_fixed": 5000, "time_add": 0});
    status.window(FLOW, step, Some("E1:0".into()), Some(&operation), 0);
    let window = status.current_window();
    let observation = status.begin(&action, window, 0);
    observation.preparing("input", 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, request_id, &request);
    status.feedback(FLOW, step + 1, feedback_name, &feedback_data, &events);
}

#[test]
fn countdown_is_monotonic_and_never_publishes_zero() {
    let status = status_with_window();
    let action = MjaiEvent::Dahai {
        actor: 0,
        pai: "5m".into(),
        tsumogiri: false,
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.preparing("input", 0);
    observation.scheduled(Duration::from_millis(100));

    let first = status.poll().expect("scheduled record is dirty");
    let first_ms = first.records[0].remaining_ms.expect("countdown");
    assert!(first_ms > 0, "a countdown must never display zero");
    thread::sleep(Duration::from_millis(20));
    let second = status.poll().expect("active countdown remains published");
    let second_ms = second.records[0].remaining_ms.expect("countdown");
    assert!(second_ms > 0, "a countdown must never display zero");
    assert!(second_ms <= first_ms, "countdown must be monotonic");
}

#[test]
fn an_unclicked_record_cannot_be_confirmed_by_a_request() {
    let status = status_with_window();
    let observation = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    observation.preparing("input", 0);
    status.request(FLOW, 70, &json!({"type": 1, "tile": "1m", "moqie": false}));
    status.response(FLOW, 70, &json!({}));
    assert_eq!(phase(&status, 0), Phase::Cancelled);
}

#[test]
fn hover_rechecks_window_and_enabled_state_before_mouse_pressed() {
    let status = status_with_window();
    let operation = json!({"seat": 0, "time_fixed": 1, "time_add": 0});
    status.window(FLOW, 11, Some("E1:0".into()), Some(&operation), 0);
    let expired = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "1p".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    thread::sleep(Duration::from_millis(3));
    assert!(
        !expired.can_press(),
        "expired hover must not send mousePressed"
    );
    assert_eq!(phase(&status, 0), Phase::Cancelled);

    let disabled = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "1s".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    status.set_enabled(false);
    assert!(
        !disabled.can_press(),
        "disabled autoplay must not send mousePressed"
    );
    assert_eq!(phase(&status, 1), Phase::Cancelled);
}

#[test]
fn duplicate_press_and_client_timeout_requests_never_become_success() {
    let status = status_with_window();
    let first = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "2m".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    first.pressed();
    first.released();
    first.finish();
    let request = json!({"type": 1, "tile": "2m", "moqie": false, "timeuse": 3});
    status.request(FLOW, 71, &request);
    status.request(FLOW, 71, &request);
    status.response(FLOW, 71, &json!({}));
    assert_eq!(phase(&status, 0), Phase::Unconfirmed);

    let second = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "2p".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    second.pressed();
    second.released();
    second.finish();
    status.request(
        FLOW,
        72,
        &json!({"type": 1, "tile": "2p", "moqie": false, "timeuse": 1_000_000}),
    );
    status.response(FLOW, 72, &json!({}));
    assert_eq!(phase(&status, 1), Phase::Unconfirmed);
}

#[test]
fn cdp_failure_allows_only_the_existing_retry_to_resume_input() {
    let status = status_with_window();
    let observation = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "2s".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    observation.preparing("input", 0);
    observation.failed();
    assert_eq!(phase(&status, 0), Phase::Failed);

    // A verifier retry already chosen by the manager can reopen this same
    // decision window.  Telemetry must not invent retry zero or suppress the
    // retry because the previous CDP attempt failed.
    observation.preparing("input", 0);
    assert_eq!(phase(&status, 0), Phase::Failed);
    observation.preparing("input", 1);
    assert_eq!(phase(&status, 0), Phase::Preparing);
    observation.scheduled(Duration::from_millis(100));
    let update = status.poll().expect("retry countdown is published");
    assert!(update.records[0].remaining_ms.unwrap_or(0) > 0);
    assert!(
        observation.can_press(),
        "the existing retry may press in-window"
    );

    observation.pressed();
    observation.released();
    observation.failed();
    observation.finish();
    assert_eq!(phase(&status, 0), Phase::Failed);
    assert_ne!(phase(&status, 0), Phase::Succeeded);
}

#[test]
fn restore_elapsed_expires_the_current_window_without_moving_its_opening() {
    let status = status_with_window();
    let operation = json!({"seat": 0, "time_fixed": 50, "time_add": 0});
    status.window(FLOW, 12, Some("E1:0".into()), Some(&operation), 0);
    let observation = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "3s".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    status.restore_elapsed(FLOW, Duration::from_secs(1));
    assert!(
        !observation.can_press(),
        "restore elapsed time must consume expiry"
    );
    assert_eq!(phase(&status, 0), Phase::Cancelled);
}

#[test]
fn feedback_step_isolated_from_the_next_decision_window() {
    let status = status_with_window();
    let action = MjaiEvent::Dahai {
        actor: 0,
        pai: "4s".into(),
        tsumogiri: false,
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 73, &json!({"type": 1, "tile": "4s", "moqie": false}));

    let next_operation = json!({"seat": 0, "time_fixed": 5000, "time_add": 0});
    status.window(FLOW, 11, Some("E1:0".into()), Some(&next_operation), 0);
    status.feedback(
        FLOW,
        12,
        "ActionDiscardTile",
        &json!({}),
        &[MjaiEvent::Dahai {
            actor: 0,
            pai: "4s".into(),
            tsumogiri: false,
        }],
    );
    assert_ne!(phase(&status, 0), Phase::Succeeded);
}

#[test]
fn client_request_and_old_input_proof_do_not_claim_success() {
    let status = status_with_window();
    let action = MjaiEvent::Dahai {
        actor: 0,
        pai: "5m".into(),
        tsumogiri: false,
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.preparing("input", 0);
    observation.pressed();
    observation.released();
    observation.finish();

    status.request(FLOW, 77, &json!({"type": 1, "tile": "5m", "moqie": false}));
    status.response(FLOW, 77, &json!({}));
    assert_eq!(phase(&status, 0), Phase::AwaitingFeedback);

    // A stale verifier result or a repeated client request is not server
    // evidence and must not manufacture a success state.
    status.response(FLOW, 77, &json!({"error": {"code": 0}}));
    assert_eq!(phase(&status, 0), Phase::AwaitingFeedback);
}

#[test]
fn server_feedback_confirms_red_discard_and_riichi_discard() {
    let status = status_with_window();

    action_record(
        &status,
        MjaiEvent::Dahai {
            actor: 0,
            pai: "5mr".into(),
            tsumogiri: false,
        },
        80,
        json!({"type": 1, "tile": "0m", "moqie": false}),
        10,
        "ActionDiscardTile",
        json!({"seat": 0, "tile": "0m"}),
        vec![MjaiEvent::Dahai {
            actor: 0,
            pai: "5mr".into(),
            tsumogiri: false,
        }],
    );
    assert_eq!(phase(&status, 0), Phase::Succeeded);

    // Riichi is only successful when the action prototype proves both the
    // declaration and the explicitly selected discard tile.
    action_record(
        &status,
        MjaiEvent::Reach {
            actor: 0,
            pai: Some("5mr".into()),
        },
        81,
        json!({"type": 7, "tile": "0m"}),
        12,
        "ActionDiscardTile",
        json!({"seat": 0, "tile": "0m", "is_liqi": true}),
        vec![],
    );
    assert_eq!(phase(&status, 1), Phase::Succeeded);
}

#[test]
fn reach_request_without_tile_waits_for_server_tile_evidence() {
    let status = status_with_window();
    let action = MjaiEvent::Reach {
        actor: 0,
        pai: Some("5mr".into()),
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();

    // Real inputOperation reach frames can carry only type/index.  The
    // request associates the press, but the server echo must still name the
    // declared tile and mark it as riichi before success is shown.
    status.request(FLOW, 82, &json!({"type": 7, "index": 5, "timeuse": 3}));
    assert_eq!(phase(&status, 0), Phase::AwaitingFeedback);
    status.feedback(
        FLOW,
        11,
        "ActionDiscardTile",
        &json!({"seat": 0, "tile": "0m", "is_liqi": true}),
        &[],
    );
    assert_eq!(phase(&status, 0), Phase::Succeeded);
}

#[test]
fn server_feedback_confirms_calls_wins_kita_and_abortive_draw() {
    let status = status_with_window();

    action_record(
        &status,
        MjaiEvent::Chi {
            actor: 0,
            target: 1,
            pai: "3m".into(),
            consumed: ["2m".into(), "4m".into()],
        },
        90,
        json!({"type": 2}),
        20,
        "ActionChiPengGang",
        json!({"seat": 0}),
        vec![MjaiEvent::Chi {
            actor: 0,
            target: 1,
            pai: "3m".into(),
            consumed: ["2m".into(), "4m".into()],
        }],
    );
    action_record(
        &status,
        MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: "7p".into(),
            consumed: ["7p".into(), "7p".into()],
        },
        91,
        json!({"type": 3}),
        22,
        "ActionChiPengGang",
        json!({"seat": 0}),
        vec![MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: "7p".into(),
            consumed: ["7p".into(), "7p".into()],
        }],
    );
    action_record(
        &status,
        MjaiEvent::Daiminkan {
            actor: 0,
            target: 1,
            pai: "9s".into(),
            consumed: ["9s".into(), "9s".into(), "9s".into()],
        },
        92,
        json!({"type": 5}),
        24,
        "ActionChiPengGang",
        json!({"seat": 0}),
        vec![MjaiEvent::Daiminkan {
            actor: 0,
            target: 1,
            pai: "9s".into(),
            consumed: ["9s".into(), "9s".into(), "9s".into()],
        }],
    );
    action_record(
        &status,
        MjaiEvent::Ankan {
            actor: 0,
            consumed: ["E".into(), "E".into(), "E".into(), "E".into()],
        },
        93,
        json!({"type": 4}),
        26,
        "ActionChiPengGang",
        json!({"seat": 0}),
        vec![MjaiEvent::Ankan {
            actor: 0,
            consumed: ["E".into(), "E".into(), "E".into(), "E".into()],
        }],
    );
    action_record(
        &status,
        MjaiEvent::Kakan {
            actor: 0,
            pai: "4p".into(),
            consumed: ["4p".into(), "4p".into(), "4p".into()],
        },
        94,
        json!({"type": 6}),
        28,
        "ActionChiPengGang",
        json!({"seat": 0}),
        vec![MjaiEvent::Kakan {
            actor: 0,
            pai: "4p".into(),
            consumed: ["4p".into(), "4p".into(), "4p".into()],
        }],
    );
    action_record(
        &status,
        MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: None,
            ura_markers: None,
        },
        95,
        json!({"type": 9}),
        30,
        "ActionHule",
        json!({"seat": 0}),
        vec![MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: None,
            ura_markers: None,
        }],
    );
    action_record(
        &status,
        MjaiEvent::Kita {
            actor: 0,
            pai: Some("N".into()),
        },
        96,
        json!({"type": 11}),
        32,
        "ActionBaBei",
        json!({"seat": 0}),
        vec![MjaiEvent::Kita {
            actor: 0,
            pai: Some("N".into()),
        }],
    );
    action_record(
        &status,
        MjaiEvent::Ryukyoku { deltas: None },
        97,
        json!({"type": 10}),
        34,
        "ActionLiuJu",
        json!({"seat": 0, "type": 1}),
        vec![],
    );

    assert!(records(&status)
        .iter()
        .all(|record| record["phase"] == "succeeded"));
}

#[test]
fn skip_can_be_confirmed_by_its_associated_no_error_response() {
    let status = status_with_window();
    let observation = status.begin(&MjaiEvent::None, status.current_window(), 0);
    observation.preparing("input", 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 101, &json!({"cancel_operation": true}));
    status.response(FLOW, 101, &json!({}));
    assert_eq!(phase(&status, 0), Phase::Succeeded);

    let observation = status.begin(&MjaiEvent::None, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 102, &json!({"cancel_operation": true}));
    status.response(FLOW, 102, &json!({"error": {"code": 1}}));
    assert_eq!(phase(&status, 1), Phase::Failed);
}

#[test]
fn wrong_feedback_window_or_actor_cannot_confirm_an_operation() {
    let status = status_with_window();
    let action = MjaiEvent::Dahai {
        actor: 0,
        pai: "4p".into(),
        tsumogiri: false,
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 110, &json!({"type": 1, "tile": "4p", "moqie": false}));

    let event = MjaiEvent::Dahai {
        actor: 1,
        pai: "4p".into(),
        tsumogiri: false,
    };
    status.feedback(
        FLOW,
        10,
        "ActionDiscardTile",
        &json!({}),
        std::slice::from_ref(&event),
    );
    assert_eq!(phase(&status, 0), Phase::AwaitingFeedback);

    status.feedback(FLOW, 11, "ActionDiscardTile", &json!({}), &[event]);
    assert_eq!(
        phase(&status, 0),
        Phase::AwaitingFeedback,
        "feedback from another seat is unrelated, not a failure proof",
    );
}

#[test]
fn missing_feedback_becomes_unconfirmed_but_late_same_scene_feedback_can_update() {
    let status = status_with_window();
    let action = MjaiEvent::Dahai {
        actor: 0,
        pai: "6s".into(),
        tsumogiri: false,
    };
    let observation = status.begin(&action, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 115, &json!({"type": 1, "tile": "6s", "moqie": false}));

    thread::sleep(Duration::from_millis(5_100));
    let _ = status.poll().expect("timeout update is published");
    assert_eq!(phase(&status, 0), Phase::Unconfirmed);

    // A telemetry timeout must not invent a retry.  The existing manager
    // retry policy may explicitly reopen the same observation with a larger
    // retry number; only that transition is allowed to restore Preparing.
    observation.preparing("input", 0);
    assert_eq!(phase(&status, 0), Phase::Unconfirmed);
    observation.preparing("input", 1);
    assert_eq!(phase(&status, 0), Phase::Preparing);

    status.feedback(
        FLOW,
        11,
        "ActionDiscardTile",
        &json!({}),
        &[MjaiEvent::Dahai {
            actor: 0,
            pai: "6s".into(),
            tsumogiri: false,
        }],
    );
    assert_eq!(phase(&status, 0), Phase::Succeeded);
}

#[test]
fn old_skip_response_cannot_cross_round_after_invalidation() {
    let status = status_with_window();
    let observation = status.begin(&MjaiEvent::None, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(
        FLOW,
        111,
        &json!({"cancel_operation": true, "type": 0, "timeuse": 3}),
    );

    // The click is now unresolved, then the live game advances to another
    // decision window.  A delayed response for the old request must not
    // turn that old pass into a success in the new round.
    status.invalidate();
    let operation = json!({"seat": 0, "time_fixed": 5000, "time_add": 0});
    status.window(FLOW, 11, Some("E1:1".into()), Some(&operation), 0);
    status.response(FLOW, 111, &json!({}));
    assert_eq!(phase(&status, 0), Phase::Unconfirmed);
}

#[test]
fn window_loss_and_disconnect_distinguish_pre_click_and_post_click() {
    let status = status_with_window();
    let before_click = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "2m".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    before_click.preparing("input", 0);
    let next = json!({"seat": 0, "time_fixed": 5000, "time_add": 0});
    status.window(FLOW, 11, Some("E1:0".into()), Some(&next), 0);
    assert_eq!(phase(&status, 0), Phase::Cancelled);

    let after_click = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "3m".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    after_click.pressed();
    status.disconnect(FLOW);
    assert_eq!(phase(&status, 1), Phase::Unconfirmed);
}

#[test]
fn request_id_reuse_never_confirms_either_ambiguous_record() {
    let status = status_with_window();
    let first = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "2p".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    first.pressed();
    first.released();
    first.finish();
    status.request(FLOW, 120, &json!({"type": 1, "tile": "2p", "moqie": false}));

    let second = status.begin(
        &MjaiEvent::Dahai {
            actor: 0,
            pai: "3p".into(),
            tsumogiri: false,
        },
        status.current_window(),
        0,
    );
    second.pressed();
    second.released();
    second.finish();
    status.request(FLOW, 120, &json!({"type": 1, "tile": "3p", "moqie": false}));
    status.response(FLOW, 120, &json!({}));

    assert_eq!(phase(&status, 0), Phase::Unconfirmed);
    assert_ne!(phase(&status, 1), Phase::Succeeded);
}

#[test]
fn request_id_reuse_marks_the_new_record_unconfirmed_without_conflict_evidence() {
    let status = status_with_window();
    let first = status.begin(&MjaiEvent::None, status.current_window(), 0);
    first.pressed();
    first.released();
    first.finish();
    let request = json!({"cancel_operation": true, "type": 0, "timeuse": 3});
    status.request(FLOW, 121, &request);

    let second = status.begin(&MjaiEvent::None, status.current_window(), 0);
    second.pressed();
    second.released();
    second.finish();
    status.request(FLOW, 121, &request);

    assert_eq!(phase(&status, 0), Phase::Unconfirmed);
    assert_eq!(phase(&status, 1), Phase::Unconfirmed);
    status.response(FLOW, 121, &json!({}));
    assert_ne!(phase(&status, 0), Phase::Succeeded);
    assert_ne!(phase(&status, 1), Phase::Succeeded);
}

#[test]
fn records_survive_rounds_and_same_game_reconnect_but_new_game_clears() {
    let status = status_with_window();
    let observation = status.begin(&MjaiEvent::None, status.current_window(), 0);
    observation.pressed();
    observation.released();
    observation.finish();
    status.request(FLOW, 130, &json!({"cancel_operation": true}));
    status.response(FLOW, 130, &json!({}));
    assert_eq!(records(&status).len(), 1);

    status.bind_game(FLOW + 1, GAME);
    assert_eq!(
        records(&status).len(),
        1,
        "same-game reconnect retains history"
    );
    status.bind_game(FLOW + 2, GAME + 1);
    assert!(
        records(&status).is_empty(),
        "a new game starts a fresh history"
    );
}
