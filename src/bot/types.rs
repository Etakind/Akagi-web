//! Public types exchanged between `BotRunner` and the rest of Akagi.
//!
//! Wire shape matches the mjai.app convention: an mjai action object with an
//! optional `meta` sibling for HUD-grade recommendation data.
//!
//! `meta` is intentionally untyped — it is a free-form JSON object emitted by
//! the bot for the frontend to render. Akagi forwards it verbatim and never
//! interprets the contents. Bots that have nothing to add omit it.
//!
//! Example wire JSON:
//! ```text
//! {"type":"dahai","actor":0,"pai":"9m","tsumogiri":false,
//!  "meta":{"confidence":0.87,"reasoning":"isolated 9m, low risk"}}
//! ```

use crate::schema::MjaiEvent;
use serde::{Deserialize, Serialize};

/// One reaction from a `BotRunner`.
///
/// `action` is uniformly an `MjaiEvent` — `MjaiEvent::None` means "no
/// action this turn" (see `src/schema/mjai/mod.rs`). Consumers can match
/// on the variant to decide whether to render or skip.
///
/// `meta` is whatever JSON object the bot chose to emit alongside its
/// action. Backend treats it as opaque; the frontend renders it however
/// it wants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BotResponse {
    #[serde(skip)]
    pub decision_context: crate::event_bus::EventContext,
    /// Local inference start time. Never serialized or restored from history.
    #[serde(skip)]
    pub decision_started: Option<std::time::Instant>,
    #[serde(flatten)]
    pub action: MjaiEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn none_action_round_trips() {
        let line = r#"{"type":"none"}"#;
        let resp: BotResponse = serde_json::from_str(line).unwrap();
        assert!(matches!(resp.action, MjaiEvent::None));
        assert!(resp.meta.is_none());
        assert_eq!(serde_json::to_string(&resp).unwrap(), line);
    }

    #[test]
    fn dahai_with_meta_round_trips() {
        let line = r#"{"type":"dahai","actor":0,"pai":"9m","tsumogiri":false,"meta":{"q_values":[0.12,0.05,0.85],"confidence":0.87}}"#;
        let resp: BotResponse = serde_json::from_str(line).unwrap();
        match &resp.action {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "9m");
                assert!(!tsumogiri);
            }
            other => panic!("expected dahai, got {other:?}"),
        }
        let meta = resp.meta.as_ref().unwrap();
        assert_eq!(meta["confidence"], json!(0.87));
        assert_eq!(meta["q_values"], json!([0.12, 0.05, 0.85]));

        // Round-trip equivalence (key order is structural, not lexical).
        let v1: serde_json::Value = serde_json::from_str(line).unwrap();
        let v2: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn dahai_without_meta_skips_field() {
        let resp = BotResponse {
            decision_context: Default::default(),
            decision_started: None,
            action: MjaiEvent::Dahai {
                actor: 1,
                pai: "5mr".into(),
                tsumogiri: true,
            },
            meta: None,
        };
        let out = serde_json::to_string(&resp).unwrap();
        assert!(
            !out.contains("meta"),
            "meta:None should not be emitted: {out}"
        );
    }

    #[test]
    fn unknown_meta_keys_round_trip_unchanged() {
        // Backend must not interpret meta — exotic keys, nested objects,
        // and arrays all survive a round-trip byte-for-byte (modulo key
        // order, which we compare via Value equality).
        let line = r#"{"type":"dahai","actor":2,"pai":"3s","tsumogiri":false,"meta":{"reasoning":"isolated","scores":{"attack":0.4,"defence":0.6},"flags":["safe","far"]}}"#;
        let resp: BotResponse = serde_json::from_str(line).unwrap();
        let v1: serde_json::Value = serde_json::from_str(line).unwrap();
        let v2: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(v1, v2);
    }
}
