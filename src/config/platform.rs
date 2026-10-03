//! Runtime web platforms. Legacy history tags are separate.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Majsoul,
    Tenhou,
}
impl Platform {
    pub fn subdir(self) -> &'static str {
        match self {
            Self::Majsoul => "majsoul",
            Self::Tenhou => "tenhou",
        }
    }
    pub fn default_url(self) -> &'static str {
        match self {
            Self::Majsoul => "https://game.maj-soul.com/1/",
            Self::Tenhou => "https://tenhou.net/4/",
        }
    }
    pub fn official_page(self, value: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(value) else {
            return false;
        };
        if url.scheme() != "https"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return false;
        }
        match self {
            Self::Majsoul => {
                url.host_str() == Some("game.maj-soul.com") && url.path().starts_with("/1/")
            }
            Self::Tenhou => url.host_str() == Some("tenhou.net") && url.path().starts_with("/4/"),
        }
    }
}
impl From<Platform> for crate::schema::Platform {
    fn from(p: Platform) -> Self {
        match p {
            Platform::Majsoul => Self::Majsoul,
            Platform::Tenhou => Self::Tenhou,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformConfig {
    pub kind: Platform,
}
