//! Closed-schema evidence. Never accepts protocol JSON, credentials or text.
use crate::autoplay::status::Phase;
use anyhow::Result;
use serde::Serialize;
use std::{fs::File, io::Write, path::Path, sync::Mutex};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEvent {
    OperationStarted,
    StateChanged,
    PressAttempt,
    PressAccepted,
    PressRejected,
    ReleaseAccepted,
    ReleaseRejected,
    ClientRequestMatched,
    ClientRequestRejected,
    ClientResponseAccepted,
    ClientResponseRejected,
    ServerFeedbackMatched,
    ServerFeedbackConflict,
    OperationVerified,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Discard,
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
    Riichi,
    Ron,
    Tsumo,
    Kita,
    AbortiveDraw,
    Pass,
    Unknown,
}
impl AuditAction {
    pub(crate) fn from_action(action: &serde_json::Value) -> Self {
        match action["type"].as_str() {
            Some("dahai") => Self::Discard,
            Some("chi") => Self::Chi,
            Some("pon") => Self::Pon,
            Some("daiminkan") => Self::Daiminkan,
            Some("ankan") => Self::Ankan,
            Some("kakan") => Self::Kakan,
            Some("reach") => Self::Riichi,
            Some("hora") if action["actor"] == action["target"] => Self::Tsumo,
            Some("hora") => Self::Ron,
            Some("kita") => Self::Kita,
            Some("ryukyoku") => Self::AbortiveDraw,
            Some("none") => Self::Pass,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditReason {
    AutoplayDisabled,
    WindowExpired,
    Disconnected,
    PageInvalid,
    ManualTakeover,
    RequestIdReused,
    InputConflict,
    FeedbackConflict,
    ServerError,
    ServerConfirmed,
    FeedbackTimeout,
    CdpFailed,
    OperationAborted,
    Unknown,
}
impl AuditReason {
    pub(crate) fn from_reason(reason: &str) -> Self {
        match reason {
            "autoplay_disabled" => Self::AutoplayDisabled,
            "window_expired" => Self::WindowExpired,
            "disconnected" => Self::Disconnected,
            "page_invalid" => Self::PageInvalid,
            "manual_takeover" => Self::ManualTakeover,
            "request_id_reused" => Self::RequestIdReused,
            "input_conflict" => Self::InputConflict,
            "feedback_conflict" => Self::FeedbackConflict,
            "server_confirmed" => Self::ServerConfirmed,
            "feedback_timeout" => Self::FeedbackTimeout,
            "cdp_failed" => Self::CdpFailed,
            "operation_aborted" => Self::OperationAborted,
            _ if reason
                .strip_prefix("server_error:")
                .is_some_and(|code| code.parse::<i64>().is_ok()) =>
            {
                Self::ServerError
            }
            _ => Self::Unknown,
        }
    }
}

/// Local operation/window numbers and local seat positions only. No game UUID,
/// player identity, tiles, scores, coordinates, payloads, URLs or free text.
#[derive(Debug, Serialize)]
pub struct AutoplayAuditRecord {
    pub schema_version: u8,
    pub ts_ms: i64,
    pub operation_id: u64,
    pub attempt: u32,
    pub retry: u32,
    pub action: AuditAction,
    pub actor: Option<u8>,
    pub target: Option<u8>,
    pub flow_id: Option<u64>,
    pub window_step: Option<u64>,
    pub event: AuditEvent,
    pub phase: Phase,
    pub reason: Option<AuditReason>,
    pub msg_id: Option<u16>,
    pub operation_type: Option<u8>,
    pub error_code: Option<i64>,
}

pub struct AutoplayAuditLogger {
    file: Mutex<File>,
}
impl AutoplayAuditLogger {
    pub fn new(session_dir: &Path) -> Result<Self> {
        let dir = session_dir.join("majsoul");
        crate::util::private_fs::directory(&dir)?;
        Ok(Self {
            file: Mutex::new(crate::util::private_fs::append(&dir.join("autoplay.log"))?),
        })
    }
    pub fn record(&self, record: &AutoplayAuditRecord) {
        // One append, including the newline, so complete events cannot be
        // coalesced away by the HUD's 100ms polling or interleaved on disk.
        let mut bytes = serde_json::to_vec(record).expect("closed audit schema serializes");
        bytes.push(b'\n');
        if self.file.lock().unwrap().write_all(&bytes).is_err() {
            tracing::warn!("automatic action evidence could not be written");
        }
    }
}
