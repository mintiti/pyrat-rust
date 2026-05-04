//! What the orchestrator needs to actually run one match.
//!
//! `Matchup<D>` carries identity (`descriptor`) plus the concrete runtime
//! values to invoke `pyrat_host::match_host::Match::new`. The descriptor
//! is the durable identity passed to sinks; the matchup adds engine inputs
//! (game_config, players, timing). The seed is the descriptor's — accessed
//! via `descriptor.seed()` so engine and sinks can never disagree.

use std::path::PathBuf;
use std::sync::Arc;

use pyrat::game::builder::GameConfig;
use pyrat_host::player::EmbeddedBot;
use pyrat_host::wire::TimingMode;

use crate::descriptor::Descriptor;

/// Wire-level timing knobs handed to bots via `MatchConfig`. Host-side
/// timing/policy (`SetupTiming`, `PlayingConfig`) is composed alongside
/// these at run time.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub mode: TimingMode,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            mode: TimingMode::Wait,
            move_timeout_ms: 3_000,
            preprocessing_timeout_ms: 10_000,
        }
    }
}

/// Factory for an in-process bot. Invoked once per match so each match gets
/// a fresh `&mut self` with no cross-match state leak.
///
/// `Box<dyn EmbeddedBot>` does not itself auto-impl `EmbeddedBot`, so the
/// caller that hands the box to `EmbeddedPlayer::accept<B: EmbeddedBot>`
/// is responsible for adapting it (blanket impl over `Box<T>` or a thin
/// wrapper).
pub type EmbeddedBotFactory = Arc<dyn Fn() -> Box<dyn EmbeddedBot> + Send + Sync>;

/// How a player slot is materialised when the match starts.
#[derive(Clone)]
pub enum PlayerSpec {
    /// Launch a subprocess and accept it on the per-match TCP listener.
    Subprocess {
        agent_id: String,
        command: String,
        working_dir: Option<PathBuf>,
    },
    /// Build an in-process bot via `factory`.
    Embedded {
        agent_id: String,
        factory: EmbeddedBotFactory,
    },
}

impl std::fmt::Debug for PlayerSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Subprocess {
                agent_id,
                command,
                working_dir,
            } => f
                .debug_struct("Subprocess")
                .field("agent_id", agent_id)
                .field("command", command)
                .field("working_dir", working_dir)
                .finish(),
            Self::Embedded { agent_id, .. } => f
                .debug_struct("Embedded")
                .field("agent_id", agent_id)
                .field("factory", &"<closure>")
                .finish(),
        }
    }
}

/// One match unit: identity plus everything `Match::new` needs.
///
/// The seed is read via `descriptor.seed()` — there is no separate
/// `Matchup::seed` field, so engine and forensic sinks can never disagree.
#[derive(Clone)]
pub struct Matchup<D: Descriptor> {
    pub descriptor: D,
    pub game_config: GameConfig,
    pub players: [PlayerSpec; 2],
    pub timing: Timing,
}

impl<D: Descriptor> Matchup<D> {
    /// Seed for engine state construction. Single source of truth.
    pub fn seed(&self) -> u64 {
        self.descriptor.seed()
    }
}

impl<D: Descriptor + std::fmt::Debug> std::fmt::Debug for Matchup<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matchup")
            .field("descriptor", &self.descriptor)
            .field("players", &self.players)
            .field("timing", &self.timing)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use pyrat::Direction;
    use pyrat_bot_api::Options;
    use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
    use pyrat_protocol::HashedTurnState;

    use super::*;

    /// Stateless bot used by the factory test — distinctness is observed
    /// through the shared counter, not through bot fields.
    struct CountingBot;

    impl Options for CountingBot {}

    impl EmbeddedBot for CountingBot {
        fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
            Direction::Stay
        }
    }

    #[test]
    fn embedded_factory_produces_fresh_instances() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        let factory: EmbeddedBotFactory = Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Box::new(CountingBot)
        });

        let _bot_a = factory();
        let _bot_b = factory();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn timing_default_uses_wait_mode() {
        let t = Timing::default();
        assert_eq!(t.mode, TimingMode::Wait);
        assert!(t.move_timeout_ms > 0);
        assert!(t.preprocessing_timeout_ms > 0);
    }
}
