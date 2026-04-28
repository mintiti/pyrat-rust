//! Bot and Hivemind traits, Context for timing and info sending.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pyrat::Direction;
use pyrat_bot_api::{BotContext, InfoSink};
use pyrat_wire::{GameResult, Player};
use tokio::sync::mpsc;

use crate::options::Options;
use crate::state::GameState;

/// Non-blocking writer for Info frames during `think()`.
///
/// Sends frames through an `mpsc` channel to a background writer task that
/// handles the actual I/O via tokio's async `FrameWriter`. This avoids the
/// `O_NONBLOCK` sharing bug that occurred with a cloned `TcpStream`.
///
/// Cheaply cloneable — multi-threaded bots (e.g. MCTS) can clone the sender
/// and move it into worker threads.
#[derive(Clone)]
pub struct InfoSender {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl InfoSender {
    pub(crate) fn new(tx: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }

    /// Send a pre-built frame through the writer channel.
    pub(crate) fn send(&self, frame: &[u8]) {
        if let Err(e) = self.tx.send(frame.to_vec()) {
            eprintln!("[sdk] send() failed: channel closed ({e})");
        }
    }

    /// Build and send an Info message from [`InfoParams`].
    pub fn send_info(&self, params: &InfoParams, turn: u16, state_hash: u64) {
        let frame = crate::wire::serialize(&pyrat_protocol::BotMsg::Info(info_from_params(
            params, turn, state_hash,
        )));
        self.send(&frame);
    }
}

pub use pyrat_bot_api::InfoParams;

impl InfoSink for InfoSender {
    fn send_info(&self, params: &InfoParams<'_>, turn: u16, state_hash: u64) {
        let frame = crate::wire::serialize(&pyrat_protocol::BotMsg::Info(info_from_params(
            params, turn, state_hash,
        )));
        self.send(&frame);
    }
}

fn info_from_params(params: &InfoParams<'_>, turn: u16, state_hash: u64) -> pyrat_protocol::Info {
    pyrat_protocol::Info {
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
    }
}

impl BotContext for Context {
    fn player(&self) -> Player {
        self.player
    }

    fn turn(&self) -> u16 {
        self.turn
    }

    fn state_hash(&self) -> u64 {
        self.state_hash
    }

    fn should_stop(&self) -> bool {
        Context::should_stop(self)
    }

    fn time_remaining_ms(&self) -> u64 {
        Context::time_remaining_ms(self)
    }

    fn think_elapsed_ms(&self) -> u32 {
        Context::think_elapsed_ms(self)
    }

    fn send_info(&self, params: &InfoParams<'_>) {
        Context::send_info(self, params);
    }

    fn send_provisional(&self, direction: Direction) {
        Context::send_provisional(self, direction);
    }

    fn info_sender(&self) -> Option<pyrat_bot_api::InfoSender> {
        self.info_sender
            .lock()
            .unwrap()
            .as_ref()
            .map(|sdk_sender| pyrat_bot_api::InfoSender::new(Arc::new(sdk_sender.clone())))
    }
}

/// Timing context passed to `think()` and `preprocess()`.
///
/// Thread-safe: `Context` is `Sync`, so `&Context` can be shared across threads
/// (e.g. rayon scoped threads, crossbeam scopes). Multi-threaded bots can also
/// call [`info_sender()`](Self::info_sender) to get a cloneable handle for
/// `std::thread::spawn`.
pub struct Context {
    deadline: Instant,
    think_start: Instant,
    player: Player,
    turn: u16,
    state_hash: u64,
    info_sender: Mutex<Option<InfoSender>>,
    stopped: Arc<AtomicBool>,
    game_over: Arc<AtomicBool>,
}

impl Context {
    /// Create a context with a deadline and optional server-stop flag.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        deadline: Instant,
        think_start: Instant,
        player: Player,
        turn: u16,
        state_hash: u64,
        info_sender: Option<InfoSender>,
        stopped: Arc<AtomicBool>,
        game_over: Arc<AtomicBool>,
    ) -> Self {
        Self {
            deadline,
            think_start,
            player,
            turn,
            state_hash,
            info_sender: Mutex::new(info_sender),
            stopped,
            game_over,
        }
    }

    /// Whether the bot should stop thinking (deadline passed **or** server sent Stop/Timeout).
    pub fn should_stop(&self) -> bool {
        Instant::now() >= self.deadline
            || self.stopped.load(Ordering::Relaxed)
            || self.game_over.load(Ordering::Relaxed)
    }

    /// Milliseconds remaining before the deadline. Returns 0 if stopped by the server.
    pub fn time_remaining_ms(&self) -> u64 {
        if self.stopped.load(Ordering::Relaxed) || self.game_over.load(Ordering::Relaxed) {
            return 0;
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .map_or(0, |d| d.as_millis() as u64)
    }

    /// Milliseconds elapsed since think started.
    pub fn think_elapsed_ms(&self) -> u32 {
        self.think_start.elapsed().as_millis() as u32
    }

    /// Clone the inner [`InfoSender`], if available.
    ///
    /// Use this to get an owned sender you can move into `std::thread::spawn`.
    pub fn info_sender(&self) -> Option<InfoSender> {
        self.info_sender.lock().unwrap().clone()
    }

    /// Send an Info message to the host (for GUI / debugging).
    pub fn send_info(&self, params: &InfoParams) {
        if let Some(sender) = self.info_sender.lock().unwrap().as_ref() {
            sender.send_info(params, self.turn, self.state_hash);
        }
    }

    /// Send a provisional (best-so-far) action to the host.
    ///
    /// The host uses the latest provisional as fallback if the committed
    /// action doesn't arrive in time.
    pub fn send_provisional(&self, direction: Direction) {
        if let Some(sender) = self.info_sender.lock().unwrap().as_ref() {
            let frame = crate::wire::serialize(&pyrat_protocol::BotMsg::Provisional {
                direction,
                player: self.player,
                turn: self.turn,
                state_hash: self.state_hash,
            });
            sender.send(&frame);
        }
    }
}

/// Trait for a single-player bot.
///
/// Implement `think()` to return a direction each turn.
/// `preprocess()` and `on_game_over()` are optional.
pub trait Bot: Options {
    /// Choose a direction for this turn.
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction;

    /// Called once before the first turn, with a longer timeout.
    fn preprocess(&mut self, _state: &GameState, _ctx: &Context) {}

    /// Called when the game ends.
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}

/// Trait for a hivemind bot controlling both players.
///
/// Same lifecycle as `Bot`, but returns two actions per turn.
pub trait Hivemind: Options {
    /// Choose directions for both players.
    fn think(&mut self, state: &GameState, ctx: &Context) -> [(Player, Direction); 2];

    /// Called once before the first turn.
    fn preprocess(&mut self, _state: &GameState, _ctx: &Context) {}

    /// Called when the game ends.
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}

// ── Private Runner trait for turn-loop unification ────

/// Unified lifecycle used by the generic `run_async` / `turn_loop`.
/// Not exported — Bot and Hivemind are the public API.
pub(crate) trait Runner: Options {
    type Actions: IntoIterator<Item = (Player, Direction)>;

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context);
    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions;
    fn runner_stay(state: &GameState) -> Self::Actions;
    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32));
}

/// Newtype wrapper so Bot can implement Runner without blanket-impl conflicts.
pub(crate) struct BotRunner<'a, B: Bot>(pub &'a mut B);

impl<B: Bot> Options for BotRunner<'_, B> {
    fn option_defs(&self) -> Vec<crate::options::SdkOptionDef> {
        self.0.option_defs()
    }
    fn apply_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.0.apply_option(name, value)
    }
}

impl<B: Bot> Runner for BotRunner<'_, B> {
    type Actions = [(Player, Direction); 1];

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context) {
        self.0.preprocess(state, ctx);
    }

    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions {
        [(state.my_player(), self.0.think(state, ctx))]
    }

    fn runner_stay(state: &GameState) -> Self::Actions {
        [(state.my_player(), Direction::Stay)]
    }

    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        self.0.on_game_over(result, scores);
    }
}

/// Newtype wrapper so Hivemind can implement Runner without blanket-impl conflicts.
pub(crate) struct HivemindRunner<'a, H: Hivemind>(pub &'a mut H);

impl<H: Hivemind> Options for HivemindRunner<'_, H> {
    fn option_defs(&self) -> Vec<crate::options::SdkOptionDef> {
        self.0.option_defs()
    }
    fn apply_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.0.apply_option(name, value)
    }
}

impl<H: Hivemind> Runner for HivemindRunner<'_, H> {
    type Actions = [(Player, Direction); 2];

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context) {
        self.0.preprocess(state, ctx);
    }

    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions {
        self.0.think(state, ctx)
    }

    fn runner_stay(_state: &GameState) -> Self::Actions {
        [
            (Player::Player1, Direction::Stay),
            (Player::Player2, Direction::Stay),
        ]
    }

    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        self.0.on_game_over(result, scores);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn flag(val: bool) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(val))
    }

    fn test_ctx(
        deadline: Instant,
        stopped: Arc<AtomicBool>,
        game_over: Arc<AtomicBool>,
    ) -> Context {
        Context::new(
            deadline,
            Instant::now(),
            Player::Player1,
            0,
            0,
            None,
            stopped,
            game_over,
        )
    }

    #[test]
    fn should_stop_deadline_only() {
        let ctx = test_ctx(
            Instant::now() - Duration::from_secs(1),
            flag(false),
            flag(false),
        );
        assert!(ctx.should_stop());
    }

    #[test]
    fn should_stop_flag_only() {
        let ctx = test_ctx(
            Instant::now() + Duration::from_secs(10),
            flag(true),
            flag(false),
        );
        assert!(ctx.should_stop());
    }

    #[test]
    fn should_stop_game_over() {
        let ctx = test_ctx(
            Instant::now() + Duration::from_secs(10),
            flag(false),
            flag(true),
        );
        assert!(ctx.should_stop());
    }

    #[test]
    fn should_stop_neither() {
        let ctx = test_ctx(
            Instant::now() + Duration::from_secs(10),
            flag(false),
            flag(false),
        );
        assert!(!ctx.should_stop());
    }

    #[test]
    fn time_remaining_returns_zero_when_stopped() {
        let ctx = test_ctx(
            Instant::now() + Duration::from_secs(10),
            flag(true),
            flag(false),
        );
        assert_eq!(ctx.time_remaining_ms(), 0);
    }

    #[test]
    fn time_remaining_returns_zero_when_game_over() {
        let ctx = test_ctx(
            Instant::now() + Duration::from_secs(10),
            flag(false),
            flag(true),
        );
        assert_eq!(ctx.time_remaining_ms(), 0);
    }
}
