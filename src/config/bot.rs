use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    pub enabled: bool,
    pub active_4p: String,
    pub active_3p: String,
    #[serde(skip_deserializing)]
    pub migration_notice: Option<String>,
}
impl Default for BotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            active_4p: crate::bot::native::NATIVE_4P.into(),
            active_3p: crate::bot::native::NATIVE_3P.into(),
            migration_notice: None,
        }
    }
}
impl BotConfig {
    pub fn active_for(&self, num_players: u8) -> &str {
        if num_players == 3 {
            crate::bot::native::NATIVE_3P
        } else {
            crate::bot::native::NATIVE_4P
        }
    }
}
