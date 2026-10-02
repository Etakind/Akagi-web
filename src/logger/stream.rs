//! `tracing` layer that publishes every event in two places at once:
//!
//! - serialized as one JSON line into `<session>/all.jsonl` (the canonical
//!   on-disk machine-readable log)
//! - cloned onto a `tokio::sync::broadcast` channel that the IPC layer
//!   forwards to the frontend log viewer over a `tauri::ipc::Channel`
//!
//! The point of doing both inside one layer is that the frontend's
//! initial-load reader (`read_log_session`) and the live tail use a
//! single canonical struct (`schema::LogEntry`) — there is no second
//! formatter to keep in sync. The disk writer is synchronous (no
//! `tracing-appender::non_blocking`) since the `broadcast` channel
//! already gives us the async hop, and `serde_json` writes are small.
//!
//! Loss policy: the broadcast channel is bounded and lossy. Slow
//! consumers (frontend that can't keep up with `RUST_LOG=trace` traffic)
//! cause the receiver to see `RecvError::Lagged(n)`; the IPC forwarder
//! injects a synthetic `WARN akagi.logger "dropped N events…"` entry so
//! the UI never silently lies about completeness.

use crate::schema::LogEntry;
use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Handle to the layer's broadcast sender. Owned by `Session` so the IPC
/// layer can call `subscribe()` long after the `Layer` itself has been
/// moved into the `tracing` registry. The on-disk file handle stays
/// inside the layer (which the registry keeps alive for process life).
#[derive(Clone)]
pub struct LogStreamHandle {
    tx: broadcast::Sender<LogEntry>,
}

impl LogStreamHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }
}

/// `tracing_subscriber::Layer` that turns each event into a
/// `schema::LogEntry`, writes it to the JSONL file, and broadcasts it.
pub struct LogStreamLayer {
    tx: broadcast::Sender<LogEntry>,
    file: Arc<Mutex<File>>,
}

impl LogStreamLayer {
    /// Open `<dir>/<file_name>` for append and build the layer + handle.
    /// Channel `capacity` is the broadcast ring size; 1024 is generous
    /// for normal workloads and still bounds memory under TRACE storms.
    pub fn open(path: &Path, capacity: usize) -> Result<(Self, LogStreamHandle)> {
        let file = crate::util::private_fs::append(path)?;
        let file = Arc::new(Mutex::new(file));
        let (tx, _) = broadcast::channel(capacity);
        let handle = LogStreamHandle { tx: tx.clone() };
        let layer = Self { tx, file };
        Ok((layer, handle))
    }
}

/// Field collector. `message` is the conventional tracing slot for the
/// human-readable string produced by `info!("…")` macros — it is hoisted
/// out of `fields` and stored separately. Everything else lands in the
/// `fields` map as a `serde_json::Value`, type-preserved when we have a
/// typed visitor method, falling back to a debug-formatted string.
struct FieldVisitor {
    message: Option<String>,
    fields: HashMap<String, serde_json::Value>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            fields: HashMap::new(),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        // `Number::from_f64` returns `None` for NaN / inf — fall back to a
        // debug string in that case so we never panic.
        let v = serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(value.to_string()));
        self.fields.insert(field.name().to_string(), v);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // For the canonical `message` field, tracing's macro records a
        // `format_args!` whose Debug == Display — so this gives us the
        // bare formatted string, not a quoted one.
        let s = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(s);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(s));
        }
    }
}

pub(super) fn safe_fields(event: &Event<'_>) -> (String, HashMap<String, serde_json::Value>) {
    let mut visitor = FieldVisitor::new();
    event.record(&mut visitor);
    if crate::privacy::sensitive_target(event.metadata().target()) {
        // Do not try to regex-redact arbitrary library errors or protocol bytes.
        visitor
            .fields
            .retain(|_, v| v.is_number() || v.is_boolean());
        (
            "I/O diagnostic; free-form details omitted by privacy policy".into(),
            visitor.fields,
        )
    } else {
        (visitor.message.unwrap_or_default(), visitor.fields)
    }
}

impl<S> Layer<S> for LogStreamLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let (message, fields) = safe_fields(event);

        let entry = LogEntry {
            ts_ms: Local::now().timestamp_millis(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            file: meta.file().map(|s| s.to_string()),
            line: meta.line(),
            message,
            fields,
        };

        // Disk write: serialize one JSON object + newline. Lock contention
        // is acceptable — emit-rate is bounded by tracing's own filtering.
        if let Ok(mut f) = self.file.lock() {
            // Errors here are swallowed: failing to log shouldn't crash
            // the program. The file handle is always re-openable; the
            // most likely cause is "disk full", which the user will see
            // through other channels (history writes, etc.).
            if serde_json::to_writer(&mut *f, &entry).is_ok() {
                let _ = f.write_all(b"\n");
            }
        }

        // Wire write: only clone+send when there's a live receiver. With
        // no receivers `send` returns `Err` (the broadcast contract).
        // Don't bother allocating in that case.
        if self.tx.receiver_count() > 0 {
            let _ = self.tx.send(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tracing::Level;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    /// Drive a real `tracing::info!` through the layer and assert the
    /// JSONL line on disk decodes back into a `LogEntry`. Exercises the
    /// `Visit` impl, the file write path, and the wire serialization in
    /// one shot — if any of them disagrees, this fails.
    #[test]
    fn end_to_end_event_round_trips() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("all.jsonl");
        let (layer, handle) = LogStreamLayer::open(&path, 16).unwrap();
        let mut rx = handle.subscribe();

        let subscriber = Registry::default().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(seat = 2, kind = "ron", "hello world");
        });

        // Disk side: parse the one line we wrote.
        let body = std::fs::read_to_string(&path).unwrap();
        let line = body.lines().next().expect("no jsonl line written");
        let from_disk: LogEntry = serde_json::from_str(line).unwrap();
        assert_eq!(from_disk.level, Level::INFO.to_string());
        assert_eq!(from_disk.target, "akagi::logger::stream::tests");
        assert_eq!(from_disk.message, "hello world");
        assert_eq!(
            from_disk.fields.get("seat"),
            Some(&serde_json::Value::Number(2.into()))
        );
        assert_eq!(
            from_disk.fields.get("kind"),
            Some(&serde_json::Value::String("ron".into()))
        );

        // Wire side: should be byte-identical to the disk side.
        let from_wire = rx.try_recv().expect("no broadcast received");
        assert_eq!(from_disk, from_wire);
    }

    #[test]
    fn handle_subscribe_yields_independent_receivers() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("all.jsonl");
        let (_layer, handle) = LogStreamLayer::open(&path, 4).unwrap();
        let mut a = handle.subscribe();
        let mut b = handle.subscribe();
        let entry = LogEntry {
            ts_ms: 1,
            level: "INFO".into(),
            target: "akagi::test".into(),
            file: None,
            line: None,
            message: "x".into(),
            fields: HashMap::new(),
        };
        handle.tx.send(entry.clone()).unwrap();
        assert_eq!(a.try_recv().unwrap(), entry);
        assert_eq!(b.try_recv().unwrap(), entry);
    }
}
