//! Run the bundled model manager on the post-tracker event bus.
use crate::{
    config::AppConfig,
    event_bus::{BotResponseBus, BotStatusBus, NotifyBus, PostTrackerBus},
    inspector::InspectorWriter,
};
use std::sync::Arc;
use tokio::sync::RwLock;
pub async fn run_bot_manager(
    config: Arc<RwLock<AppConfig>>,
    events: PostTrackerBus,
    response: BotResponseBus,
    status: BotStatusBus,
    notify: NotifyBus,
    inspector: InspectorWriter,
) -> anyhow::Result<()> {
    super::BotManager::new(config, response, status, notify, inspector)
        .run(events.subscribe())
        .await
}
