//! Durable evidence is independent of UI polling and contains no raw data.
use akagi::{
    autoplay::status::{AutoplayStatus, Observation},
    logger::autoplay::AutoplayAuditLogger,
    schema::MjaiEvent,
};
use serde_json::{json, Value};
use std::sync::Arc;

struct Audit {
    dir: tempfile::TempDir,
    status: Arc<AutoplayStatus>,
}
impl Audit {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let status = Arc::new(AutoplayStatus::default());
        status.set_audit_logger(Arc::new(AutoplayAuditLogger::new(dir.path()).unwrap()));
        status.bind_game(1, 9001);
        status.set_enabled(true);
        status.window(
            1,
            10,
            Some("E1:0".into()),
            Some(&json!({"seat":0,"time_fixed":10000})),
            0,
        );
        Self { dir, status }
    }
    fn begin(&self, tsumo: bool) -> Observation {
        self.status.begin(
            &MjaiEvent::Hora {
                actor: 0,
                target: if tsumo { 0 } else { 1 },
                deltas: None,
                ura_markers: None,
            },
            self.status.current_window(),
            0,
        )
    }
    fn rows(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.path().join("majsoul/autoplay.log"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn feedback(&self, step: u64, target: u8) {
        self.status.feedback(
            1,
            step,
            "ActionHule",
            &json!({}),
            &[MjaiEvent::Hora {
                actor: 0,
                target,
                deltas: None,
                ura_markers: None,
            }],
        );
    }
    fn verified(&self) -> usize {
        self.rows()
            .iter()
            .filter(|row| row["event"] == "operation_verified")
            .count()
    }
}

#[test]
fn ron_and_tsumo_have_durable_click_request_and_server_evidence_without_polling() {
    for tsumo in [false, true] {
        let audit = Audit::new();
        let observation = audit.begin(tsumo);
        let attempt = observation.pressed();
        observation.press_accepted(attempt);
        observation.released_for(attempt);
        observation.finish();
        audit
            .status
            .request(1, 42, &json!({"type":if tsumo {8} else {9},"timeuse":1}));
        audit.status.response(1, 42, &json!({}));
        assert_eq!(audit.verified(), 0, "RPC success is not a win confirmation");
        audit.feedback(11, if tsumo { 0 } else { 1 });
        let rows = audit.rows();
        for event in [
            "operation_started",
            "press_attempt",
            "press_accepted",
            "release_accepted",
            "client_request_matched",
            "client_response_accepted",
            "server_feedback_matched",
            "operation_verified",
        ] {
            assert_eq!(
                rows.iter().filter(|row| row["event"] == event).count(),
                1,
                "{event}"
            );
        }
        assert_eq!(audit.verified(), 1);
        let evidence: Vec<_> = rows
            .iter()
            .filter(|row| row["event"] != "operation_started" && row["event"] != "state_changed")
            .collect();
        assert!(evidence.iter().all(|row| row["operation_id"] == 1
            && row["attempt"] == attempt
            && row["flow_id"] == 1
            && row["window_step"] == 10
            && row["action"] == if tsumo { "tsumo" } else { "ron" }));
        let _ = audit.status.poll();
        let _ = audit.status.poll();
        assert_eq!(
            audit.rows(),
            rows,
            "HUD polling must neither erase nor duplicate evidence"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(audit.dir.path().join("majsoul/autoplay.log"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn fast_client_and_server_events_before_cdp_ack_wait_for_both_acknowledgements() {
    let audit = Audit::new();
    let observation = audit.begin(false);
    let attempt = observation.pressed();
    audit.status.request(1, 42, &json!({"type":9}));
    audit.feedback(11, 1);
    assert_eq!(audit.verified(), 0);
    observation.press_accepted(attempt);
    assert_eq!(audit.verified(), 0);
    observation.released_for(attempt);
    assert_eq!(audit.verified(), 1);
    observation.finish();
    observation.press_accepted(attempt);
    assert_eq!(audit.verified(), 1, "one click is verified only once");
}

#[test]
fn attempted_failed_manual_and_conflicting_inputs_never_become_verified_wins() {
    for scenario in [
        "attempt_only",
        "no_release",
        "manual",
        "automatic",
        "reused_id",
        "new_game",
        "timeout",
        "wrong_input",
        "server_error",
        "wrong_target",
        "stale_step",
        "wrong_flow",
    ] {
        let audit = Audit::new();
        let observation = audit.begin(false);
        let attempt = if scenario == "manual" {
            0
        } else {
            observation.pressed()
        };
        if scenario != "attempt_only" {
            observation.press_accepted(attempt);
        }
        if scenario != "no_release" {
            observation.released_for(attempt);
        }
        if scenario == "attempt_only" {
            observation.failed();
        }
        let payload = match scenario {
            "automatic" => json!({"type":9,"auto_operation":true}),
            "timeout" => json!({"type":9,"timeuse":1000000}),
            "wrong_input" => json!({"type":1,"tile":"1m"}),
            _ => json!({"type":9}),
        };
        audit.status.request(1, 42, &payload);
        if scenario == "reused_id" {
            audit.status.request(1, 42, &payload);
        }
        if scenario == "new_game" {
            audit.status.bind_game(1, 9002);
        }
        if scenario == "server_error" {
            audit.status.response(1, 42, &json!({"error":{"code":77}}));
        }
        if scenario == "wrong_flow" {
            audit.status.feedback(
                2,
                11,
                "ActionHule",
                &json!({}),
                &[MjaiEvent::Hora {
                    actor: 0,
                    target: 1,
                    deltas: None,
                    ura_markers: None,
                }],
            );
        } else {
            audit.feedback(
                if scenario == "stale_step" { 12 } else { 11 },
                if scenario == "wrong_target" { 2 } else { 1 },
            );
        }
        assert_eq!(audit.verified(), 0, "{scenario}");
    }
}

#[test]
fn retry_acknowledgements_cannot_be_mixed_across_click_attempts() {
    let audit = Audit::new();
    let observation = audit.begin(false);
    let first = observation.pressed();
    observation.press_accepted(first);
    observation.failed();
    observation.preparing("ron", 1);
    let second = observation.pressed();
    observation.released_for(second);
    audit.status.request(1, 42, &json!({"type":9}));
    audit.feedback(11, 1);
    assert_eq!(
        audit.verified(),
        0,
        "old press ack cannot verify a later release/request"
    );
    observation.press_accepted(second);
    assert_eq!(audit.verified(), 1);
    assert_eq!(
        audit
            .rows()
            .iter()
            .find(|row| row["event"] == "operation_verified")
            .unwrap()["attempt"],
        second
    );
}

#[test]
fn audit_never_records_arbitrary_action_step_round_or_protocol_fields() {
    const SECRET: &str = "FAKE-token-password-cookie-DO-NOT-RECORD";
    let audit = Audit::new();
    audit.status.window(
        1,
        10,
        Some(SECRET.into()),
        Some(&json!({"seat":0,"time_fixed":10000})),
        0,
    );
    let observation = audit.status.begin(
        &MjaiEvent::Reach {
            actor: 99,
            pai: Some(SECRET.into()),
        },
        audit.status.current_window(),
        0,
    );
    observation.preparing(SECRET, 1);
    let attempt = observation.pressed();
    observation.press_accepted(attempt);
    observation.released_for(attempt);
    audit.status.request(1, 42, &json!({"type":7,"index":0,"password":SECRET,"access_token":SECRET,"Cookie":SECRET,"Authorization":SECRET}));
    audit.status.response(
        1,
        42,
        &json!({"error":{"code":77,"message":SECRET},"account_id":123456,"nickname":SECRET}),
    );
    let bytes = std::fs::read(audit.dir.path().join("majsoul/autoplay.log")).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
    let allowed = [
        "schema_version",
        "ts_ms",
        "operation_id",
        "attempt",
        "retry",
        "action",
        "actor",
        "target",
        "flow_id",
        "window_step",
        "event",
        "phase",
        "reason",
        "msg_id",
        "operation_type",
        "error_code",
    ];
    for row in audit.rows() {
        assert!(row
            .as_object()
            .unwrap()
            .keys()
            .all(|key| allowed.contains(&key.as_str())));
        assert_ne!(row, json!({}));
        assert!(
            row["actor"].is_null(),
            "out-of-range seats must not be serialized"
        );
    }
}
