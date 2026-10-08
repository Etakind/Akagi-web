//! The always-on-top suggestion overlay ("PiP") window.
//!
//! A second webview, frameless and transparent, that floats over the game
//! client and renders the bot's top-N suggestions. It needs no backend
//! plumbing of its own: `bot-response` is already `app.emit()`-ed, which
//! broadcasts to *every* webview, so the overlay just listens for it (see
//! `frontend/src/routes/Overlay.tsx`).
//!
//! Both windows load the same `index.html`; the frontend branches on
//! `getCurrentWindow().label` to decide which root to render. That keeps the
//! window identity in one place (this module's [`LABEL`]) instead of encoding
//! it in a URL that the router would then have to parse back out.
//!
//! Lifecycle is driven entirely by `config.overlay.enabled`:
//!
//! - at startup, `lib::run` calls [`reconcile`] once;
//! - on every `update_config`, the command calls [`reconcile`] again;
//! - the overlay's own close button flips `enabled` to false, which routes
//!   back through the same path.

use crate::config::OverlayConfig;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_window_state::{StateFlags, WindowExt};
use tracing::{info, warn};

/// Window label. Also the label the `capabilities/overlay.json` capability is
/// scoped to — renaming this without renaming that leaves the overlay webview
/// with no permission to `listen()`, i.e. permanently blank.
pub const LABEL: &str = "overlay";

/// Event carrying a fresh [`OverlayConfig`] to every webview: the overlay reads
/// top-N / opacity off it, and the main window uses it to keep its own toggles
/// in sync when the overlay is closed from the overlay's own × button.
pub const CONFIG_EVENT: &str = "overlay-config";

/// Position and size are the only state worth restoring. The plugin's default
/// (`StateFlags::all()`) would also restore DECORATIONS — putting a title bar
/// back onto a window that is deliberately frameless — so `lib::run` excludes
/// this label from the automatic restore and we do it ourselves.
const RESTORE_FLAGS: StateFlags = StateFlags::POSITION.union(StateFlags::SIZE);

const DEFAULT_WIDTH: f64 = 300.0;
const MIN_WIDTH: f64 = 190.0;

// The rows split the window's height between them (see the `overlay-show` mahgen
// kind), so the window's height has to be a function of how many rows there are.
// A height that fits three rows comfortably squashes five into an unreadable
// smear, and `top_n` is user-settable — so both the starting height and the
// floor are derived from it rather than fixed.
/// Title bar, card border, and the padding around the list.
const CHROME_HEIGHT: f64 = 48.0;
/// Leaves room for label + note, borders and the inter-row gaps at the floor.
const MIN_ROW_HEIGHT: f64 = 42.0;
/// Roomy enough that the tile is worth glancing at without leaning in.
const DEFAULT_ROW_HEIGHT: f64 = 62.0;

fn default_height(top_n: usize) -> f64 {
    CHROME_HEIGHT + top_n as f64 * DEFAULT_ROW_HEIGHT
}

fn min_height(top_n: usize) -> f64 {
    CHROME_HEIGHT + top_n as f64 * MIN_ROW_HEIGHT
}

pub fn get<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

/// Open the overlay, or re-apply the live settings to the one already open.
pub fn open<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) -> tauri::Result<()> {
    let rows = cfg.clamped_top_n();
    let enabled = app
        .try_state::<crate::ipc::AppState>()
        .is_some_and(|state| state.autoplay_context.status.snapshot().enabled);
    let extra = event_height(cfg, enabled);

    if let Some(w) = get(app) {
        w.set_always_on_top(cfg.always_on_top)?;
        // Raising `top_n` in Settings adds rows to a window that may already be
        // at its old floor, so the floor has to move with it — otherwise the new
        // rows just squeeze the existing ones.
        fit_records(&w, cfg, enabled)?;
        w.show()?;
        return Ok(());
    }

    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Akagi Overlay")
        .inner_size(
            DEFAULT_WIDTH,
            default_height(rows) * cfg.font_scale() + extra,
        )
        .min_inner_size(MIN_WIDTH, min_height(rows) * cfg.font_scale() + extra)
        .decorations(false)
        .transparent(true)
        .always_on_top(cfg.always_on_top)
        .skip_taskbar(true)
        .maximizable(false)
        .minimizable(false)
        .shadow(false)
        // Never steal focus from the game client — the whole point of the
        // overlay is that you don't have to look away, let alone click away.
        .focused(false)
        .build()?;

    // Undecorated windows are still resizable from their edges on Windows and
    // macOS, so no in-page resize grip is needed.
    if let Err(e) = w.restore_state(RESTORE_FLAGS) {
        warn!("overlay: could not restore saved geometry: {e}");
    }
    fit_records(&w, cfg, enabled)?;

    info!("overlay window opened");
    Ok(())
}

fn event_height(cfg: &OverlayConfig, enabled: bool) -> f64 {
    if enabled {
        (28.0 + cfg.clamped_event_count() as f64 * 40.0) * cfg.font_scale()
    } else {
        0.0
    }
}

fn fit_records<R: Runtime>(
    window: &WebviewWindow<R>,
    cfg: &OverlayConfig,
    enabled: bool,
) -> tauri::Result<()> {
    let floor = min_height(cfg.clamped_top_n()) * cfg.font_scale() + event_height(cfg, enabled);
    window.set_min_size(Some(LogicalSize::new(MIN_WIDTH, floor)))?;
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    if size.height < floor {
        window.set_size(LogicalSize::new(size.width.max(MIN_WIDTH), floor))?;
    }
    Ok(())
}

/// Resize on the UI thread without moving or focusing the game/overlay.
pub fn resize_for_status<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig, enabled: bool) {
    let handle = app.clone();
    let cfg = cfg.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = get(&handle) {
            if let Err(error) = fit_records(&window, &cfg, enabled) {
                warn!("overlay: status resize failed: {error}");
            }
        }
    });
}

pub fn close<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = get(app) {
        w.close()?;
        info!("overlay window closed");
    }
    Ok(())
}

/// Bring the live window in line with `cfg`: open it, close it, or leave it
/// alone. Idempotent — safe to call on every config save.
///
/// **This must run on the main thread, and it does not block.** On Windows,
/// `WebviewWindowBuilder::build()` called from a Tokio worker — which is where
/// every `#[tauri::command] async fn` runs — deadlocks: it asks the event loop
/// to create the window and then blocks the caller waiting for a reply the main
/// thread cannot deliver. The whole GUI freezes, with no error and no log line,
/// while background tasks carry on as if nothing happened. So the work is
/// posted to the event loop and `reconcile` returns immediately.
///
/// Startup goes through here too even though `lib::run`'s `setup` closure is
/// already on the main thread: one path, one set of rules.
pub fn reconcile<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) {
    let handle = app.clone();
    let cfg = cfg.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle, &cfg)) {
        warn!("overlay: could not schedule reconcile on the main thread: {e}");
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) {
    let result = if cfg.enabled {
        open(app, cfg)
    } else {
        close(app)
    };
    if let Err(e) = result {
        warn!("overlay: reconcile failed: {e}");
        return;
    }
    // Broadcast, not `emit_to(LABEL, …)`: the overlay needs the new top-N /
    // opacity, and the main window needs it to keep its toggles in sync with an
    // overlay that was closed from its own × button.
    if let Err(e) = app.emit(CONFIG_EVENT, cfg) {
        warn!("overlay: could not emit {CONFIG_EVENT}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TOP_N_MAX, TOP_N_MIN};

    /// The rows split the window's height, so a window sized for three rows
    /// squashes five into an unreadable smear. Both the starting height and the
    /// floor have to grow with `top_n` — a fixed height is the bug this replaced.
    #[test]
    fn window_height_grows_with_the_row_count() {
        for n in TOP_N_MIN..TOP_N_MAX {
            assert!(
                min_height(n + 1) > min_height(n),
                "floor must rise from {n} to {} rows",
                n + 1
            );
            assert!(
                default_height(n + 1) > default_height(n),
                "starting height must rise from {n} to {} rows",
                n + 1
            );
        }
    }

    /// Every row must clear `MIN_ROW_HEIGHT` at the floor, at any `top_n` —
    /// that is what keeps a legible tile next to its label.
    #[test]
    fn floor_leaves_every_row_its_minimum() {
        for n in TOP_N_MIN..=TOP_N_MAX {
            let per_row = (min_height(n) - CHROME_HEIGHT) / n as f64;
            assert!(
                per_row >= MIN_ROW_HEIGHT,
                "{n} rows get {per_row}px each, below the {MIN_ROW_HEIGHT}px minimum"
            );
        }
    }

    /// The browser list inserts a gap between rows. The window floor must
    /// leave enough room after those gaps for the scaled two-line label/note
    /// content and its borders at every supported font size.
    #[test]
    fn floor_budgets_for_overlay_row_gaps() {
        const LIST_GAP: f64 = 4.0;
        const LABEL_AND_NOTE_HEIGHT: f64 = 20.0 + 15.0;
        const BORDER_HEIGHT: f64 = 2.0;

        for scale in [12.0 / 14.0, 1.0, 24.0 / 14.0] {
            for n in TOP_N_MIN..=TOP_N_MAX {
                let gaps = n.saturating_sub(1) as f64 * LIST_GAP;
                let list_height = min_height(n) - CHROME_HEIGHT;
                let per_row = (list_height - gaps) / n as f64 * scale;
                let content_height = LABEL_AND_NOTE_HEIGHT * scale + BORDER_HEIGHT;
                assert!(
                    per_row >= content_height,
                    "font scale {scale}, {n} rows get {per_row}px after {gaps}px of list gaps, below the {content_height}px content minimum"
                );
            }
        }
    }

    #[test]
    fn the_starting_height_is_roomier_than_the_floor() {
        for n in TOP_N_MIN..=TOP_N_MAX {
            assert!(default_height(n) > min_height(n));
        }
    }

    #[test]
    fn event_rows_scale_with_overlay_font_size_only() {
        let normal = OverlayConfig::default();
        assert_eq!(event_height(&normal, true), 188.0);
        assert_eq!(event_height(&normal, false), 0.0);

        let large = OverlayConfig {
            font_size: 21,
            ..normal.clone()
        };
        assert_eq!(event_height(&large, true), 282.0);
    }
}
