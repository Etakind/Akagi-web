use anyhow::{Context, Result};
use std::{fs::File, io::Write, path::Path, sync::Mutex};

/// Append-only text log writer, one file per "flow" (e.g. one Majsoul
/// WebSocket connection). Lines are written atomically under a mutex so
/// concurrent direction tasks don't interleave bytes mid-line.
pub struct FlowLogger {
    label: String,
    file: Mutex<File>,
    game_events: bool,
}

impl FlowLogger {
    /// Open `<session_dir>/<subdir>/<file_name>`, creating `subdir` if
    /// needed. Caller supplies the full filename (extension included), so
    /// the same `FlowLogger` works for `.log`, `.mjai.jsonl`, etc.
    /// `label` is purely for diagnostics (used in failure logs).
    pub fn new(
        session_dir: &Path,
        subdir: &str,
        file_name: &str,
        label: impl Into<String>,
    ) -> Result<Self> {
        let dir = session_dir.join(subdir);
        crate::util::private_fs::directory(&dir)
            .with_context(|| format!("Failed to create flow log dir {}", dir.display()))?;
        let path = dir.join(file_name);
        let file = crate::util::private_fs::append(&path)
            .with_context(|| format!("Failed to open flow log {}", path.display()))?;
        Ok(Self {
            label: label.into(),
            file: Mutex::new(file),
            game_events: file_name.ends_with(".mjai.jsonl"),
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Append `line` followed by `\n`. No formatting beyond the newline.
    pub fn writeln(&self, line: &str) {
        let safe;
        let line = if self.game_events {
            line
        } else {
            let input = serde_json::from_str::<serde_json::Value>(line).unwrap_or_default();
            let mut meta = serde_json::Map::new();
            for name in ["msg_id", "len"] {
                if let Some(v) = input.get(name).filter(|v| v.is_number()) {
                    meta.insert(name.into(), v.clone());
                }
            }
            if let Some(ts) = input
                .get("ts")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            {
                meta.insert("ts".into(), ts.to_rfc3339().into());
            }
            for (name, allowed) in [
                ("dir", &["up", "down"][..]),
                (
                    "type",
                    &[
                        "request",
                        "response",
                        "notification",
                        "notify",
                        "unknown",
                        "error",
                    ][..],
                ),
            ] {
                let v = input
                    .get(name)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if allowed.contains(&v.as_str()) {
                    meta.insert(name.into(), v.into());
                }
            }
            if let Some(v) = input.get("method").and_then(|v| v.as_str()) {
                meta.insert("method".into(), crate::privacy::safe_method(v).into());
            }
            safe = serde_json::Value::Object(meta).to_string();
            &safe
        };
        let mut file = self.file.lock().expect("flow log mutex poisoned");
        if let Err(e) = file
            .write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
        {
            tracing::warn!("flow log '{}' write failed: {e}", self.label);
        }
    }
}
