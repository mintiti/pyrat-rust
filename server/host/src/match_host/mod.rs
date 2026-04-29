//! Linear match lifecycle: `Match<S>` drives two [`Player`](crate::player::Player)
//! handles through Created → Ready → Playing → Finished.
//!
//! Each phase is a distinct type. Transitions consume `self` and return the
//! next phase, so calling `step()` before `setup()` is a compile error. The
//! Match owns the engine state, both players, the per-player option
//! assignments, and the event sink — every protocol message flows through
//! one place. Sideband (Info, Provisional, RenderCommands) is forwarded to
//! the [`EventSink`](crate::player::EventSink) by each Player; the Match
//! itself never inspects it.
//!
//! Slice 5 covers the linear lifecycle only. Analysis sub-states
//! (`Thinking`, `Collected`) for GUI step-mode are added in slice 7.

mod config;
mod error;
mod events;
mod phases;
mod policy;
mod result;

pub use config::{PlayingConfig, SetupTiming};
pub use error::MatchError;
pub(crate) use events::emit;
pub use events::MatchEvent;
pub use phases::{Created, Finished, Match, Playing, Ready, StepResult};
pub use policy::{ActionOutcome, DefaultFaultPolicy, FaultPolicy, StrictFaultPolicy};
pub use result::MatchResult;
