//! Observational Majsoul telemetry. Input counters remain retry hints, never proof.
//! All writers (including the synchronous protocol bridge) share this store.
use crate::schema::MjaiEvent;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Scheduled,
    Executing,
    AwaitingFeedback,
    Succeeded,
    Failed,
    Cancelled,
    Unconfirmed,
}

impl Phase {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Scheduled | Self::Executing | Self::AwaitingFeedback
        )
    }
    fn accepts_feedback(self) -> bool {
        self.active() || self == Self::Unconfirmed
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct OperationRecord {
    pub id: u64,
    pub round: String,
    pub created_at: i64,
    pub action: Value,
    pub step: String,
    pub retry: u32,
    pub phase: Phase,
    pub remaining_ms: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusUpdate {
    pub version: u64,
    pub reset: bool,
    pub enabled: bool,
    pub records: Vec<OperationRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    flow: u64,
    step: u64,
    opened: Instant,
    expires: Option<Instant>,
}
impl Window {
    pub fn unexpired(&self) -> bool {
        self.expires
            .is_none_or(|deadline| Instant::now() < deadline)
    }
}

struct Entry {
    record: OperationRecord,
    window: Option<Window>,
    deadline: Option<Instant>,
    pressed: Option<Instant>,
    released: Option<Instant>,
    finished: bool,
    request: bool,
    request_press: Option<Instant>,
}

impl Entry {
    fn conclude(&mut self, phase: Phase, reason: &str) {
        self.record.phase = phase;
        self.record.reason = Some(reason.into());
        self.deadline = None;
        self.record.remaining_ms = None;
    }
    fn view(&self, now: Instant) -> OperationRecord {
        let mut record = self.record.clone();
        record.remaining_ms = self.deadline.and_then(|deadline| {
            // Expired waits become preparation, not an unkept promise of 0.0s.
            (deadline > now).then(|| deadline.duration_since(now).as_millis().max(1) as u64)
        });
        if record.phase == Phase::Scheduled && record.remaining_ms.is_none() {
            record.phase = Phase::Preparing;
        }
        record
    }
}

#[derive(Default)]
struct State {
    version: u64,
    next_id: u64,
    reset: bool,
    enabled: bool,
    game: Option<u64>,
    flow: Option<u64>,
    round: String,
    window: Option<Window>,
    entries: Vec<Entry>,
    dirty: HashSet<u64>,
    requests: HashMap<(u64, u16), u64>,
    // Reused outstanding IDs cannot safely identify either response.
    ambiguous_requests: HashSet<(u64, u16)>,
}

#[derive(Default)]
pub struct AutoplayStatus {
    state: Mutex<State>,
}

impl AutoplayStatus {
    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled {
            state.enabled = enabled;
            state.version += 1;
            state.reset = true; // publish a full snapshot for switch-only changes
            if !enabled {
                for entry in &mut state.entries {
                    if entry.record.phase.active() && entry.pressed.is_none() {
                        entry.conclude(Phase::Cancelled, "autoplay_disabled");
                    }
                }
            }
        }
    }

    pub fn bind_game(&self, flow: u64, game: u64) {
        let mut state = self.state.lock().unwrap();
        if state.game != Some(game) {
            state.entries.clear();
            state.dirty.clear();
            state.requests.clear();
            state.ambiguous_requests.clear();
            state.round.clear();
            state.game = Some(game);
            state.reset = true;
        } else if state.flow != Some(flow) {
            Self::interrupt(&mut state, "disconnected");
        }
        state.flow = Some(flow);
        state.window = None;
        state.version += 1;
    }

    fn interrupt(state: &mut State, reason: &str) {
        for entry in &mut state.entries {
            if entry.record.phase.active() {
                entry.conclude(
                    if entry.pressed.is_some() {
                        Phase::Unconfirmed
                    } else {
                        Phase::Cancelled
                    },
                    reason,
                );
                state.dirty.insert(entry.record.id);
            }
        }
        state.window = None;
        state.version += 1;
    }

    pub fn invalidate(&self) {
        Self::interrupt(&mut self.state.lock().unwrap(), "page_invalid");
    }

    pub fn disconnect(&self, flow: u64) {
        let mut state = self.state.lock().unwrap();
        if state.flow == Some(flow) {
            Self::interrupt(&mut state, "disconnected");
            state.flow = None;
        }
    }

    /// Called after live feedback, before the manager can plan a new decision.
    /// A window exists even in unlimited-time rooms (no TimeBudget).
    pub fn window(
        &self,
        flow: u64,
        step: u64,
        round: Option<String>,
        operation: Option<&Value>,
        seat: u8,
    ) {
        let mut state = self.state.lock().unwrap();
        if state.flow != Some(flow) {
            return;
        }
        if let Some(round) = round {
            state.round = round;
        }
        let operation = operation.filter(|op| op["seat"].as_u64() == Some(u64::from(seat)));
        let now = Instant::now();
        state.window = operation.map(|op| {
            let fixed = op["time_fixed"].as_u64().unwrap_or(0);
            let add = op["time_add"].as_u64().unwrap_or(0);
            Window {
                flow,
                step,
                opened: now,
                expires: (fixed > 0)
                    .then(|| now + Duration::from_millis(fixed.saturating_add(add))),
            }
        });
        let current = state.window;
        let mut cancelled = Vec::new();
        for entry in &mut state.entries {
            if entry.record.phase.active() && entry.pressed.is_none() && entry.window != current {
                entry.conclude(Phase::Cancelled, "window_expired");
                cancelled.push(entry.record.id);
            }
        }
        state.dirty.extend(cancelled);
        state.version += 1;
    }

    pub fn current_window(&self) -> Option<Window> {
        self.state.lock().unwrap().window
    }

    pub fn window_valid(&self, window: Option<Window>, started: Option<Instant>) -> bool {
        let state = self.state.lock().unwrap();
        window.is_some_and(|window| {
            state.window == Some(window)
                && started.is_some_and(|started| started >= window.opened)
                && window.expires.is_none_or(|end| Instant::now() < end)
        })
    }

    pub fn restore_elapsed(&self, flow: u64, elapsed: Duration) {
        let mut state = self.state.lock().unwrap();
        if state.flow == Some(flow) {
            if let Some(window) = &mut state.window {
                window.expires = window
                    .expires
                    .map(|end| end.checked_sub(elapsed).unwrap_or(window.opened));
            }
        }
    }

    pub fn begin(
        self: &Arc<Self>,
        action: &MjaiEvent,
        window: Option<Window>,
        seat: u8,
    ) -> Observation {
        let mut state = self.state.lock().unwrap();
        state.next_id += 1;
        let id = state.next_id;
        let mut action = serde_json::to_value(action).unwrap();
        if action["type"] == "ryukyoku" {
            action["actor"] = seat.into();
        }
        let record = OperationRecord {
            id,
            round: state.round.clone(),
            created_at: chrono::Utc::now().timestamp_millis(),
            action,
            step: "input".into(),
            retry: 0,
            phase: Phase::Preparing,
            remaining_ms: None,
            reason: None,
        };
        state.entries.push(Entry {
            record,
            window,
            deadline: None,
            pressed: None,
            released: None,
            finished: false,
            request: false,
            request_press: None,
        });
        state.dirty.insert(id);
        state.version += 1;
        Observation {
            status: self.clone(),
            id,
        }
    }

    fn edit(&self, id: u64, edit: impl FnOnce(&mut Entry)) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.record.id == id) {
            edit(entry);
            state.dirty.insert(id);
            state.version += 1;
        }
    }

    /// Bind an actually executed press to a fresh client request. Neither this
    /// observation nor a non-error response proves a discard/meld succeeded.
    pub fn request(&self, flow: u64, id: u16, payload: &Value) {
        let mut state = self.state.lock().unwrap();
        let key = (flow, id);
        if let Some(old) = state.requests.remove(&key) {
            state.ambiguous_requests.insert(key);
            if let Some(entry) = state
                .entries
                .iter_mut()
                .find(|entry| entry.record.id == old)
            {
                entry.request = false;
                if entry.record.phase.accepts_feedback() {
                    entry.conclude(Phase::Unconfirmed, "request_id_reused");
                }
                state.dirty.insert(old);
            }
            state.version += 1;
        }
        if state.flow != Some(flow) {
            return;
        }
        let current = state.window;
        if current.is_none() {
            return;
        }
        let now = Instant::now();
        let mut bound = None;
        let ambiguous = state.ambiguous_requests.contains(&key);
        for entry in state.entries.iter_mut().rev() {
            if !entry.record.phase.active() || entry.window != current {
                continue;
            }
            let Some(pressed) = entry.pressed else {
                entry.conclude(Phase::Cancelled, "manual_takeover");
                bound = Some((entry.record.id, false));
                break;
            };
            if now.duration_since(pressed) > Duration::from_secs(2)
                || current.is_some_and(|w| w.expires.is_some_and(|end| now >= end))
            {
                continue;
            }
            let client = !payload["auto_operation"].as_bool().unwrap_or(false)
                && payload["timeuse"]
                    .as_u64()
                    .is_none_or(|time| time < 1_000_000);
            if !client || entry.request_press == Some(pressed) {
                entry.request = false;
                entry.conclude(Phase::Unconfirmed, "manual_takeover");
                bound = Some((entry.record.id, false));
                break;
            }
            if ambiguous {
                entry.request = false;
                entry.conclude(Phase::Unconfirmed, "request_id_reused");
                bound = Some((entry.record.id, false));
            } else if request_matches(&entry.record.action, payload) {
                entry.request = true;
                entry.request_press = Some(pressed);
                bound = Some((entry.record.id, true));
            } else {
                entry.conclude(Phase::Failed, "input_conflict");
                bound = Some((entry.record.id, false));
            }
            break;
        }
        if let Some((record, accepted)) = bound {
            if accepted {
                state.requests.insert(key, record);
            }
            state.dirty.insert(record);
            state.version += 1;
        }
    }

    pub fn response(&self, flow: u64, id: u16, payload: &Value) {
        let mut state = self.state.lock().unwrap();
        if state.ambiguous_requests.remove(&(flow, id)) {
            return;
        }
        let Some(record) = state.requests.remove(&(flow, id)) else {
            return;
        };
        if state.flow != Some(flow) {
            return;
        }
        let round = state.round.clone();
        if let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.record.id == record)
        {
            if !entry.record.phase.accepts_feedback()
                || !entry.request
                || entry.record.round != round
            {
                return;
            }
            let code = payload
                .get("error")
                .and_then(|error| error["code"].as_i64())
                .unwrap_or(0);
            if code != 0 {
                entry.conclude(Phase::Failed, &format!("server_error:{code}"));
            } else if entry.record.action["type"] == "none" {
                entry.conclude(Phase::Succeeded, "server_confirmed");
            } else {
                return;
            }
            state.dirty.insert(record);
            state.version += 1;
        }
    }

    /// Only live ActionPrototype echoes, never restore/replay events. Step
    /// adjacency isolates an echo from later identical decisions in the round.
    pub fn feedback(&self, flow: u64, step: u64, name: &str, data: &Value, events: &[MjaiEvent]) {
        let mut state = self.state.lock().unwrap();
        if state.flow != Some(flow) {
            return;
        }
        let round = state.round.clone();
        let mut changed = Vec::new();
        for entry in &mut state.entries {
            if !entry.record.phase.accepts_feedback()
                || !entry.request
                || entry.pressed.is_none()
                || entry.record.round != round
            {
                continue;
            }
            let Some(window) = entry.window else {
                continue;
            };
            if window.flow != flow || window.step.checked_add(1) != Some(step) {
                continue;
            }
            let expected = &entry.record.action;
            let kind = expected["type"].as_str().unwrap_or("");
            if kind == "none" {
                continue;
            }
            let matches = if kind == "reach" {
                data["seat"] == expected["actor"]
                    && name == "ActionDiscardTile"
                    && (data["is_liqi"].as_bool() == Some(true)
                        || data["is_wliqi"].as_bool() == Some(true))
                    && crate::bridge::majsoul::tile::ms_to_mjai(data["tile"].as_str().unwrap_or(""))
                        .ok()
                        .map(|tile| Value::String(tile.into()))
                        == expected.get("pai").cloned()
            } else if kind == "ryukyoku" {
                // Kyuushu kyuuhai (type 1), not other abortive/exhaustive draws.
                name == "ActionLiuJu"
                    && data["type"].as_u64() == Some(1)
                    && data["seat"] == expected["actor"]
            } else {
                events
                    .iter()
                    .any(|event| event_matches(expected, &serde_json::to_value(event).unwrap()))
            };
            if matches {
                entry.conclude(Phase::Succeeded, "server_confirmed");
                changed.push(entry.record.id);
            } else if events.iter().any(|event| {
                let actual = serde_json::to_value(event).unwrap();
                actual["actor"] == expected["actor"]
                    && matches!(
                        actual["type"].as_str(),
                        Some(
                            "dahai"
                                | "chi"
                                | "pon"
                                | "daiminkan"
                                | "ankan"
                                | "kakan"
                                | "hora"
                                | "kita"
                        )
                    )
            }) {
                entry.conclude(Phase::Failed, "feedback_conflict");
                changed.push(entry.record.id);
            }
        }
        if !changed.is_empty() {
            state.dirty.extend(changed);
            state.version += 1;
        }
    }

    fn tick(state: &mut State, now: Instant) {
        for entry in &mut state.entries {
            if entry.record.phase == Phase::AwaitingFeedback
                && entry
                    .released
                    .is_some_and(|released| now.duration_since(released) >= Duration::from_secs(5))
            {
                entry.conclude(Phase::Unconfirmed, "feedback_timeout");
                state.dirty.insert(entry.record.id);
                state.version += 1;
            }
        }
    }

    pub fn snapshot(&self) -> StatusUpdate {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        Self::tick(&mut state, now);
        StatusUpdate {
            version: state.version,
            reset: true,
            enabled: state.enabled,
            records: state.entries.iter().map(|entry| entry.view(now)).collect(),
        }
    }

    pub fn poll(&self) -> Option<StatusUpdate> {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        Self::tick(&mut state, now);
        let records: Vec<_> = state
            .entries
            .iter()
            .filter(|entry| {
                state.reset || state.dirty.contains(&entry.record.id) || entry.deadline.is_some()
            })
            .map(|entry| entry.view(now))
            .collect();
        if records.is_empty() && !state.reset {
            return None;
        }
        state.version += 1;
        let update = StatusUpdate {
            version: state.version,
            reset: state.reset,
            enabled: state.enabled,
            records,
        };
        state.reset = false;
        state.dirty.clear();
        Some(update)
    }
}

/// RAII closes early-return paths; it never changes input timing or retry policy.
pub struct Observation {
    status: Arc<AutoplayStatus>,
    id: u64,
}

impl Observation {
    /// Recheck after hover: a resolved decision must not receive a stale press.
    pub fn can_press(&self) -> bool {
        let mut state = self.status.state.lock().unwrap();
        let current = state.window;
        let enabled = state.enabled;
        let now = Instant::now();
        let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.record.id == self.id)
        else {
            return false;
        };
        if !entry.record.phase.active() {
            return false;
        }
        let valid = enabled
            && entry.window.is_some()
            && entry.window == current
            && current.is_none_or(|window| window.expires.is_none_or(|end| now < end));
        if !valid {
            entry.conclude(
                if entry.pressed.is_some() {
                    Phase::Unconfirmed
                } else {
                    Phase::Cancelled
                },
                "window_expired",
            );
            state.dirty.insert(self.id);
            state.version += 1;
        }
        valid
    }
    pub fn preparing(&self, step: &str, retry: u32) {
        self.status.edit(self.id, |entry| {
            // Existing retry policy may decide to retry after a slow verify
            // wait. Telemetry never adds a retry, nor suppresses that policy.
            let timed_out = retry > entry.record.retry
                && entry.record.phase == Phase::Unconfirmed
                && entry.record.reason.as_deref() == Some("feedback_timeout");
            let retry_after_cdp_error = retry > 0
                && entry.record.phase == Phase::Failed
                && entry.record.reason.as_deref() == Some("cdp_failed");
            if timed_out || retry_after_cdp_error {
                entry.record.phase = Phase::Preparing;
                entry.record.reason = None;
            }
            if entry.record.phase.active() {
                entry.record.step = step.into();
                entry.record.retry = retry;
                entry.record.phase = Phase::Preparing;
                entry.deadline = None;
            }
        });
    }
    pub fn scheduled(&self, duration: Duration) {
        self.status.edit(self.id, |entry| {
            if entry.record.phase.active() {
                entry.record.phase = Phase::Scheduled;
                entry.deadline = Some(Instant::now() + duration);
            }
        });
    }
    pub fn pressed(&self) {
        self.status.edit(self.id, |entry| {
            if entry.record.phase.active() {
                entry.record.phase = Phase::Executing;
                entry.deadline = None;
                entry.pressed = Some(Instant::now());
            }
        });
    }
    pub fn released(&self) {
        self.status.edit(self.id, |entry| {
            entry.released = Some(Instant::now());
            if entry.record.phase.active() {
                entry.record.phase = Phase::AwaitingFeedback;
            }
        });
    }
    pub fn failed(&self) {
        self.status.edit(self.id, |entry| {
            if entry.record.phase.active() {
                entry.conclude(Phase::Failed, "cdp_failed");
            }
        });
    }
    pub fn finish(&self) {
        self.status.edit(self.id, |entry| {
            entry.finished = true;
            if entry.record.phase.active() {
                if entry.pressed.is_none() {
                    entry.conclude(Phase::Cancelled, "page_invalid");
                } else {
                    entry.record.phase = Phase::AwaitingFeedback;
                    entry.deadline = None;
                }
            }
        });
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.status.edit(self.id, |entry| {
            if !entry.finished && entry.record.phase.active() {
                entry.conclude(
                    if entry.pressed.is_some() {
                        Phase::Unconfirmed
                    } else {
                        Phase::Cancelled
                    },
                    "operation_aborted",
                );
            }
        });
    }
}

fn request_matches(action: &Value, request: &Value) -> bool {
    let kind = action["type"].as_str().unwrap_or("");
    if kind == "none" {
        return request["cancel_operation"].as_bool() == Some(true)
            || request["type"].as_u64() == Some(0);
    }
    if request["cancel_operation"].as_bool() == Some(true) {
        return false;
    }
    let code = match kind {
        "dahai" => 1,
        "chi" => 2,
        "pon" => 3,
        "ankan" => 4,
        "daiminkan" => 5,
        "kakan" => 6,
        "reach" => 7,
        "hora" if action["actor"] == action["target"] => 8,
        "hora" => 9,
        "ryukyoku" => 10,
        "kita" => 11,
        _ => return false,
    };
    if request["type"].as_u64() != Some(code) {
        return false;
    }
    if matches!(kind, "dahai" | "reach") {
        let raw = request["tile"].as_str().unwrap_or("");
        // Riichi may name an operation candidate by index, not by tile.
        // Such a request only binds intent; the echo must still name the tile.
        if kind != "reach" || !raw.is_empty() {
            let tile = crate::bridge::majsoul::tile::ms_to_mjai(raw).ok();
            if tile.map(|tile| Value::String(tile.into())) != action.get("pai").cloned() {
                return false;
            }
        }
        if kind == "dahai"
            && request["moqie"].as_bool().unwrap_or(false)
                != action["tsumogiri"].as_bool().unwrap_or(false)
        {
            return false;
        }
    }
    true
}

fn event_matches(expected: &Value, actual: &Value) -> bool {
    if expected["type"] != actual["type"] {
        return false;
    }
    for key in ["actor", "target", "pai", "tsumogiri"] {
        if let Some(value) = expected.get(key) {
            if !value.is_null() && actual.get(key) != Some(value) {
                return false;
            }
        }
    }
    if let Some(consumed) = expected["consumed"].as_array() {
        let Some(actual) = actual["consumed"].as_array() else {
            return false;
        };
        let mut expected = consumed.clone();
        let mut actual = actual.clone();
        expected.sort_by_key(Value::to_string);
        actual.sort_by_key(Value::to_string);
        if expected != actual {
            return false;
        }
    }
    true
}
