//! 浏览器采集生命周期：启动、停止、重启以及界面状态通知。

use crate::capture::{
    chromium::ChromiumBackend, CaptureBackend, CaptureCtx, CaptureKind as RtCaptureKind,
    ShutdownToken,
};
use crate::ipc::state::AppState;
use crate::schema::{CaptureKind, CaptureStatus, Notification};
use anyhow::Result;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

fn schema_kind(k: RtCaptureKind) -> CaptureKind {
    match k {
        RtCaptureKind::Chromium => CaptureKind::Chromium,
    }
}

/// Stop the running backend (if any) and wait briefly for the
/// supervisor task to flip status to `Stopped` before returning. Used by
/// `restart_capture` so the spawn that follows starts on a clean slate.
pub async fn stop_and_wait(state: &AppState, max_wait: Duration) {
    // Subscribe *before* signalling shutdown so we can't lose the
    // resulting status emission to a race.
    let mut rx = state.capture_status_bus.subscribe();

    let stop = {
        let mut ctl = state.capture_control.lock().await;
        ctl.stop.take()
    };
    let Some(tx) = stop else {
        return; // not running
    };
    // Kick in-flight WS flows so existing connections actually disconnect
    // (mirrors stop_capture semantics).
    let _ = tx.send(());

    let _ = tokio::time::timeout(max_wait, async {
        loop {
            match rx.recv().await {
                Ok(CaptureStatus::Stopped) | Ok(CaptureStatus::Error { .. }) => break,
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    })
    .await;
}

/// Stop the running backend (if any) and start a fresh one based on the
/// current `cfg.capture.mode`. Idempotent: safe to call when nothing is
/// running (becomes a plain `spawn`).
pub async fn restart_capture(state: AppState) -> Result<()> {
    stop_and_wait(&state, Duration::from_secs(2)).await;
    spawn_capture_supervisor(state).await
}

/// Spawn the capture supervisor task and wire it into
/// `state.capture_control`. Returns once the task has been spawned (not
/// when the backend is actually live).
pub async fn spawn_capture_supervisor(state: AppState) -> Result<()> {
    // Defensive: if a previous task is still alive, refuse rather than
    // race two oneshots into the same control slot.
    {
        let ctl = state.capture_control.lock().await;
        if ctl.stop.is_some() {
            warn!("capture supervisor: backend already running, ignoring spawn");
            return Ok(());
        }
    }

    let (chromium_cfg, http_cfg, platform) = {
        let cfg = state.config.read().await;
        if let Some(reason) = &cfg.capture.unavailable_reason {
            anyhow::bail!("{reason}");
        }
        (
            cfg.capture.chromium.clone(),
            cfg.capture.http.clone(),
            cfg.platform.kind,
        )
    };
    let (shutdown_token, shutdown_notify) = ShutdownToken::new();
    let backend: Box<dyn CaptureBackend> = Box::new(ChromiumBackend::new(chromium_cfg));
    let descriptor = backend.descriptor();
    let kind = schema_kind(descriptor.kind);

    // Status: emit Running with a descriptive label up front so the UI
    // sees an immediate transition from Stopped → Running. Backends that
    // fail mid-startup will flip back to Error from the spawned task.
    let label = match descriptor.kind {
        RtCaptureKind::Chromium => format!("chromium ({})", descriptor.label),
    };
    let running_status = CaptureStatus::Running {
        kind,
        descriptor: label.clone(),
    };
    {
        let mut ctl = state.capture_control.lock().await;
        ctl.status = running_status.clone();
    }
    let _ = state.capture_status_bus.send(running_status);

    state.autoplay_context.autoplay_enabled.store(
        state.config.read().await.autoplay.enabled,
        std::sync::atomic::Ordering::SeqCst,
    );
    *state.autoplay_context.platform.write().unwrap() = platform;
    state
        .autoplay_context
        .generation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *state.autoplay_context.tenhou_state.write().unwrap() = None;
    let ctx = CaptureCtx {
        session: state.log_session.clone(),
        platform,
        mjai_bus: state.mjai_bus.clone(),
        notify_bus: state.notify_bus.clone(),
        autoplay: Some(state.autoplay_context.clone()),
        http: http_cfg,
    };
    let status_bus = state.capture_status_bus.clone();
    let control = state.capture_control.clone();
    let notify_bus = state.notify_bus.clone();

    // Splice shutdown_notify into a oneshot-shaped trigger for
    // `capture_control.stop`. The existing API (commands::stop_capture)
    // sends `()` on the oneshot; we flip `notify_waiters` in response.
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut ctl = state.capture_control.lock().await;
        ctl.stop = Some(stop_tx);
    }
    let shutdown_notify_for_relay = shutdown_notify.clone();
    tokio::spawn(async move {
        let _ = (&mut stop_rx).await;
        shutdown_notify_for_relay.notify_waiters();
    });

    info!(
        "capture supervisor: starting {} backend ({})",
        descriptor.kind.as_str(),
        label
    );
    tokio::spawn(async move {
        let result = backend.run(ctx, shutdown_token).await;
        let next = match &result {
            Ok(()) => {
                info!("capture supervisor: stopped cleanly");
                CaptureStatus::Stopped
            }
            Err(e) => {
                let msg = format!("{e:#}");
                error!("capture supervisor: {msg}");
                // Surface via toast so the user sees it without watching
                // the dashboard. Sticky id `capture-error` lets the next
                // restart's Running emission overwrite the same toast.
                let _ = notify_bus.send(
                    Notification::error(format!("{} capture stopped", kind_label(kind)))
                        .body(format!(
                            "{msg} — open Settings → Capture and click Restart."
                        ))
                        .sticky()
                        .id("capture-error"),
                );
                CaptureStatus::Error {
                    kind,
                    descriptor: Some(label.clone()),
                    error: msg,
                }
            }
        };
        {
            let mut ctl = control.lock().await;
            ctl.stop = None;
            ctl.status = next.clone();
        }
        let _ = status_bus.send(next);
    });

    Ok(())
}

fn kind_label(k: CaptureKind) -> &'static str {
    match k {
        CaptureKind::Chromium => "Chromium",
    }
}
