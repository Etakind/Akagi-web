//! Autoplay manager: subscribes to bot decisions + mjai events,
//! translates them into UI clicks dispatched via CDP.
//!
//! Lifecycle:
//! - Spawned by `crate::lib::run` when `cfg.autoplay.enabled = true`.
//! - One long-lived `tokio::select!` loop over `BotResponseBus` and
//!   `MjaiBus`. Bot responses drive clicks; mjai events update local
//!   per-game tracking state (`last_kawa_tile`, `last_self_tsumo`,
//!   `self_riichi_accepted`).
//!
//! Failure modes are silent-by-design: if the page handle is missing
//! (chromium backend not running) or the canvas-rect query fails, the
//! manager logs a warning and skips the click. The bot pipeline is
//! untouched; the user can still play the round manually.

use crate::autoplay::cdp_input::{dispatch_click_shaped, evaluate_canvas_rect};
use crate::autoplay::context::{AutoplayContext, CanvasRect};
use crate::autoplay::majsoul::MajsoulAutoplay;
use crate::autoplay::platform::{ActionContext, PlatformAutoplay, Step};
use crate::autoplay::tenhou::TenhouAutoplay;
use crate::autoplay::verify::InputTicket;
use crate::bot::BotResponse;
use crate::config::AppConfig;
use crate::event_bus::{BotResponseBus, MjaiBus, NotifyBus};
use crate::game_state::tracker::GameTracker;
use crate::schema::MjaiEvent;
use chromiumoxide::Page;
use riichienv_core::action::Action;
use riichienv_core::state::legal_actions::GameStateLegalActions;
use riichienv_core::state_3p::legal_actions::GameState3PLegalActions;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast::error::RecvError, Mutex, RwLock};
use tracing::{debug, info, warn};

/// How long before a cached `CanvasRect` is treated as stale and re-queried.
const CANVAS_RECT_TTL: Duration = Duration::from_secs(30);

pub struct AutoplayManager {
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    tracker: Arc<Mutex<GameTracker>>,
    mjai_bus: MjaiBus,
    /// For telling the user about a decision that came out wrong — see the
    /// riichi check in `handle_bot_response` and the dead-click pause.
    notify: NotifyBus,
    majsoul: MajsoulAutoplay,
    tenhou: TenhouAutoplay,
    state: ManagerState,
    /// User Lua delay policy (hot-reloaded from disk; see
    /// `autoplay::delay::script`).
    delay_script: crate::autoplay::delay::ScriptHost,
    /// Directory holding the loaded config file; the script lives at
    /// `<config_dir>/delay.lua`.
    config_dir: std::path::PathBuf,
}

#[derive(Default)]
struct ManagerState {
    last_kawa_tile: Option<String>,
    last_self_tsumo: Option<String>,
    self_riichi_accepted: bool,
    canvas_rect_at: Option<Instant>,
    /// Decisions in a row where the client accepted nothing we pressed.
    /// Reset by any input the client does accept.
    dead_clicks: u32,
    /// Cached seat index for our player. Captured directly from
    /// `StartGame { id }` and kept across kyoku resets. Avoids try_lock
    /// failures in the synchronous mjai event handler causing missed
    /// tsumo/dahai updates, and is available from the very first event
    /// rather than waiting for the first successful `handle_bot_response`.
    cached_our_seat: Option<u8>,
    acted_window: Option<Instant>,
}

impl AutoplayManager {
    pub fn new(
        cfg: Arc<RwLock<AppConfig>>,
        ctx: Arc<AutoplayContext>,
        tracker: Arc<Mutex<GameTracker>>,
        mjai_bus: MjaiBus,
        notify: NotifyBus,
        config_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            cfg,
            ctx,
            tracker,
            mjai_bus,
            notify,
            majsoul: MajsoulAutoplay::new(),
            tenhou: TenhouAutoplay::new(),
            state: ManagerState::default(),
            delay_script: crate::autoplay::delay::ScriptHost::default(),
            config_dir,
        }
    }

    /// Run forever. Returns `Err` only on bus closure (process exit).
    pub async fn run(mut self, response_bus: BotResponseBus) -> anyhow::Result<()> {
        let mut bot_rx = response_bus.subscribe();
        let mut mjai_rx = self.mjai_bus.subscribe();
        info!("autoplay manager started");
        loop {
            tokio::select! {
                msg = bot_rx.recv() => match msg {
                    Ok(resp) => self.handle_bot_response(resp).await,
                    Err(RecvError::Lagged(n)) => warn!("autoplay: bot bus lagged {n}"),
                    Err(RecvError::Closed) => {
                        info!("autoplay: bot bus closed; exiting");
                        return Ok(());
                    }
                },
                msg = mjai_rx.recv() => match msg {
                    Ok(ev) => {
                        self.handle_mjai_event(&ev);
                    }
                    Err(RecvError::Lagged(n)) => warn!("autoplay: mjai bus lagged {n}"),
                    Err(RecvError::Closed) => {
                        info!("autoplay: mjai bus closed; exiting");
                        return Ok(());
                    }
                },
            }
        }
    }

    async fn handle_bot_response(&mut self, resp: BotResponse) {
        // Re-read config every iteration so `cfg.autoplay.enabled` can be
        // toggled at runtime via the Settings UI without restarting.
        let cfg_guard = self.cfg.read().await;
        if !cfg_guard.autoplay.enabled || !self.ctx.accepts_decision(resp.decision_started) {
            return;
        }
        let cfg = cfg_guard.autoplay.majsoul.clone();
        let delay_cfg = cfg_guard.autoplay.delay.clone();
        let platform_kind = cfg_guard.platform.kind;
        let generation = self
            .ctx
            .generation
            .load(std::sync::atomic::Ordering::SeqCst);
        drop(cfg_guard);
        let tenhou_state = self.ctx.tenhou_state.read().ok().and_then(|g| g.clone());
        let planned_window = tenhou_state.as_ref().and_then(|s| s.window);

        // Snapshot the server time budget for the current decision window
        // (written by the Majsoul bridge; None off-Majsoul or pre-game) and
        // normalize the bot's confidence metadata. Both feed the delay
        // model — neither can alter the chosen action. `opened_at` is kept
        // as the window's identity for the post-sleep staleness check.
        let planned_budget = self.ctx.time_budget.read().ok().and_then(|g| *g);
        let budget = planned_budget.map(|b| crate::autoplay::delay::BudgetSnapshot {
            fixed_ms: b.fixed_ms,
            add_ms: b.add_ms,
            elapsed_ms: b.elapsed_ms(),
        });
        let probs = crate::autoplay::delay::probs::normalize_meta(resp.meta.as_ref());

        // The delay script lives at a fixed path next to the config file.
        // In Lua mode it is generated from the bundled default when
        // missing, then hot-reloaded on change (cheap mtime stat). In
        // legacy mode the script is dropped entirely.
        let lua_mode = delay_cfg.mode == crate::config::DelayMode::Lua;
        let script_path = self.config_dir.join("delay.lua");
        if lua_mode {
            self.delay_script.ensure_default(&script_path);
        }
        self.delay_script.maybe_reload(&script_path, lua_mode);

        // Pull our seat + legal actions from the live engine state. This
        // bracket releases the tracker mutex before we sleep/click.
        let (our_seat, legal_actions, snapshot, num_players) = {
            let tracker = self.tracker.lock().await;
            let our_seat = match tracker.our_seat() {
                Some(s) => s,
                None => return, // game hasn't started or no perspective tagged
            };
            // Keep cached_our_seat up to date for handle_mjai_event.
            self.state.cached_our_seat = Some(our_seat);
            let snapshot = match tracker.snapshot() {
                Some(s) => s,
                None => return,
            };
            let num_players = snapshot.num_players;
            let legal_actions: Vec<Action> = if num_players == 3 {
                tracker
                    .state_3p()
                    .map(|s| s._get_legal_actions_internal(our_seat))
                    .unwrap_or_default()
            } else {
                tracker
                    .state()
                    .map(|s| s._get_legal_actions_internal(our_seat))
                    .unwrap_or_default()
            };
            (our_seat, legal_actions, snapshot, num_players)
        };

        let action_ctx = ActionContext {
            tenhou: tenhou_state.as_ref(),
            action: &resp.action,
            snapshot: &snapshot,
            legal_actions: &legal_actions,
            our_seat,
            last_kawa_tile: self.state.last_kawa_tile.as_deref(),
            last_self_tsumo: self.state.last_self_tsumo.as_deref(),
            self_riichi_accepted: self.state.self_riichi_accepted,
            num_players,
            cfg: &cfg,
            delay_cfg,
            budget,
            probs,
            delay_script: self.delay_script.script(),
        };

        if platform_kind == crate::config::Platform::Tenhou
            && (planned_window.is_none()
                || planned_window.is_some_and(|w| self.state.acted_window == Some(w.opened_at)))
        {
            return;
        }
        let platform: &dyn PlatformAutoplay = match platform_kind {
            crate::config::Platform::Majsoul => &self.majsoul,
            crate::config::Platform::Tenhou => &self.tenhou,
        };
        let plan = platform.plan(&action_ctx);
        if plan.steps.is_empty() {
            return;
        }
        debug!(
            "autoplay: action={:?} steps={}",
            resp.action,
            plan.steps.len(),
        );

        // Only a click needs to know where the canvas is. A plan built of
        // `Send` steps talks to the page's socket and would otherwise be
        // held hostage by a rect query it never uses.
        let needs_canvas = plan.steps.iter().any(|s| matches!(s, Step::Click { .. }));
        let rect = if needs_canvas {
            // Cache + TTL. If we can't resolve one, drop the click — the page
            // handle isn't ready yet (e.g. user still on the lobby), or the
            // chromium backend isn't running at all.
            match self.canvas_rect_resolve().await {
                Some(r) => Some(r),
                None => {
                    warn!(
                        "autoplay: no canvas rect — skipping click for {:?}",
                        resp.action
                    );
                    return;
                }
            }
        } else {
            None
        };

        // Ticket taken before the first press: the check afterwards asks
        // whether the client sent *another* input command, so an input from
        // the previous decision cannot be mistaken for this one landing.
        let ticket = self.ctx.input_watch.ticket();

        // Whether this plan declares riichi: the reach declaration and its
        // tile go out in one plan (the declaring discard is pre-resolved onto
        // `Reach.pai`), so the reach action alone identifies it.
        let declares_reach = matches!(resp.action, MjaiEvent::Reach { .. });

        // An attached client loaded before Fetch instrumentation cannot safely act.
        if platform_kind == crate::config::Platform::Tenhou {
            let guard = self.ctx.page.read().await;
            let Some(page) = guard.as_ref() else {
                return;
            };
            let adapter = page
                .evaluate("(() => typeof window.__akagiTenhou?.R?.c21 === 'function')()")
                .await;
            if !matches!(adapter, Ok(value) if value.value().and_then(|v| v.as_bool()) == Some(true))
            {
                drop(guard);
                self.cfg.write().await.autoplay.enabled = false;
                self.ctx.set_enabled(false);
                let _ = self.notify.send(crate::schema::Notification::warn("Tenhou autoplay unavailable").body("Client adapter missing. Re-enter the official page yourself when safe, then enable autoplay again. Observation remains available.").sticky());
                return;
            }
        }
        let mut window_checked = false;
        if let Some(window) = planned_window {
            self.state.acted_window = Some(window.opened_at);
        }
        for step in &plan.steps {
            let current = self.cfg.read().await;
            if !self
                .ctx
                .autoplay_enabled
                .load(std::sync::atomic::Ordering::SeqCst)
                || !current.autoplay.enabled
                || current.platform.kind != platform_kind
                || self
                    .ctx
                    .generation
                    .load(std::sync::atomic::Ordering::SeqCst)
                    != generation
            {
                return;
            }
            drop(current);
            match step {
                Step::Sleep { duration_ms } => {
                    tokio::time::sleep(Duration::from_millis(*duration_ms as u64)).await;
                }
                Step::AwaitReady { timeout_ms } => {
                    let page_guard = self.ctx.page.read().await;
                    let Some(page) = page_guard.as_ref() else {
                        warn!("autoplay: no page handle — cannot wait for the client");
                        return;
                    };
                    if !self.ctx.page_allowed(page).await || !self.plan_active(generation).await {
                        return;
                    }
                    if !Self::await_turn_ready(page, *timeout_ms).await {
                        return;
                    }
                }
                Step::DomClick { selectors, label } => {
                    if self.tenhou_window_moved(planned_window) {
                        warn!(
                            "autoplay: decision window closed mid-delay — dropping stale {label}"
                        );
                        return;
                    }
                    let page_guard = self.ctx.page.read().await;
                    let Some(page) = page_guard.as_ref() else {
                        warn!("autoplay: no page handle — cannot press {label}");
                        return;
                    };
                    // The client only renders the buttons it is currently
                    // offering, so a selector that matches nothing means the
                    // decision resolved while we were thinking. Report it and
                    // stop rather than pressing something else.
                    // The buttons exist only between the end of the
                    // client's animation for the triggering frame and
                    // whatever resolves the window, and neither edge is
                    // observable from here — a single look loses the race
                    // either way. Wait for the element instead, bounded by
                    // what is left of the turn.
                    if !self.ctx.page_allowed(page).await || !self.plan_active(generation).await {
                        return;
                    }
                    match self
                        .press_when_offered(page, selectors, label, generation, planned_window)
                        .await
                    {
                        true => {}
                        false => return,
                    }
                }
                Step::Discard { tile_index } => {
                    // The client's discard handler applies locally whether or
                    // not it is our turn — its own UI only reaches it while
                    // one is — so a stale call desyncs the board, not just
                    // wastes a frame. The riichi plan is the exception: its
                    // own button press replaces the window (the server acks
                    // the declaration and the bridge re-opens it), and the
                    // tile it owes is still this plan's to throw.
                    if discard_needs_window_guard(&resp.action)
                        && self.tenhou_window_moved(planned_window)
                    {
                        warn!(
                            "autoplay: decision window closed mid-delay — dropping stale discard"
                        );
                        return;
                    }
                    let page_guard = self.ctx.page.read().await;
                    let Some(page) = page_guard.as_ref() else {
                        warn!("autoplay: no page handle — cannot discard");
                        return;
                    };
                    if !self.ctx.page_allowed(page).await || !self.plan_active(generation).await {
                        return;
                    }
                    match crate::autoplay::cdp_input::discard_tile(page, *tile_index).await {
                        Ok(true) => info!("autoplay: discarded tile index {tile_index}"),
                        Ok(false) => {
                            warn!(
                                "autoplay: the client script was not instrumented, so its \
                                 discard handler is unreachable; skipping"
                            );
                            return;
                        }
                        Err(e) => {
                            warn!("autoplay: discard failed: {e:#}");
                            return;
                        }
                    }
                }
                Step::Click { x_norm, y_norm } => {
                    let Some(rect) = rect else {
                        warn!("autoplay: click step with no canvas rect; skipping");
                        continue;
                    };
                    // The decision window can close while we sleep: a
                    // higher-priority claimant (ron over our chi window)
                    // resolves it early, and the *next* window's buttons
                    // render at the same coordinates — a stale click
                    // would press a live button of the wrong decision.
                    // The bridge rewrites the budget slot exactly when
                    // that happens, so the slot still holding the window
                    // we planned against is the cheap validity check.
                    // Checked once, before the first click: later steps
                    // of one plan run inside our own action's window.
                    // With no budget tracked (off-Majsoul) there is no
                    // signal — behaviour is unchanged there.
                    if !window_checked {
                        window_checked = true;
                        if planned_budget.is_some() {
                            let current = self.ctx.time_budget.read().ok().and_then(|g| *g);
                            if current.map(|b| b.opened_at) != planned_budget.map(|b| b.opened_at) {
                                warn!(
                                    "autoplay: decision window closed mid-delay — dropping stale click for {:?}",
                                    resp.action
                                );
                                return;
                            }
                        }
                    }
                    let (px, py) = rect.pixel(*x_norm, *y_norm);
                    if !rect.contains(px, py) {
                        warn!(
                            "autoplay: click ({px},{py}) outside canvas rect {:?}; skipping",
                            rect
                        );
                        continue;
                    }
                    // Need to re-acquire the page handle on each click;
                    // it may have been replaced (tab reload) between
                    // successive clicks within one action.
                    let page_guard = self.ctx.page.read().await;
                    let Some(page) = page_guard.as_ref() else {
                        warn!("autoplay: no page handle — aborting click sequence");
                        return;
                    };
                    if !self.ctx.page_allowed(page).await || !self.plan_active(generation).await {
                        warn!(
                            official_page_rejected = true,
                            "autoplay: input target no longer allowed"
                        );
                        return;
                    }
                    // A lost press usually costs nothing — the decision
                    // window closes and the client passes. A lost *riichi*
                    // press is different: the discard that follows it still
                    // goes out, so the hand throws its riichi tile with no
                    // riichi behind it. That press gets the sturdier shape
                    // from the first attempt rather than only on a retry.
                    if let Err(e) = dispatch_click_shaped(
                        page,
                        px,
                        py,
                        cfg.hover_delay_ms,
                        cfg.click_hold_ms,
                        declares_reach,
                    )
                    .await
                    {
                        warn!("autoplay: dispatch_click failed: {e:#}");
                        return;
                    }
                    drop(page_guard);
                }
            }
        }

        if let (true, Some(rect)) = (cfg.verify_input_ms > 0, rect) {
            // What counts as proof. A discard plan is proven by any input.
            // Every other plan pressed action buttons, and for those a
            // plain discard can only be the client's own turn-timeout
            // tsumogiri — it arrives with a human-scale `timeuse` and no
            // `auto_operation` flag, so the wire-level filter cannot drop
            // it, and accepting it would report retries as registered at
            // the exact moments the presses had failed.
            let require_non_discard = !matches!(resp.action, MjaiEvent::Dahai { .. });
            let registered = self
                .verify_and_retry(
                    rect,
                    &plan,
                    ticket,
                    &cfg,
                    planned_budget,
                    &resp.action,
                    require_non_discard,
                    generation,
                )
                .await;
            // Repeated unregistered input pauses actions; never reload a game.
            if registered {
                self.state.dead_clicks = 0;
            } else {
                self.state.dead_clicks += 1;
                if cfg.reload_after_failures > 0
                    && self.state.dead_clicks >= cfg.reload_after_failures
                {
                    warn!(
                        decisions = self.state.dead_clicks,
                        "autoplay: the client has stopped accepting presses — pausing autoplay"
                    );
                    let _ = self.notify.send(
                        crate::schema::Notification::warn("Autoplay input unavailable").body(
                            "Clicks stopped registering; automatic actions have been paused.",
                        ),
                    );
                    self.state.dead_clicks = 0;
                    self.pause_autoplay().await;
                }
            }
            // A riichi plan that produced an input command has not
            // necessarily declared anything: if the button press was lost
            // and only the tile press landed, the client sends a plain
            // discard, which the presence check happily accepts. The wire
            // tells them apart, so check, and say so — the tile is gone
            // either way, but the player is now in a hand they did not
            // choose and nothing else would tell them. It is also the one
            // window where the bot's own view can drift: if the declaring
            // tile came from an autoplay reach follow-up (#257), the bot was
            // fed a reach that then did not happen, so flag that too.
            if declares_reach
                && self.ctx.input_watch.sent_since(ticket)
                && !self.ctx.input_watch.reach_since(ticket)
            {
                warn!(
                    "autoplay: riichi was planned for {:?} but the client sent a plain discard — the declaration press was lost and the tile went out without it",
                    resp.action
                );
                let _ = self.notify.send(
                    crate::schema::Notification::error("Riichi did not go through").body(
                        "The declaration press was lost and the tile was discarded without it. This hand is not in riichi — and the bot may still believe it declared, so its reads for the rest of this hand can be off.",
                    ),
                );
            }
        }
    }

    /// Wait for the client's input command; press again if it never came.
    ///
    /// Retrying is bounded and gated on the decision window still being the
    /// one the plan was made for: if our action did land and the answer was
    /// merely slow, the server's echo closes that window and the retry is
    /// dropped rather than pressed into the next decision.
    async fn await_turn_ready(page: &Page, timeout_ms: u32) -> bool {
        const POLL_INTERVAL: Duration = Duration::from_millis(120);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let started = Instant::now();
        loop {
            match crate::autoplay::cdp_input::turn_clock_running(page).await {
                Ok(true) => {
                    debug!(
                        "autoplay: client ready after {}ms",
                        started.elapsed().as_millis()
                    );
                    return true;
                }
                Ok(false) => {}
                Err(e) => {
                    warn!("autoplay: readiness probe failed: {e:#}");
                    return false;
                }
            }
            if Instant::now() >= deadline {
                warn!(
                    "autoplay: client never started its clock within {timeout_ms}ms; \
                     skipping this decision"
                );
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn press_when_offered(
        &self,
        page: &Page,
        selectors: &[String],
        label: &str,
        generation: u64,
        window: Option<crate::autoplay::tenhou_state::DecisionWindow>,
    ) -> bool {
        const POLL_INTERVAL: Duration = Duration::from_millis(120);
        const WAIT_BUDGET: Duration = Duration::from_millis(2_400);

        let deadline = Instant::now() + WAIT_BUDGET;
        let mut attempts = 0u32;
        loop {
            if !self.ctx.page_allowed(page).await
                || !self.plan_active(generation).await
                || self.tenhou_window_moved(window)
            {
                return false;
            }
            attempts += 1;
            match crate::autoplay::cdp_input::click_dom(page, selectors).await {
                Ok(true) => {
                    info!("autoplay: pressed {label} ({selectors:?}) after {attempts} look(s)");
                    return true;
                }
                Ok(false) => {}
                Err(e) => {
                    warn!("autoplay: pressing {label} failed: {e:#}");
                    return false;
                }
            }
            if Instant::now() >= deadline {
                let offered = crate::autoplay::cdp_input::list_action_buttons(page).await;
                warn!(
                    "autoplay: {label} button ({selectors:?}) never appeared in {:?}; \
                     client is offering slots {offered:?} (its own order: highest first)",
                    WAIT_BUDGET
                );
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn tenhou_window_moved(
        &self,
        planned: Option<crate::autoplay::tenhou_state::DecisionWindow>,
    ) -> bool {
        let Some(planned) = planned else {
            return false;
        };
        let current = self
            .ctx
            .tenhou_state
            .read()
            .ok()
            .and_then(|g| g.as_ref().and_then(|s| s.window));
        current.map(|w| w.opened_at) != Some(planned.opened_at)
    }

    #[allow(clippy::too_many_arguments)]
    async fn verify_and_retry(
        &self,
        rect: CanvasRect,
        plan: &crate::autoplay::PlanResult,
        ticket: InputTicket,
        cfg: &crate::config::MajsoulAutoplayConfig,
        planned_budget: Option<crate::autoplay::budget::TimeBudget>,
        action: &MjaiEvent,
        require_non_discard: bool,
        generation: u64,
    ) -> bool {
        let clicks: Vec<(f64, f64)> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Click { x_norm, y_norm } => Some((*x_norm, *y_norm)),
                _ => None,
            })
            .collect();
        if clicks.is_empty() {
            return true;
        }

        for attempt in 0..=cfg.click_retries {
            if self
                .wait_for_input(ticket, cfg.verify_input_ms, require_non_discard)
                .await
            {
                if attempt > 0 {
                    info!("autoplay: retry {attempt} registered for {action:?}");
                }
                return true;
            }
            if attempt == cfg.click_retries {
                break;
            }
            if !self.plan_active(generation).await || !self.window_still_open(planned_budget) {
                debug!(
                    "autoplay: no input seen for {action:?}, but the decision window has moved on — not retrying"
                );
                return true;
            }
            let again = retry_slice(&clicks, attempt);
            // The coordinates are in the message on purpose: when a button
            // visibly reacts and the action still does not happen, this
            // line is what says which slot was aimed at.
            warn!(
                "autoplay: no input command after clicking {action:?} at {clicks:?} (hover {}ms, hold {}ms); pressing {} of {} click(s) again (attempt {})",
                cfg.hover_delay_ms,
                cfg.click_hold_ms,
                again.len(),
                clicks.len(),
                attempt + 1
            );
            // Retries vary the press rather than repeating it verbatim.
            // A press that lands on the right control and still does not
            // commit is not a positioning problem, so pressing the same way
            // again has nothing new to offer: the second attempt holds
            // longer, the third also nudges the cursor mid-press.
            let hold = retry_hold_ms(cfg.click_hold_ms, attempt);
            let jiggle = attempt >= 1;
            for (i, (x_norm, y_norm)) in again.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(u64::from(cfg.inter_click_delay_ms)))
                        .await;
                }
                let (px, py) = rect.pixel(*x_norm, *y_norm);
                if !rect.contains(px, py) {
                    warn!(
                        "autoplay: retry click ({px},{py}) outside canvas rect {:?}; skipping",
                        rect
                    );
                    continue;
                }
                let page_guard = self.ctx.page.read().await;
                let Some(page) = page_guard.as_ref() else {
                    warn!("autoplay: no page handle — abandoning retry for {action:?}");
                    return false;
                };
                if !self.ctx.page_allowed(page).await || !self.plan_active(generation).await {
                    warn!(
                        official_page_rejected = true,
                        "autoplay: retry target no longer allowed"
                    );
                    return false;
                }
                if let Err(e) =
                    dispatch_click_shaped(page, px, py, cfg.hover_delay_ms, hold, jiggle).await
                {
                    warn!("autoplay: retry dispatch_click failed: {e:#}");
                }
            }
        }
        // The timings are in the message because a config written before a
        // default changed keeps its stored values, and "the new default
        // never reached me" is otherwise indistinguishable from "the click
        // is landing in the wrong place".
        warn!(
            "autoplay: {action:?} never produced an input command after {} attempt(s) at {clicks:?} (hover {}ms, hold {}ms); if the button visibly reacts to these coordinates the press is being ignored — try longer timings under Settings -> Autoplay",
            cfg.click_retries + 1,
            cfg.hover_delay_ms,
            cfg.click_hold_ms
        );
        false
    }

    /// Poll for an input command until `timeout_ms` runs out. Polling (not
    /// one flat sleep) so a click that worked returns almost immediately and
    /// only a lost one pays the full wait. With `require_non_discard`, a
    /// plain discard does not satisfy the wait — the proof standard for a
    /// plan whose clicks were all action buttons.
    async fn wait_for_input(
        &self,
        ticket: InputTicket,
        timeout_ms: u32,
        require_non_discard: bool,
    ) -> bool {
        const POLL: Duration = Duration::from_millis(20);
        let deadline = std::time::Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let seen = if require_non_discard {
                self.ctx.input_watch.non_discard_since(ticket)
            } else {
                self.ctx.input_watch.sent_since(ticket)
            };
            if seen {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Whether the decision window the plan was made against is still the
    /// live one. With no budget tracked there is no signal, and the answer
    /// is "yes" — behaviour off-Majsoul is unchanged.
    fn window_still_open(&self, planned: Option<crate::autoplay::budget::TimeBudget>) -> bool {
        let Some(planned) = planned else {
            return true;
        };
        let current = self.ctx.time_budget.read().ok().and_then(|g| *g);
        current.map(|b| b.opened_at) == Some(planned.opened_at)
    }

    async fn plan_active(&self, generation: u64) -> bool {
        self.ctx
            .generation
            .load(std::sync::atomic::Ordering::SeqCst)
            == generation
            && self
                .ctx
                .autoplay_enabled
                .load(std::sync::atomic::Ordering::SeqCst)
            && self.cfg.read().await.autoplay.enabled
    }

    /// Pause after input failures without navigating or refreshing the client.
    async fn pause_autoplay(&mut self) {
        self.cfg.write().await.autoplay.enabled = false;
        self.ctx.set_enabled(false);
        let _ = self.notify.send(crate::schema::Notification::warn("Autoplay paused").body("Input stopped registering. Check the game when safe; Akagi will not refresh it.").sticky());
    }
    fn handle_mjai_event(&mut self, ev: &MjaiEvent) {
        match ev {
            MjaiEvent::StartGame { id, .. } => {
                // Capture our seat directly from the StartGame event rather
                // than going through the tracker. This avoids the try_lock
                // race entirely and makes cached_our_seat available from
                // the very first event of the game.
                let seat = *id;
                self.state = ManagerState::default();
                self.state.cached_our_seat = seat;
            }
            MjaiEvent::EndGame { .. } => {
                self.state = ManagerState::default();
            }
            MjaiEvent::StartKyoku { .. } | MjaiEvent::EndKyoku => {
                // Per-kyoku reset: keep last seen rect cache and cached seat,
                // drop everything else. Keep last_kawa_tile as None so
                // push_random_pre_delay uses the max delay (opening-hand guard).
                let canvas_at = self.state.canvas_rect_at;
                let cached_seat = self.state.cached_our_seat;
                // The dead-click count survives too: a client that has
                // stopped accepting presses stays that way across the kyoku
                // boundary, and zeroing it here would need the failures to
                // land inside one hand before anything recovered them.
                let dead_clicks = self.state.dead_clicks;
                self.state = ManagerState::default();
                self.state.canvas_rect_at = canvas_at;
                self.state.cached_our_seat = cached_seat;
                self.state.dead_clicks = dead_clicks;
            }
            MjaiEvent::Tsumo { actor, pai } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = Some(pai.clone());
                    }
                }
            }
            MjaiEvent::Dahai { actor, pai, .. } => {
                self.state.last_kawa_tile = Some(pai.clone());
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = None;
                    }
                }
            }
            MjaiEvent::ReachAccepted { actor } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.self_riichi_accepted = true;
                    }
                }
            }
            MjaiEvent::Chi { actor, .. }
            | MjaiEvent::Pon { actor, .. }
            | MjaiEvent::Daiminkan { actor, .. }
            | MjaiEvent::Ankan { actor, .. }
            | MjaiEvent::Kakan { actor, .. } => {
                if let Some(seat) = self.our_seat_cached() {
                    if *actor == seat {
                        self.state.last_self_tsumo = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Best-effort seat lookup. Uses the cached seat from `StartGame` first,
    /// falling back to try_lock on the tracker.
    fn our_seat_cached(&self) -> Option<u8> {
        self.state
            .cached_our_seat
            .or_else(|| self.tracker.try_lock().ok().and_then(|t| t.our_seat()))
    }

    async fn canvas_rect_resolve(&mut self) -> Option<CanvasRect> {
        let now = Instant::now();
        if let Some(at) = self.state.canvas_rect_at {
            if now.duration_since(at) < CANVAS_RECT_TTL {
                if let Some(r) = *self.ctx.canvas_rect.read().await {
                    return Some(r);
                }
            }
        }
        // Re-query.
        let page_guard = self.ctx.page.read().await;
        let page = page_guard.as_ref()?.clone();
        drop(page_guard);
        if !self.ctx.page_allowed(&page).await {
            return None;
        }
        match evaluate_canvas_rect(&page).await {
            Ok(rect) => {
                *self.ctx.canvas_rect.write().await = Some(rect);
                self.state.canvas_rect_at = Some(now);
                Some(rect)
            }
            Err(e) => {
                debug!("autoplay: evaluate_canvas_rect failed: {e:#}");
                None
            }
        }
    }
}

/// Which clicks to press again on retry `attempt` (0-based).
///
/// In a multi-click plan — a chi/pon/kan whose candidate row needs
/// disambiguating, or a Path-A riichi — the *last* click is both the one
/// that commits the action and the one most likely to have been swallowed:
/// it fires `inter_click_delay_ms` after the previous press, while the
/// candidate row is still animating in, whereas the opening click follows
/// the full thinking delay against a settled UI.
///
/// So the first retry presses only that last click. If the row never opened
/// because the *opening* click was the one that missed, it lands on empty
/// table and does nothing — a harmless way to be wrong, unlike re-pressing
/// a chi button whose row is already open, whose effect we cannot predict.
/// A second retry then replays the whole sequence on the theory that the
/// opening click was the one lost.
fn retry_slice(clicks: &[(f64, f64)], attempt: u32) -> &[(f64, f64)] {
    if attempt == 0 && clicks.len() > 1 {
        &clicks[clicks.len() - 1..]
    } else {
        clicks
    }
}

/// Hold duration for retry press number `attempt` (0-based). The original
/// press already ran at `base`, so the first retry — the second press
/// overall — is the one that must hold twice as long; `attempt + 1` would
/// have repeated the original hold verbatim and wasted the attempt.
/// Capped so a user-configured long hold cannot escalate past 2 s.
fn retry_hold_ms(base: u32, attempt: u32) -> u32 {
    base.saturating_mul(attempt + 2).min(2_000)
}

/// Spawn point for the autoplay loop. Wired by `crate::lib::run` so the
/// `tauri::async_runtime` Tokio runtime is the host.
pub async fn run_autoplay_manager(
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    tracker: Arc<Mutex<GameTracker>>,
    mjai_bus: MjaiBus,
    response_bus: BotResponseBus,
    notify: NotifyBus,
    config_dir: std::path::PathBuf,
) -> anyhow::Result<()> {
    AutoplayManager::new(cfg, ctx, tracker, mjai_bus, notify, config_dir)
        .run(response_bus)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::context::AutoplayContext;
    use crate::event_bus::mjai_bus;
    use crate::game_state::tracker;

    /// Build a minimal `AutoplayManager` suitable for unit-testing
    /// `handle_mjai_event`. No CDP page, no config — just enough to
    /// exercise the mjai event handler without touching async resources.
    fn make_manager() -> AutoplayManager {
        let bus = mjai_bus();
        let tracker = tracker::new_handle();
        AutoplayManager::new(
            Arc::new(RwLock::new(AppConfig::default())),
            Arc::new(AutoplayContext::default()),
            tracker,
            bus,
            crate::event_bus::notify_bus(),
            std::env::temp_dir(),
        )
    }

    #[tokio::test]
    async fn disable_and_reenable_cancel_delayed_action_and_pause_updates_state() {
        let mut m = make_manager();
        m.cfg.write().await.autoplay.enabled = true;
        m.ctx.set_enabled(true);
        let generation = m.ctx.generation.load(std::sync::atomic::Ordering::SeqCst);
        assert!(m.plan_active(generation).await);
        let mut status = m.ctx.enabled_changes.subscribe();
        m.ctx.set_enabled(false);
        m.ctx.set_enabled(true);
        assert!(!m.plan_active(generation).await);
        let current = m.ctx.generation.load(std::sync::atomic::Ordering::SeqCst);
        assert!(m.plan_active(current).await);
        m.pause_autoplay().await;
        status.changed().await.unwrap();
        assert!(!*status.borrow_and_update());
        assert!(!m.cfg.read().await.autoplay.enabled);
        assert!(!m.plan_active(current).await);
    }

    /// Regression: `cached_our_seat` must be populated immediately when
    /// `StartGame` is received, before any bot response fires. Previously
    /// the seat was only cached inside `handle_bot_response`, so the first
    /// `Tsumo` event on the opening draw could arrive before the bot had
    /// responded and `last_self_tsumo` would be silently missed.
    #[test]
    fn start_game_sets_cached_seat_immediately() {
        let mut m = make_manager();

        // Before any event — no seat cached.
        assert!(m.state.cached_our_seat.is_none());

        // StartGame with id = Some(1) — seat must be cached right away.
        m.handle_mjai_event(&MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(1),
            num_players: 4,
            game_meta: None,
        });
        assert_eq!(
            m.state.cached_our_seat,
            Some(1),
            "seat must be cached from StartGame"
        );

        // A Tsumo by our seat (1) before any bot response should be recorded.
        m.handle_mjai_event(&MjaiEvent::Tsumo {
            actor: 1,
            pai: "3m".into(),
        });
        assert_eq!(
            m.state.last_self_tsumo.as_deref(),
            Some("3m"),
            "last_self_tsumo must be recorded even before first bot response"
        );
    }

    /// Seat is preserved across `StartKyoku` and `EndKyoku` resets so
    /// tsumo tracking continues to work from the first draw of each round.
    #[test]
    fn cached_seat_survives_kyoku_reset() {
        let mut m = make_manager();

        m.handle_mjai_event(&MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(2),
            num_players: 4,
            game_meta: None,
        });
        assert_eq!(m.state.cached_our_seat, Some(2));

        m.handle_mjai_event(&MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "1m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25_000; 4],
            tehais: vec![vec!["1m".into(); 13]; 4],
            num_players: 4,
        });
        assert_eq!(
            m.state.cached_our_seat,
            Some(2),
            "seat must survive StartKyoku reset"
        );

        m.handle_mjai_event(&MjaiEvent::EndKyoku);
        assert_eq!(
            m.state.cached_our_seat,
            Some(2),
            "seat must survive EndKyoku reset"
        );
    }

    /// A client that has stopped accepting presses stays that way across
    /// a kyoku boundary — the failures run to the end of the game — so the
    /// count has to survive one, or the threshold would only ever be
    /// reached by failures packed into a single hand. It does reset for a
    /// new game, which is a new page state.
    #[test]
    fn dead_click_count_survives_a_kyoku_but_not_a_game() {
        let mut m = make_manager();
        m.state.dead_clicks = 2;
        m.handle_mjai_event(&MjaiEvent::EndKyoku);
        assert_eq!(m.state.dead_clicks, 2, "the client is still not listening");

        m.handle_mjai_event(&MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
            game_meta: None,
        });
        assert_eq!(m.state.dead_clicks, 0, "a new table is a fresh start");
    }

    /// A single-click plan (most of them: discard, pon, ron, skip) has
    /// nothing to escalate — every attempt presses the same one click.
    #[test]
    fn a_single_click_plan_always_retries_that_click() {
        let clicks = [(1.0, 2.0)];
        assert_eq!(retry_slice(&clicks, 0), &clicks[..]);
        assert_eq!(retry_slice(&clicks, 1), &clicks[..]);
    }

    /// A multi-click plan escalates: the committing click first (the one
    /// pressed while the candidate row was still animating), then the whole
    /// sequence. Re-pressing a chi button whose row is already open has an
    /// effect we cannot predict, so it is not the first thing tried.
    #[test]
    fn a_multi_click_plan_retries_the_committing_click_first() {
        let clicks = [(1.0, 2.0), (3.0, 4.0)];
        assert_eq!(
            retry_slice(&clicks, 0),
            &[(3.0, 4.0)],
            "first retry presses only the candidate/tile click"
        );
        assert_eq!(
            retry_slice(&clicks, 1),
            &clicks[..],
            "second retry assumes the opening click was lost too"
        );
    }

    /// Defensive: an empty plan is never verified, but the slice helper
    /// must not panic if it ever is.
    #[test]
    fn no_clicks_is_not_a_panic() {
        let clicks: [(f64, f64); 0] = [];
        assert!(retry_slice(&clicks, 0).is_empty());
        assert!(retry_slice(&clicks, 3).is_empty());
    }

    /// The first retry (attempt 0) is the *second* press overall and must
    /// already escalate the hold — `attempt + 1` would repeat the original
    /// hold verbatim and waste the attempt.
    #[test]
    fn retry_press_escalates_the_hold_from_the_first_retry() {
        assert_eq!(retry_hold_ms(120, 0), 240, "second press holds twice");
        assert_eq!(retry_hold_ms(120, 1), 360);
        assert_eq!(retry_hold_ms(1_500, 1), 2_000, "capped at 2s");
        assert_eq!(retry_hold_ms(u32::MAX, 5), 2_000, "no overflow");
    }

    /// Observer/replay mode: `StartGame` with `id: None` must not cache a
    /// stale seat from a previous game.
    #[test]
    fn start_game_without_id_clears_cached_seat() {
        let mut m = make_manager();

        m.handle_mjai_event(&MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
            game_meta: None,
        });
        assert_eq!(m.state.cached_our_seat, Some(0));

        // New game, observer mode — no seat.
        m.handle_mjai_event(&MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: None,
            num_players: 4,
            game_meta: None,
        });
        assert!(
            m.state.cached_our_seat.is_none(),
            "stale seat must be cleared"
        );
    }
}

fn discard_needs_window_guard(action: &MjaiEvent) -> bool {
    !matches!(action, MjaiEvent::Reach { .. })
}
