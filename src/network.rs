//! Remaining app network purposes: maintenance release metadata, explicit
//! official browser downloads, and loopback CDP discovery. No upload API.
use anyhow::{Context, Result};
use std::time::Duration;
#[derive(Clone, Copy)]
pub enum Purpose {
    ReleaseMetadata,
    BrowserDownload,
    LocalDiscovery,
}
pub fn client(purpose: Purpose) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("akagi-maintenance/", env!("CARGO_PKG_VERSION")));
    match purpose {
        Purpose::LocalDiscovery => builder = builder.no_proxy().timeout(Duration::from_secs(3)),
        Purpose::ReleaseMetadata => {
            builder = builder.https_only(true).timeout(Duration::from_secs(15))
        }
        Purpose::BrowserDownload => builder = builder.https_only(true),
    }
    builder.build().context("network client unavailable")
}
