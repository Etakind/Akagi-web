//! Maintenance-fork update metadata only. Installation is always manual.
pub mod check;
pub use check::{check_for_update, UpdateInfo};
