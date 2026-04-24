//! Bot-facing context: the interface a bot sees during `think` and
//! `preprocess`.

use std::sync::Arc;

use pyrat::Direction;
use pyrat_protocol::OwnedInfo;
use pyrat_wire::Player;

use crate::InfoParams;

/// Per-turn interface the bot uses to query deadlines, emit sideband
/// (Info / Provisional), and cooperate with host-initiated stops.
///
/// Implemented by `pyrat_sdk::Context` (networked) and
/// `pyrat_host::player::EmbeddedCtx` (in-process) so a single bot
/// implementation works unchanged across both transports.
pub trait BotContext {
    /// Player slot this bot is controlling.
    fn player(&self) -> Player;

    /// Current turn number.
    fn turn(&self) -> u16;

    /// Server-canonical hash of the current turn state.
    fn state_hash(&self) -> u64;

    /// True when the bot should stop thinking: deadline passed, host
    /// sent Stop, or the game ended.
    fn should_stop(&self) -> bool;

    /// Milliseconds remaining before the deadline. Returns 0 if the
    /// bot has been told to stop.
    fn time_remaining_ms(&self) -> u64;

    /// Milliseconds elapsed since `think` started.
    fn think_elapsed_ms(&self) -> u32;

    /// Emit an observer-facing Info frame.
    fn send_info(&self, params: &InfoParams<'_>);

    /// Emit a provisional best move. The host uses the latest
    /// provisional as a timeout fallback.
    fn send_provisional(&self, direction: Direction);

    /// Cloneable handle for sending Info frames from worker threads
    /// (e.g., MCTS search threads). `None` if the underlying transport
    /// has no room for sideband.
    fn info_sender(&self) -> Option<InfoSender>;
}

/// Cloneable Info emitter. Multi-threaded bots obtain one of these from
/// [`BotContext::info_sender`] and hand clones to worker threads; each
/// call passes the worker's view of `turn` and `state_hash`.
#[derive(Clone)]
pub struct InfoSender {
    sink: Arc<dyn InfoSink>,
}

impl InfoSender {
    /// Construct from a concrete sink. Transport crates (SDK, host) use
    /// this to hand out pre-wrapped senders.
    pub fn new(sink: Arc<dyn InfoSink>) -> Self {
        Self { sink }
    }

    /// Build an `OwnedInfo` from `params` and the worker-supplied
    /// `turn` / `state_hash`, then forward to the underlying sink.
    pub fn send_info(&self, params: &InfoParams<'_>, turn: u16, state_hash: u64) {
        let info = OwnedInfo {
            player: params.player,
            multipv: params.multipv,
            target: params.target,
            depth: params.depth,
            nodes: params.nodes,
            score: params.score,
            pv: params.pv.to_vec(),
            message: params.message.to_string(),
            turn,
            state_hash,
        };
        self.sink.send_info(info);
    }
}

/// Transport-specific Info emitter. SDK wraps a frame-writer channel;
/// host wraps an `EventSink` + per-match metadata.
pub trait InfoSink: Send + Sync {
    fn send_info(&self, info: OwnedInfo);
}
