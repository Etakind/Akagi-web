//! Opt-in live acceptance harness. Uses production capture/recovery/input code.
//! Run only after opening a free AI game yourself. Never starts a game.
//! cargo run --example midgame_smoke -- 9222 90
use akagi::{
    autoplay, bot,
    capture::{
        chromium::ChromiumBackend,
        recovery::{RecoveryPhase, AUTOPLAY, BOT},
        CaptureBackend, CaptureCtx, ShutdownToken,
    },
    config::{AppConfig, ChromiumConfig},
    event_bus, game_state,
    logger::Session,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let port = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(9222);
    let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(90);
    let root = std::env::temp_dir().join("akagi-midgame-acceptance");
    let session = Arc::new(Session::init(&root, "error", "info", &[])?);
    println!("acceptance_log={}", session.dir().display());
    let mut config = AppConfig::default();
    config.autoplay.enabled = true;
    let config = Arc::new(RwLock::new(config));
    let ctx = Arc::new(autoplay::AutoplayContext::new());
    ctx.set_enabled(true);
    ctx.recovery.register(BOT | AUTOPLAY);
    let bus = event_bus::mjai_bus();
    let post = event_bus::post_tracker_bus();
    let responses = event_bus::bot_response_bus();
    let notify = event_bus::notify_bus();
    let tracker = game_state::spawn_with_post(bus.subscribe(), Some(post.clone()));
    let manager = bot::manager::BotManager::new(
        config.clone(),
        responses.clone(),
        event_bus::bot_status_bus(),
        notify.clone(),
        session.inspector(),
    );
    let bot_task = tokio::spawn(manager.run(post.subscribe()));
    let auto = autoplay::manager::AutoplayManager::new(
        config,
        ctx.clone(),
        tracker.clone(),
        bus.clone(),
        notify.clone(),
        root.clone(),
    );
    let auto_task = tokio::spawn(auto.run(responses.clone()));
    let mut response_rx = responses.subscribe();
    let chromium = ChromiumConfig {
        attach_port: port,
        ..Default::default()
    };
    let (stop, stop_capture) = ShutdownToken::new();
    let capture_ctx = CaptureCtx {
        session: session.clone(),
        platform: akagi::config::Platform::Majsoul,
        mjai_bus: bus.clone(),
        notify_bus: notify,
        autoplay: Some(ctx.clone()),
        http: Default::default(),
    };
    let capture = tokio::spawn(Box::new(ChromiumBackend::new(chromium)).run(capture_ctx, stop));
    let mut recovery_started = false;
    let mut recovery_task = None;
    let mut succeeded = std::collections::HashSet::new();
    let mut last_status = None;
    let ledger = session.flow_logger("majsoul", "autoplay.log", "acceptance input confirmation")?;
    let lifecycle = session.flow_logger("majsoul", "recovery.log", "acceptance recovery")?;
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
    while tokio::time::Instant::now() < end {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = timer.tick() => {},
        }
        let state = ctx.recovery.snapshot();
        if last_status.as_ref() != Some(&state) {
            lifecycle.writeln(&serde_json::json!({"ts_ms": chrono::Utc::now().timestamp_millis(), "status": &state}).to_string());
            println!(
                "recovery={:?} method={:?} reason={:?}",
                state.phase, state.method, state.reason
            );
            last_status = Some(state.clone());
        }
        if !recovery_started
            && state.can_recover
            && matches!(
                state.phase,
                RecoveryPhase::Inactive | RecoveryPhase::Missing
            )
            && ctx.page.read().await.is_some()
        {
            recovery_started = true;
            let ctx = ctx.clone();
            let bus = bus.clone();
            recovery_task = Some(tokio::spawn(async move {
                let result = akagi::capture::chromium::recovery::recover_game(ctx, bus).await;
                println!(
                    "recover_result={}",
                    if result.is_ok() { "ready" } else { "failed" }
                );
                result
            }));
        }
        while let Ok(response) = response_rx.try_recv() {
            println!(
                "decision={} live_context={}",
                serde_json::to_value(&response.action)?["type"],
                response.decision_context.token.is_some()
            );
        }
        if let Some(update) = ctx.status.poll() {
            ledger.writeln(&serde_json::json!({"ts_ms": chrono::Utc::now().timestamp_millis(), "status": &update}).to_string());
            for record in update.records {
                if record.phase == autoplay::status::Phase::Succeeded && succeeded.insert(record.id)
                {
                    println!(
                        "confirmed_operation={} action={}",
                        record.id, record.action["type"]
                    );
                }
            }
        }
        if succeeded.len() >= 3 {
            break;
        }
    }
    let method = ctx.recovery.snapshot().method;
    ctx.set_enabled(false);
    stop_capture.notify_one();
    if let Some(task) = recovery_task {
        task.abort();
    }
    let _ = tokio::time::timeout(Duration::from_secs(3), capture).await;
    bot_task.abort();
    auto_task.abort();
    println!("acceptance_confirmed={} method={method:?}", succeeded.len());
    anyhow::ensure!(
        recovery_started && succeeded.len() >= 3,
        "Live recovery plus three server-confirmed operations was not established"
    );
    Ok(())
}
