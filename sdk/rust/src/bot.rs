//! Bot and Hivemind traits, Context for timing and info sending.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pyrat::Direction;
use pyrat_wire::{GameResult, Player};

use crate::options::Options;
use crate::state::GameState;

/// Synchronous writer for Info frames during `think()`.
///
/// Wraps a `TcpStream` behind `Arc<Mutex<…>>` so it is `Send + Sync` and
/// cheaply cloneable. Multi-threaded bots (e.g. MCTS) can clone the sender
/// and move it into worker threads.
#[derive(Clone)]
pub struct InfoSender {
    stream: Arc<Mutex<std::net::TcpStream>>,
}

impl InfoSender {
    pub(crate) fn new(stream: std::net::TcpStream) -> Self {
        Self {
            stream: Arc::new(Mutex::new(stream)),
        }
    }

    /// Send a pre-built Info frame. Locks the stream internally.
    pub fn send(&self, frame: &[u8]) {
        let Ok(mut stream) = self.stream.lock() else {
            eprintln!("[sdk] send_info() failed: mutex poisoned");
            return;
        };
        let len = (frame.len() as u32).to_be_bytes();
        if let Err(e) = stream
            .write_all(&len)
            .and_then(|()| stream.write_all(frame))
        {
            eprintln!("[sdk] send_info() failed: {e}");
        }
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
    info_sender: Mutex<Option<InfoSender>>,
}

impl Context {
    /// Create a context with a deadline.
    pub(crate) fn new(deadline: Instant, info_sender: Option<InfoSender>) -> Self {
        Self {
            deadline,
            info_sender: Mutex::new(info_sender),
        }
    }

    /// Whether the deadline has passed.
    pub fn should_stop(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Milliseconds remaining before the deadline. Returns 0 if past.
    pub fn time_remaining_ms(&self) -> u64 {
        self.deadline
            .checked_duration_since(Instant::now())
            .map_or(0, |d| d.as_millis() as u64)
    }

    /// Clone the inner [`InfoSender`], if available.
    ///
    /// Use this to get an owned sender you can move into `std::thread::spawn`.
    /// Returns `None` when no sender is available (e.g. during preprocess).
    pub fn info_sender(&self) -> Option<InfoSender> {
        self.info_sender.lock().unwrap().clone()
    }

    /// Reclaim the `InfoSender` so it can be reused across turns.
    pub(crate) fn take_info_sender(&self) -> Option<InfoSender> {
        self.info_sender.lock().unwrap().take()
    }

    /// Send an Info message to the host (for GUI / debugging).
    ///
    /// Writes synchronously on a cloned TCP socket. Errors are logged to stderr.
    #[allow(clippy::too_many_arguments)]
    pub fn send_info(
        &self,
        player: Player,
        multipv: u16,
        target: Option<(u8, u8)>,
        depth: u16,
        nodes: u32,
        score: f32,
        pv: &[Direction],
        message: &str,
    ) {
        if let Some(sender) = self.info_sender.lock().unwrap().as_ref() {
            let frame =
                crate::wire::build_info(player, multipv, target, depth, nodes, score, pv, message);
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
