//! 手动浏览器接入检查。输出固定结果码；诊断通过项目脱敏。不读取账号，不进行登录或游戏输入。
//! `cargo run --example browser_probe -- attach` 检查 9222；`isolated` 启动临时 Edge。
use akagi::capture::{
    chromium::{detect, ChromiumBackend},
    CaptureBackend, CaptureCtx, ShutdownToken,
};
use akagi::{
    autoplay::AutoplayContext,
    config::{ChromiumConfig, Platform},
    event_bus,
    logger::Session,
};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| eprintln!("BROWSER_PROBE_INTERNAL_ERROR")));
    let mode = std::env::args().nth(1).unwrap_or_else(|| "attach".into());
    let result = probe(&mode).await;
    println!("{result}");
    if result != "BROWSER_PAGE_SUBSCRIBED" {
        std::process::exit(1);
    }
}

async fn probe(mode: &str) -> &'static str {
    if !matches!(mode, "attach" | "isolated") {
        return "BROWSER_PROBE_INVALID_MODE";
    }
    let Ok(temp) = tempfile::tempdir() else {
        return "BROWSER_PROBE_TEMP_FAILED";
    };
    let Ok(session) = Session::init(temp.path(), "off", "off", &[]) else {
        return "BROWSER_PROBE_LOG_FAILED";
    };
    let mut cfg = ChromiumConfig::default();
    if mode == "attach" {
        cfg.attach_port = 9222;
    } else {
        let Some(browser) = detect::detect_system_browsers()
            .into_iter()
            .find(|b| b.kind == detect::BrowserKind::Edge)
        else {
            return "BROWSER_NOT_INSTALLED";
        };
        cfg.executable = browser.path.to_string_lossy().into_owned();
        cfg.user_data_dir = temp.path().join("profile").to_string_lossy().into_owned();
        cfg.extra_args = vec!["--headless=new".into()];
    }
    let context = Arc::new(AutoplayContext::new());
    let (shutdown, stop) = ShutdownToken::new();
    let ctx = CaptureCtx {
        session: Arc::new(session),
        platform: Platform::Majsoul,
        mjai_bus: event_bus::mjai_bus(),
        notify_bus: event_bus::notify_bus(),
        autoplay: Some(context.clone()),
        http: Default::default(),
    };
    let mut task = tokio::spawn(Box::new(ChromiumBackend::new(cfg)).run(ctx, shutdown));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    let status = loop {
        tokio::select! {
            result = &mut task => {
                break match result {
                    Ok(Err(e)) if e.to_string().contains("403") || e.to_string().contains("denied") => "BROWSER_AUTHORIZATION_REJECTED",
                    _ => "BROWSER_CONNECTION_UNAVAILABLE",
                };
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if context.page.read().await.is_some() { break "BROWSER_PAGE_SUBSCRIBED"; }
                if tokio::time::Instant::now() >= deadline { break "BROWSER_PAGE_OR_AUTHORIZATION_TIMEOUT"; }
            }
        }
    };
    stop.notify_waiters();
    if !task.is_finished()
        && tokio::time::timeout(Duration::from_secs(10), &mut task)
            .await
            .is_err()
    {
        task.abort();
        let _ = task.await;
    }
    drop(context);
    status
}
