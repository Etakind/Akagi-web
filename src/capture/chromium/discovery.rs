//! Discover by browser-owned TargetInfo. A page's URL request can wait forever
//! in chromiumoxide when that page's initialization stalls; never query every
//! tab's Page handle just to decide which one is the game.
use anyhow::{anyhow, Result};
use chromiumoxide::{cdp::browser_protocol::target::GetTargetsParams, Browser, Page};
use futures_util::{stream, StreamExt};
use std::{collections::HashSet, time::Duration};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct Snapshot {
    pub current: HashSet<String>,
    pub pages: Vec<Page>,
}

pub(super) async fn snapshot(browser: &Browser, official_only: bool) -> Result<Snapshot> {
    if !official_only {
        let pages = tokio::time::timeout(DISCOVERY_TIMEOUT, browser.pages())
            .await
            .map_err(|_| anyhow!("CDP page enumeration timed out"))?
            .map_err(|_| anyhow!("CDP page enumeration failed"))?;
        return Ok(Snapshot {
            current: pages
                .iter()
                .map(|p| p.target_id().inner().clone())
                .collect(),
            pages,
        });
    }
    let targets = tokio::time::timeout(
        DISCOVERY_TIMEOUT,
        browser.execute(GetTargetsParams::default()),
    )
    .await
    .map_err(|_| anyhow!("CDP target discovery timed out"))?
    .map_err(|_| anyhow!("CDP target discovery failed"))?;
    let ids: Vec<_> = targets
        .result
        .target_infos
        .into_iter()
        .filter(|t| t.r#type == "page" && super::cdp::official_majsoul_page(&t.url))
        .map(|t| t.target_id)
        .collect();
    // A transient unavailable Page handle must not discard an active routing
    // task. TargetInfo, not successful get_page calls, determines removals.
    let current = ids.iter().map(|id| id.inner().clone()).collect();
    let pages = stream::iter(ids)
        .map(|id| async move {
            tokio::time::timeout(Duration::from_secs(1), browser.get_page(id))
                .await
                .ok()?
                .ok()
        })
        .buffer_unordered(8)
        .filter_map(|p| async move { p })
        .collect()
        .await;
    Ok(Snapshot { current, pages })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn stalled_page_initialization_does_not_block_game_discovery() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::task::spawn_blocking(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tungstenite::accept(socket).unwrap();
            let targets = vec![
                json!({"targetId":"unrelated", "type":"page", "title":"private", "url":"https://mail.example.test/", "attached":false, "canAccessOpener":false}),
                json!({"targetId":"game", "type":"page", "title":"", "url":"https://game.maj-soul.com/1/", "attached":false, "canAccessOpener":false}),
            ];
            while let Ok(message) = ws.read() {
                let Ok(text) = message.to_text() else {
                    continue;
                };
                let Ok(call) = serde_json::from_str::<serde_json::Value>(text) else {
                    continue;
                };
                let result = match call["method"].as_str().unwrap_or("") {
                    "Target.setDiscoverTargets" => {
                        for t in &targets {
                            ws.send(
                                json!({"method":"Target.targetCreated","params":{"targetInfo":t}})
                                    .to_string()
                                    .into(),
                            )
                            .unwrap();
                        }
                        json!({})
                    }
                    "Target.getTargets" => json!({"targetInfos":targets}),
                    "Target.attachToTarget" => {
                        let id = call["params"]["targetId"].as_str().unwrap();
                        let t = targets.iter().find(|t| t["targetId"] == id).unwrap();
                        ws.send(json!({"method":"Target.attachedToTarget","params":{"sessionId":id,"targetInfo":t,"waitingForDebugger":false}}).to_string().into()).unwrap();
                        json!({"sessionId":id})
                    }
                    // Simulate a page whose initialization never completes.
                    _ => continue,
                };
                if ws
                    .send(json!({"id":call["id"],"result":result}).to_string().into())
                    .is_err()
                {
                    break;
                }
            }
        });
        let (browser, mut handler) = Browser::connect(endpoint).await.unwrap();
        let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let found = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let s = snapshot(&browser, true).await.unwrap();
                assert_eq!(s.current, HashSet::from(["game".to_string()]));
                if !s.pages.is_empty() {
                    break s;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(found.pages.len(), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), found.pages[0].url())
                .await
                .is_err()
        );
        // Rediscovery still completes even though the page-URL query is stuck.
        assert_eq!(snapshot(&browser, true).await.unwrap().pages.len(), 1);
        pump.abort();
        drop(browser);
        server.await.unwrap();
    }
}
