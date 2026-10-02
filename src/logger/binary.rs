use anyhow::{Context, Result};
use std::{
    fs::File,
    io::Write,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

/// Append-only binary log writer.
///
/// Frame format (little-endian):
///
/// ```text
/// [u64 micros_since_epoch][u8 tag][u32 len][bytes; len]
/// ```
///
/// `tag` is caller-defined (e.g. 0 = upstream, 1 = downstream).
pub struct BinaryLogger {
    name: String,
    file: Mutex<File>,
}

impl BinaryLogger {
    pub fn new(session_dir: &Path, name: &str) -> Result<Self> {
        let path = session_dir.join(format!("{name}.binlog"));
        let file = crate::util::private_fs::append(&path)
            .with_context(|| format!("Failed to open binary log {}", path.display()))?;
        Ok(Self {
            name: name.to_string(),
            file: Mutex::new(file),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn log(&self, tag: u8, bytes: &[u8]) -> Result<()> {
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        // Raw wire capture is permanently disabled in this privacy build.
        // Keep a valid legacy binlog envelope containing no payload.
        let _ = bytes;
        let len = 0u32;

        let mut buf = Vec::with_capacity(13);
        buf.extend_from_slice(&micros.to_le_bytes());
        buf.push(tag);
        buf.extend_from_slice(&len.to_le_bytes());

        let mut file = self.file.lock().expect("binary log mutex poisoned");
        file.write_all(&buf)
            .with_context(|| format!("Failed to write to binary log '{}'", self.name))?;
        Ok(())
    }

    /// Like [`log`], but logs a `warn` event on failure rather than returning
    /// the error. Convenient for hot paths where logging failure must not
    /// disrupt control flow.
    pub fn write(&self, tag: u8, bytes: &[u8]) {
        if let Err(e) = self.log(tag, bytes) {
            tracing::warn!("binary log '{}' write failed: {e}", self.name);
        }
    }
}
