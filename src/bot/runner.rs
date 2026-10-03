//! Sequential local inference runner contract.
use crate::{bot::types::BotResponse, schema::MjaiEvent};
use anyhow::Result;
use async_trait::async_trait;
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait BotRunner: Send {
    /// Push a batch of events; return the bot's reaction.
    ///
    /// Mjai contract: bot sees every event but only "reacts" at decision
    /// points. When no action is owed, the bot returns
    /// `MjaiEvent::None` wrapped in a `BotResponse`.
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse>;

    /// Tear down and respawn for a new game.
    async fn reset(&mut self) -> Result<()>;
}
