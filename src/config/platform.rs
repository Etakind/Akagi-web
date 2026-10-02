//! 本维护版只采集雀魂网页端；历史标签定义独立保留。
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Majsoul,
}
impl Platform {
    pub fn subdir(self) -> &'static str {
        "majsoul"
    }
}
impl From<Platform> for crate::schema::Platform {
    fn from(_: Platform) -> Self {
        Self::Majsoul
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformConfig {
    pub kind: Platform,
}
