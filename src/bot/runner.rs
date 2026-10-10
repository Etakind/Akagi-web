//! Sequential local inference runner contract.
use crate::{bot::types::BotResponse, schema::MjaiEvent};
use anyhow::Result;
use async_trait::async_trait;
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait BotRunner: Send {
    /// Rebuild from observed events without running inference or taking actions.
    async fn restore(&mut self, _events: &[MjaiEvent]) -> Result<()> {
        anyhow::bail!("Runner does not support state-only restoration")
    }
    /// Push a batch of events; return the bot's reaction.
    ///
    /// Mjai contract: bot sees every event but only "reacts" at decision
    /// points. When no action is owed, the bot returns
    /// `MjaiEvent::None` wrapped in a `BotResponse`.
    async fn react(&mut self, events: &[MjaiEvent]) -> Result<BotResponse>;

    /// Tear down and respawn for a new game.
    async fn reset(&mut self) -> Result<()>;
}
