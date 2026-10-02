//! 将建议转换为雀魂画布点击和等待步骤。计划计算本身不执行输入。

use crate::autoplay::delay::{BudgetSnapshot, DecisionProbs};
use crate::config::{DelayModelConfig, MajsoulAutoplayConfig};
use crate::game_state::snapshot::GameStateSnapshot;
use crate::schema::MjaiEvent;
use riichienv_core::action::Action;

/// One step in the click sequence the manager will execute.
///
/// The 16:9-normalised coordinates match the convention used by the
/// original Akagi Python autoplay `LOCATION` table.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Click at a normalised 16:9 point on the game canvas.
    Click { x_norm: f64, y_norm: f64 },
    /// Pause for `duration_ms` before the next step. Used for the
    /// pre-click "thinking" delay and the inter-click gap inside one
    /// action.
    Sleep { duration_ms: u32 },
}

/// Everything the platform impl needs to translate one bot decision
/// into a concrete click sequence.
pub struct ActionContext<'a> {
    /// The bot's chosen action (from `BotResponseBus`).
    pub action: &'a MjaiEvent,
    /// Live game state from the riichi engine.
    pub snapshot: &'a GameStateSnapshot,
    /// Currently legal actions for `our_seat`, sourced from the riichi
    /// engine's `_get_legal_actions_internal`. The platform impl uses
    /// this to:
    /// - decide which action button (chi/pon/kan/...) is in which
    ///   on-screen position, by intersecting with the platform's
    ///   priority table;
    /// - enumerate chi/pon/kan candidate combinations when the bot's
    ///   action is ambiguous (multiple `consume_tiles`).
    pub legal_actions: &'a [Action],
    /// Bot's seat.
    pub our_seat: u8,
    /// The most recent tile any seat discarded — needed to disambiguate
    /// chi/pon target.
    pub last_kawa_tile: Option<&'a str>,
    /// The tile we drew this turn, if any. Used to detect tsumohai
    /// position when emitting `dahai`.
    pub last_self_tsumo: Option<&'a str>,
    /// True from the moment the server confirms our riichi until the
    /// kyoku ends. While set, dahai clicks are suppressed (Majsoul auto-
    /// discards in riichi mode).
    pub self_riichi_accepted: bool,
    /// 3 (sanma) or 4 (yonma).
    pub num_players: u8,
    /// Per-platform config knobs (delays, mouse-move emission, ...).
    pub cfg: &'a MajsoulAutoplayConfig,
    /// Delay-model parameters (see `autoplay::delay`). Owned: it is a
    /// small parameter block cloned per bot response, which keeps test
    /// construction free of an extra borrow.
    pub delay_cfg: DelayModelConfig,
    /// Server time budget for the current decision window, if known.
    /// `None` off-Majsoul or before the first operation list arrives.
    pub budget: Option<BudgetSnapshot>,
    /// Normalized bot confidence for this decision, if the bot's meta
    /// could be interpreted (see `autoplay::delay::probs`).
    pub probs: Option<DecisionProbs>,
    /// User Lua delay policy, when loaded. Consulted by the delay model;
    /// on any script failure the built-in policy runs instead.
    pub delay_script: Option<&'a crate::autoplay::delay::DelayScript>,
}

/// Output of `PlatformAutoplay::plan`: the click sequence to execute.
///
/// The riichi declaring discard is always resolved before the plan is
/// built — the bot fills `Reach.pai` (natively or via the manager's
/// autoplay reach follow-up, see `bot::manager` and #257) — so both
/// platforms declare and discard in a single plan; there is no
/// bus-injection follow-up path.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanResult {
    pub steps: Vec<Step>,
}

pub trait PlatformAutoplay: Send + Sync {
    /// Translate the bot's action into a click sequence + side-effect
    /// hints. Pure: must not perform IO. The manager handles the actual
    /// CDP dispatch and bus injection.
    fn plan(&self, ctx: &ActionContext) -> PlanResult;
}
