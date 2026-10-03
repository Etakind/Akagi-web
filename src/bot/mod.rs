//! Bundled local inference. No external code installation or network inference.
pub mod manager;
pub mod native;
pub mod runner;
pub mod supervisor;
pub mod types;
pub use manager::BotManager;
pub use runner::BotRunner;
pub use supervisor::run_bot_manager;
pub use types::BotResponse;
