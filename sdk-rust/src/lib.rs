//! PyRat Rust Bot SDK.
//!
//! Implement the [`Bot`] trait and call [`run()`] to connect to the host
//! and play a match.
//!
//! ```rust,no_run
//! use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
//!
//! struct MyBot;
//! impl Options for MyBot {}
//! impl Bot for MyBot {
//!     fn think(&mut self, state: &GameState, _ctx: &Context) -> Direction {
//!         state.effective_moves(None).first().copied().unwrap_or(Direction::Stay)
//!     }
//! }
//!
//! # fn main() {
//! pyrat_sdk::run(MyBot, "MyBot", "Author");
//! # }
//! ```

mod bot;
mod options;
mod state;
mod wire;

// Re-export public API
pub use bot::{Bot, Context, Hivemind};
pub use options::{Options, SdkOptionDef};
pub use pyrat_wire::OptionType;
pub use state::{GameSim, GameState};

// Re-export engine types bots need
pub use pyrat::{Coordinates, Direction, MoveUndo};
pub use pyrat_engine_interface::pathfinding::FullPathResult;

// Re-export wire types bots need
pub use pyrat_wire::{GameResult, Player};

// Re-export derive macro
pub use pyrat_sdk_derive::Options as DeriveOptions;

/// Safety margin subtracted from move timeout to account for communication latency.
const MOVE_SAFETY_MARGIN_MS: u64 = 5;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use pyrat_wire::framing::{FrameReader, FrameWriter};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use wire::{
    build_action, build_identify, build_pong, build_preprocessing_done, build_ready,
    extract_host_msg, HostMsg,
};

/// Run a single-player bot. Blocks until the game ends.
///
/// Reads `PYRAT_HOST_PORT` and `PYRAT_AGENT_ID` from the environment,
/// connects to the host, and runs the full lifecycle.
pub fn run(mut bot: impl Bot, name: &str, author: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(run_async(&mut bot::BotRunner(&mut bot), name, author));
}

/// Run a hivemind bot controlling both players. Blocks until the game ends.
pub fn run_hivemind(mut bot: impl Hivemind, name: &str, author: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(run_async(&mut bot::HivemindRunner(&mut bot), name, author));
}

async fn connect() -> TcpStream {
    let port: u16 = std::env::var("PYRAT_HOST_PORT")
        .expect("PYRAT_HOST_PORT not set")
        .parse()
        .expect("PYRAT_HOST_PORT not a valid port");

    TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("failed to connect to host")
}

fn get_agent_id() -> String {
    std::env::var("PYRAT_AGENT_ID").unwrap_or_default()
}

// ── Bot lifecycle ────────────────────────────────────

async fn run_async(bot: &mut impl bot::Runner, name: &str, author: &str) {
    let stream = connect().await;
    let (read, write) = tokio::io::split(stream);
    let mut reader = FrameReader::with_default_max(read);
    let mut writer = FrameWriter::with_default_max(write);
    let agent_id = get_agent_id();

    let identify = build_identify(name, author, &agent_id, &bot.option_defs());
    send_frame(&mut writer, &identify).await;
    send_frame(&mut writer, &build_ready()).await;

    let mut state = setup_phase(bot, &mut reader).await;
    turn_loop(bot, &mut state, &mut reader, &mut writer).await;
}

// ── Setup phase ──────────────────────────────────────

async fn setup_phase<O: options::Options, R: AsyncRead + Unpin>(
    bot: &mut O,
    reader: &mut FrameReader<R>,
) -> state::GameState {
    let mut game_state: Option<state::GameState> = None;

    loop {
        let frame = match reader.read_frame().await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[sdk] setup read error: {e}");
                std::process::exit(1);
            },
        };

        match extract_host_msg(frame) {
            Ok(HostMsg::SetOption { name, value }) => {
                if let Err(e) = bot.apply_option(&name, &value) {
                    eprintln!("[sdk] warning: SetOption {name}={value}: {e}");
                }
            },
            Ok(HostMsg::MatchConfig(cfg)) => match state::GameState::from_config(&cfg) {
                Ok(s) => game_state = Some(s),
                Err(e) => {
                    eprintln!("[sdk] error building game state: {e}");
                    std::process::exit(1);
                },
            },
            Ok(HostMsg::StartPreprocessing) => {
                break;
            },
            Ok(other) => {
                eprintln!(
                    "[sdk] unexpected message during setup: {}",
                    msg_name(&other)
                );
            },
            Err(e) => {
                eprintln!("[sdk] setup parse error: {e}");
            },
        }
    }

    game_state.expect("MatchConfig never received before StartPreprocessing")
}

// ── Turn loop ────────────────────────────────────────

async fn turn_loop<T: bot::Runner, R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    bot: &mut T,
    state: &mut state::GameState,
    reader: &mut FrameReader<R>,
    writer: &mut FrameWriter<W>,
) {
    // Preprocessing
    {
        let deadline =
            Instant::now() + Duration::from_millis(state.preprocessing_timeout_ms().into());
        let ctx = Context::new(deadline);
        bot.runner_preprocess(state, &ctx);
        send_frame(writer, &build_preprocessing_done()).await;
    }

    // Play turns
    loop {
        let frame = match reader.read_frame().await {
            Ok(f) => f,
            Err(_) => break,
        };

        match extract_host_msg(frame) {
            Ok(HostMsg::TurnState(ts)) => {
                state.update(ts);

                let timeout_ms =
                    u64::from(state.move_timeout_ms()).saturating_sub(MOVE_SAFETY_MARGIN_MS);
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                let ctx = Context::new(deadline);

                let actions = match catch_unwind(AssertUnwindSafe(|| bot.runner_think(state, &ctx)))
                {
                    Ok(a) => a,
                    Err(panic) => {
                        let msg = panic_message(&panic);
                        eprintln!("[sdk] think() panicked: {msg}");
                        T::runner_stay(state)
                    },
                };

                for (player, direction) in actions {
                    send_frame(writer, &build_action(player, direction)).await;
                }
            },
            Ok(HostMsg::Ping) => {
                send_frame(writer, &build_pong()).await;
            },
            Ok(HostMsg::Timeout { .. }) => {
                eprintln!("[sdk] timeout received");
            },
            Ok(HostMsg::GameOver(go)) => {
                bot.runner_on_game_over(go.result, (go.player1_score, go.player2_score));
                break;
            },
            Ok(HostMsg::Stop) => break,
            Ok(other) => {
                eprintln!("[sdk] unexpected message during play: {}", msg_name(&other));
            },
            Err(e) => {
                eprintln!("[sdk] play parse error: {e}");
            },
        }
    }
}

// ── Helpers ──────────────────────────────────────────

async fn send_frame<W: AsyncWrite + Unpin>(writer: &mut FrameWriter<W>, data: &[u8]) {
    if let Err(e) = writer.write_frame(data).await {
        eprintln!("[sdk] write error: {e}");
        std::process::exit(1);
    }
}

fn msg_name(msg: &HostMsg) -> &'static str {
    match msg {
        HostMsg::SetOption { .. } => "SetOption",
        HostMsg::MatchConfig(_) => "MatchConfig",
        HostMsg::StartPreprocessing => "StartPreprocessing",
        HostMsg::TurnState(_) => "TurnState",
        HostMsg::Timeout { .. } => "Timeout",
        HostMsg::GameOver(_) => "GameOver",
        HostMsg::Ping => "Ping",
        HostMsg::Stop => "Stop",
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
