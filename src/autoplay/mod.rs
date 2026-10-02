pub mod budget;
pub mod cdp_input;
pub mod context;
pub mod delay;
pub mod majsoul;
pub mod manager;
pub mod platform;
pub mod verify;

pub use budget::{BudgetSource, SharedTimeBudget, TimeBudget};
pub use context::{AutoplayContext, CanvasRect};
pub use manager::run_autoplay_manager;
pub use platform::{ActionContext, PlanResult, PlatformAutoplay, Step};
pub use verify::{InputKind, InputWatch, SharedInputWatch};
