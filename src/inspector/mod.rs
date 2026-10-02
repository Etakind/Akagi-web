//! Inspector data path — captures every WS frame, MJAI event, and bot
//! reaction into one canonical timeline.
//!
//! The Logs → Inspector tab is the only consumer; this module is the
//! plumbing that gets pipeline events out of the proxy / bridge / mjai
//! bus / bot manager and into one place.
//!
//! Design mirrors `crate::logger::stream` (and lives at the same lifetime
//! tier — owned by `Session`):
//!
//! - One file: `<session>/inspector.jsonl`. Each line is a serialized
//!   `schema::InspectorEntry`. Source of truth for past-session viewing.
//! - One broadcast: `tokio::sync::broadcast::Sender<InspectorEntry>` of
//!   capacity 1024. The IPC `subscribe_inspector` command grabs a
//!   receiver per frontend channel; slow consumers see `Lagged(n)` and
//!   the forwarder injects a synthetic record so the UI reflects the gap.
//! - Single `InspectorWriter` cloned via `Arc` to every emitter
//!   (proxy/handler.rs, capture/chromium/cdp.rs, mjai_bus subscriber,
//!   bot manager). Writers serialize to disk synchronously, then
//!   `try_send` the broadcast — file write is the durable path, the bus
//!   is best-effort.
//!
//! Why disk-write before broadcast: the wire side is lossy by design
//! (broadcast capacity), the file is not. If a slow frontend drops live
//! entries, the user can still load the full session from disk.

pub mod annotate;

use crate::schema::InspectorEntry;
use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Bus type for inspector live-tail. One `Sender` per `Session`; clones
/// are handed to each emitting subsystem.
pub type InspectorBus = broadcast::Sender<InspectorEntry>;

/// Append-only sink for inspector entries.
///
/// Cheap to clone — internally `Arc`-wrapped. The writer is shared
/// across the proxy handler, chromium capture, mjai-bus subscriber, and
/// bot manager; each calls `record(...)` independently.
#[derive(Clone)]
pub struct InspectorWriter {
    inner: Arc<Inner>,
}

struct Inner {
    file: Mutex<File>,
    tx: InspectorBus,
}

impl InspectorWriter {
    /// Open `<dir>/<file_name>` for append and build the writer + a
    /// broadcast `Sender`. The caller passes the same `Sender` into
    /// `Session` so the IPC layer can hand out receivers later.
    pub fn open(path: &Path, capacity: usize) -> Result<(Self, InspectorBus)> {
        let file = crate::util::private_fs::append(path)?;
        let (tx, _) = broadcast::channel(capacity);
        let writer = Self {
            inner: Arc::new(Inner {
                file: Mutex::new(file),
                tx: tx.clone(),
            }),
        };
        Ok((writer, tx))
    }

    /// Record one entry. Best-effort: failures in either the file write
    /// or the broadcast are swallowed so emit-site code stays simple
    /// (the inspector is observability, not an authoritative store).
    pub fn record(&self, entry: InspectorEntry) {
        let entry = crate::privacy::inspector(entry);
        if let Ok(mut f) = self.inner.file.lock() {
            if serde_json::to_writer(&mut *f, &entry).is_ok() {
                let _ = f.write_all(b"\n");
            }
        }
        if self.inner.tx.receiver_count() > 0 {
            let _ = self.inner.tx.send(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FrameDirection, FrameRaw, InspectorEntry};
    use tempfile::TempDir;

    fn sample_frame() -> InspectorEntry {
        InspectorEntry::WsFrame {
            ts_ms: 1,
            direction: FrameDirection::Down,
            flow_id: "test:1".into(),
            size: 4,
            raw: FrameRaw::Text("<Z/>".into()),
            parsed: None,
            emitted: 0,
        }
    }

    #[test]
    fn record_writes_disk_and_broadcasts() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("inspector.jsonl");
        let (writer, tx) = InspectorWriter::open(&path, 8).unwrap();
        let mut rx = tx.subscribe();
        let entry = sample_frame();
        writer.record(entry.clone());

        // Disk
        let body = std::fs::read_to_string(&path).unwrap();
        let line = body.lines().next().unwrap();
        let from_disk: InspectorEntry = serde_json::from_str(line).unwrap();
        assert_eq!(from_disk, crate::privacy::inspector(entry.clone()));

        // Wire
        let from_wire = rx.try_recv().unwrap();
        assert_eq!(from_wire, crate::privacy::inspector(entry));
    }

    #[test]
    fn record_skips_broadcast_when_no_receivers() {
        // No receivers → file should still be written, broadcast no-op.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("inspector.jsonl");
        let (writer, _tx) = InspectorWriter::open(&path, 8).unwrap();
        writer.record(sample_frame());
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().count(), 1);
    }
}
