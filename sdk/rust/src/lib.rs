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
pub use bot::{Bot, Context, Hivemind, InfoParams, InfoSender};
pub use options::{Options, SdkOptionDef};
pub use pyrat_wire::OptionType;
pub use state::GameState;

// Re-export engine GameState as GameSim for simulation/search use
pub use pyrat::GameState as GameSim;

// Re-export engine types bots need
pub use pyrat::{Coordinates, Direction, MoveUndo};
pub use pyrat_engine_interface::pathfinding::FullPathResult;
pub use pyrat_engine_interface::GameView;

// Re-export wire types bots need
pub use pyrat_wire::{GameResult, Player};

// Re-export derive macro
pub use pyrat_sdk_derive::Options as DeriveOptions;

/// Safety margin subtracted from move timeout to account for communication latency.
const MOVE_SAFETY_MARGIN_MS: u64 = 5;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pyrat_protocol::{BotMsg, HostMsg, SearchLimits};
use pyrat_wire::framing::{FrameReader, FrameWriter};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use wire::{parse_host_frame, serialize};

/// Run a single-player bot. Blocks until the game ends.
///
/// Reads `PYRAT_HOST_PORT` and `PYRAT_AGENT_ID` from the environment,
/// connects to the host, and runs the full lifecycle.
pub fn run(mut bot: impl Bot, name: &str, author: &str) {
    let std_stream = connect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(run_async(
        &mut bot::BotRunner(&mut bot),
        name,
        author,
        std_stream,
    ));
}

/// Run a hivemind bot controlling both players. Blocks until the game ends.
pub fn run_hivemind(mut bot: impl Hivemind, name: &str, author: &str) {
    let std_stream = connect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(run_async(
        &mut bot::HivemindRunner(&mut bot),
        name,
        author,
        std_stream,
    ));
}

/// Connect to the host. The std→tokio conversion happens inside `run_async`.
fn connect() -> std::net::TcpStream {
    let port: u16 = std::env::var("PYRAT_HOST_PORT")
        .expect("PYRAT_HOST_PORT not set")
        .parse()
        .expect("PYRAT_HOST_PORT not a valid port");

    let stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .expect("failed to connect to host");
    stream.set_nodelay(true).expect("failed to set TCP_NODELAY");
    stream
}

fn get_agent_id() -> String {
    std::env::var("PYRAT_AGENT_ID").unwrap_or_default()
}

// ── Bot lifecycle ────────────────────────────────────

async fn run_async(
    bot: &mut impl bot::Runner,
    name: &str,
    author: &str,
    std_stream: std::net::TcpStream,
) {
    std_stream
        .set_nonblocking(true)
        .expect("failed to set non-blocking");
    let stream = TcpStream::from_std(std_stream).expect("failed to convert TCP socket to tokio");
    let (read, write) = tokio::io::split(stream);
    let mut reader = FrameReader::with_default_max(read);
    let mut writer = FrameWriter::with_default_max(write);
    let agent_id = get_agent_id();

    // Send Identify and wait for Welcome → Configure → GoPreprocess.
    let identify = serialize(&BotMsg::Identify {
        name: name.to_string(),
        author: author.to_string(),
        agent_id,
        options: bot.option_defs(),
    });
    send_frame(&mut writer, &identify).await;

    let mut state = setup_phase(bot, &mut reader, &mut writer).await;

    // Repurpose the async writer as a persistent writer task behind a channel.
    // This avoids the O_NONBLOCK sharing bug from try_clone()'d TcpStreams.
    let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(writer_task(writer, write_rx));
    let info_sender = bot::InfoSender::new(write_tx);

    let stopped = Arc::new(AtomicBool::new(false));
    let game_over = Arc::new(AtomicBool::new(false));
    let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel();

    // Spawn persistent reader task — reads frames, sets stop flag, forwards to turn loop.
    let reader_stopped = stopped.clone();
    let reader_game_over = game_over.clone();
    tokio::spawn(async move {
        reader_task(reader, msg_tx, reader_stopped, reader_game_over).await;
    });

    turn_loop(bot, &mut state, msg_rx, &info_sender, stopped, game_over).await;
}

// ── Writer task ──────────────────────────────────────

/// Drains the write channel and sends each frame through the async `FrameWriter`.
async fn writer_task<W: AsyncWrite + Unpin>(
    mut writer: FrameWriter<W>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(frame) = rx.recv().await {
        if let Err(e) = writer.write_frame(&frame).await {
            eprintln!("[sdk] writer task error: {e}");
            break;
        }
    }
}

// ── Setup phase ──────────────────────────────────────

/// Run the new-protocol handshake:
///
///   <- Welcome { player_slot }
///   <- Configure { options, match_config }
///   -> Ready { state_hash }
///   <- GoPreprocess { state_hash }
///
/// Returns once GoPreprocess is received. The caller drives preprocess()
/// from the turn loop using the returned state.
async fn setup_phase<O: options::Options, R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    bot: &mut O,
    reader: &mut FrameReader<R>,
    writer: &mut FrameWriter<W>,
) -> state::GameState {
    let mut slot: Option<Player> = None;
    let mut state_opt: Option<state::GameState> = None;

    loop {
        let frame = match reader.read_frame().await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[sdk] setup read error: {e}");
                std::process::exit(1);
            },
        };

        match parse_host_frame(frame) {
            Ok(HostMsg::Welcome { player_slot }) => {
                slot = Some(player_slot);
            },
            Ok(HostMsg::Configure {
                options,
                match_config,
            }) => {
                let player_slot = slot.unwrap_or_else(|| {
                    eprintln!("[sdk] Configure received before Welcome");
                    std::process::exit(1);
                });
                for (name, value) in &options {
                    if let Err(e) = bot.apply_option(name, value) {
                        eprintln!("[sdk] warning: option {name}={value}: {e}");
                    }
                }
                let s =
                    state::GameState::from_config(player_slot, &match_config).unwrap_or_else(|e| {
                        eprintln!("[sdk] error building game state: {e}");
                        std::process::exit(1);
                    });
                let ready = serialize(&BotMsg::Ready {
                    state_hash: s.state_hash(),
                });
                send_frame(writer, &ready).await;
                state_opt = Some(s);
            },
            Ok(HostMsg::GoPreprocess { state_hash }) => {
                let Some(s) = state_opt else {
                    eprintln!("[sdk] GoPreprocess received before Configure");
                    std::process::exit(1);
                };
                if s.state_hash() != state_hash {
                    eprintln!(
                        "[sdk] GoPreprocess hash mismatch (host={state_hash:#018x}, sdk={:#018x})",
                        s.state_hash()
                    );
                    std::process::exit(1);
                }
                return s;
            },
            Ok(HostMsg::ProtocolError { reason }) => {
                eprintln!("[sdk] host reported protocol error: {reason}");
                std::process::exit(1);
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
}

// ── Reader task ──────────────────────────────────────

/// Persistent reader — owns the socket read half, forwards messages to the
/// turn loop via a channel, sets the stop flag on Stop/GameOver.
async fn reader_task<R: AsyncRead + Unpin>(
    mut reader: FrameReader<R>,
    msg_tx: tokio::sync::mpsc::UnboundedSender<HostMsg>,
    stopped: Arc<AtomicBool>,
    game_over: Arc<AtomicBool>,
) {
    loop {
        let frame = match reader.read_frame().await {
            Ok(f) => f,
            Err(pyrat_wire::framing::FrameError::Disconnected) => {
                game_over.store(true, Ordering::Relaxed);
                break;
            },
            Err(e) => {
                eprintln!("[sdk] read error: {e}");
                game_over.store(true, Ordering::Relaxed);
                break;
            },
        };
        match parse_host_frame(frame) {
            Ok(msg) => {
                if matches!(&msg, HostMsg::Stop | HostMsg::GameOver { .. }) {
                    stopped.store(true, Ordering::Relaxed);
                }
                if matches!(&msg, HostMsg::GameOver { .. }) {
                    game_over.store(true, Ordering::Relaxed);
                }
                let _ = msg_tx.send(msg);
            },
            Err(e) => {
                eprintln!("[sdk] parse error: {e}");
            },
        }
    }
    // msg_tx dropped here → msg_rx.recv() returns None → turn_loop exits.
}

// ── Turn loop ────────────────────────────────────────

async fn turn_loop<T: bot::Runner>(
    bot: &mut T,
    state: &mut state::GameState,
    mut msg_rx: tokio::sync::mpsc::UnboundedReceiver<HostMsg>,
    info_sender: &bot::InfoSender,
    stopped: Arc<AtomicBool>,
    game_over: Arc<AtomicBool>,
) {
    // Preprocessing: GoPreprocess was already received in setup. Run preprocess()
    // immediately and signal completion.
    {
        let deadline =
            Instant::now() + Duration::from_millis(state.preprocessing_timeout_ms().into());
        let ctx = Context::new(
            deadline,
            Instant::now(),
            state.my_player(),
            0,
            state.state_hash(),
            Some(info_sender.clone()),
            stopped.clone(),
            game_over.clone(),
        );
        tokio::task::block_in_place(|| {
            bot.runner_preprocess(state, &ctx);
        });
        info_sender.send(&serialize(&BotMsg::PreprocessingDone));
    }

    // Play turns
    while let Some(msg) = msg_rx.recv().await {
        match msg {
            HostMsg::Advance {
                p1_dir,
                p2_dir,
                turn: _,
                new_hash,
            } => {
                let computed = state.apply_advance(p1_dir, p2_dir);
                if computed == new_hash {
                    info_sender.send(&serialize(&BotMsg::SyncOk { hash: computed }));
                } else {
                    info_sender.send(&serialize(&BotMsg::Resync { my_hash: computed }));
                }
            },
            HostMsg::FullState {
                match_config,
                turn_state,
            } => match state.load_full_state(&match_config, &turn_state) {
                Ok(hash) => {
                    info_sender.send(&serialize(&BotMsg::SyncOk { hash }));
                },
                Err(e) => {
                    eprintln!("[sdk] failed to load FullState: {e}");
                    break;
                },
            },
            HostMsg::Go { state_hash, limits } => {
                think_and_send(
                    bot,
                    state,
                    info_sender,
                    &stopped,
                    &game_over,
                    state_hash,
                    limits,
                );
            },
            HostMsg::GoState {
                turn_state,
                state_hash,
                limits,
            } => {
                let computed = state.load_turn_state(&turn_state);
                if computed != state_hash {
                    eprintln!(
                        "[sdk] GoState hash mismatch (host={state_hash:#018x}, sdk={computed:#018x})"
                    );
                }
                think_and_send(
                    bot,
                    state,
                    info_sender,
                    &stopped,
                    &game_over,
                    state_hash,
                    limits,
                );
            },
            HostMsg::Stop => {
                // reader_task already set stopped=true; nothing else to do.
            },
            HostMsg::GameOver {
                result,
                player1_score,
                player2_score,
            } => {
                bot.runner_on_game_over(result, (player1_score, player2_score));
                break;
            },
            HostMsg::ProtocolError { reason } => {
                eprintln!("[sdk] host reported protocol error: {reason}");
                break;
            },
            other => {
                eprintln!("[sdk] unexpected message during play: {}", msg_name(&other));
            },
        }
    }
}

fn think_and_send<T: bot::Runner>(
    bot: &mut T,
    state: &mut state::GameState,
    info_sender: &bot::InfoSender,
    stopped: &Arc<AtomicBool>,
    game_over: &Arc<AtomicBool>,
    state_hash: u64,
    limits: SearchLimits,
) {
    let raw_ms = u64::from(limits.timeout_ms.unwrap_or_else(|| state.move_timeout_ms()));
    let think_start = Instant::now();
    let deadline = if raw_ms == 0 {
        think_start + Duration::from_secs(86400)
    } else {
        think_start + Duration::from_millis(raw_ms.saturating_sub(MOVE_SAFETY_MARGIN_MS))
    };

    let turn = state.turn();
    stopped.store(false, Ordering::Relaxed);
    let ctx = Context::new(
        deadline,
        think_start,
        state.my_player(),
        turn,
        state_hash,
        Some(info_sender.clone()),
        stopped.clone(),
        game_over.clone(),
    );

    let actions = tokio::task::block_in_place(|| {
        match catch_unwind(AssertUnwindSafe(|| bot.runner_think(state, &ctx))) {
            Ok(a) => a,
            Err(panic) => {
                let msg = panic_message(&panic);
                eprintln!("[sdk] think() panicked: {msg}");
                T::runner_stay(state)
            },
        }
    });

    // Clamp to 1: the host rejects think_ms == 0 (indistinguishable from missing field).
    let think_ms = (think_start.elapsed().as_millis() as u32).max(1);
    for (player, direction) in actions {
        info_sender.send(&serialize(&BotMsg::Action {
            direction,
            player,
            turn,
            state_hash,
            think_ms,
        }));
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
        HostMsg::Welcome { .. } => "Welcome",
        HostMsg::Configure { .. } => "Configure",
        HostMsg::GoPreprocess { .. } => "GoPreprocess",
        HostMsg::Advance { .. } => "Advance",
        HostMsg::Go { .. } => "Go",
        HostMsg::GoState { .. } => "GoState",
        HostMsg::Stop => "Stop",
        HostMsg::FullState { .. } => "FullState",
        HostMsg::ProtocolError { .. } => "ProtocolError",
        HostMsg::GameOver { .. } => "GameOver",
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

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::Coordinates;
    use pyrat_protocol::{MatchConfig, TurnState};
    use pyrat_wire::TimingMode;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc;

    fn dummy_info_sender() -> (bot::InfoSender, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (bot::InfoSender::new(tx), rx)
    }

    /// Stop sent from the host: reader_task forwards it and sets `stopped`,
    /// but does NOT set `game_over` (only GameOver and Disconnect do).
    #[tokio::test]
    async fn reader_stop_sets_flag_and_forwards() {
        let frame = pyrat_protocol::serialize_host_msg(&HostMsg::Stop);

        let (client, server) = tokio::io::duplex(4096);
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);
        let mut fw = pyrat_wire::framing::FrameWriter::with_default_max(server);

        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        fw.write_frame(&frame).await.unwrap();

        let r_stopped = stopped.clone();
        let r_game_over = game_over.clone();
        tokio::spawn(async move {
            reader_task(reader, msg_tx, r_stopped, r_game_over).await;
        });

        match msg_rx.recv().await {
            Some(HostMsg::Stop) => {},
            other => panic!("expected Stop, got {other:#?}"),
        }

        assert!(stopped.load(Ordering::Relaxed));
        assert!(!game_over.load(Ordering::Relaxed));

        drop(fw);
    }

    /// Disconnect ends the read loop and sets `game_over`.
    #[tokio::test]
    async fn reader_clean_disconnect() {
        let (client, server) = tokio::io::duplex(4096);
        drop(server);
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);

        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        reader_task(reader, msg_tx, stopped.clone(), game_over.clone()).await;

        assert!(!stopped.load(Ordering::Relaxed));
        assert!(game_over.load(Ordering::Relaxed));
        assert!(msg_rx.recv().await.is_none());
    }

    /// InfoSender survives many rapid writes without losing frames.
    /// Regression for the O_NONBLOCK sharing bug fixed by routing through
    /// an mpsc channel.
    #[tokio::test]
    async fn info_sender_writes_survive_nonblocking_socket() {
        let (client, server) = tokio::io::duplex(4096);
        let writer = FrameWriter::with_default_max(client);
        let mut reader = FrameReader::with_default_max(server);

        let (write_tx, write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(writer_task(writer, write_rx));
        let info_sender = bot::InfoSender::new(write_tx);

        const N: usize = 200;
        for i in 0..N {
            let frame = format!("frame-{i}").into_bytes();
            info_sender.send(&frame);
        }
        drop(info_sender);

        let mut received = Vec::new();
        while let Ok(frame) = reader.read_frame().await {
            received.push(frame.to_vec());
        }
        assert_eq!(received.len(), N);
        for (i, frame) in received.iter().enumerate() {
            assert_eq!(frame, &format!("frame-{i}").into_bytes());
        }
    }

    /// turn_loop must not clobber the `stopped` flag set by a queued GameOver.
    /// Reproduces the case where reader_task processed a GameOver while
    /// turn_loop was draining a TurnState that arrived just before it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn go_state_does_not_clobber_game_over_stopped_flag() {
        use std::sync::atomic::AtomicU32;

        struct SpinBot(Arc<AtomicU32>);
        impl crate::options::Options for SpinBot {}
        impl bot::Bot for SpinBot {
            fn think(
                &mut self,
                _state: &crate::state::GameState,
                ctx: &bot::Context,
            ) -> pyrat::Direction {
                while !ctx.should_stop() {
                    self.0.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(1));
                }
                pyrat::Direction::Stay
            }
        }

        let cfg = MatchConfig {
            width: 3,
            height: 3,
            max_turns: 10,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(1, 1)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(2, 2),
            timing: TimingMode::Wait,
            move_timeout_ms: 100,
            preprocessing_timeout_ms: 100,
        };
        let mut state = crate::state::GameState::from_config(Player::Player1, &cfg).unwrap();

        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        // reader_task already saw GameOver → both flags set.
        let stopped = Arc::new(AtomicBool::new(true));
        let game_over = Arc::new(AtomicBool::new(true));

        let (info_sender, _write_rx) = dummy_info_sender();
        let iterations = Arc::new(AtomicU32::new(0));

        // Channel: GoState → GameOver. The bot would normally think on GoState,
        // but with game_over set, should_stop returns true immediately.
        msg_tx
            .send(HostMsg::GoState {
                turn_state: Box::new(TurnState {
                    turn: 2,
                    player1_position: Coordinates::new(0, 0),
                    player2_position: Coordinates::new(2, 2),
                    player1_score: 0.0,
                    player2_score: 0.0,
                    player1_mud_turns: 0,
                    player2_mud_turns: 0,
                    cheese: vec![Coordinates::new(1, 1)],
                    player1_last_move: pyrat::Direction::Stay,
                    player2_last_move: pyrat::Direction::Stay,
                }),
                state_hash: 0,
                limits: SearchLimits::default(),
            })
            .unwrap();

        msg_tx
            .send(HostMsg::GameOver {
                result: GameResult::Draw,
                player1_score: 0.0,
                player2_score: 0.0,
            })
            .unwrap();

        let mut bot = SpinBot(iterations.clone());
        let mut runner = bot::BotRunner(&mut bot);

        let result = tokio::time::timeout(Duration::from_secs(5), async {
            turn_loop(
                &mut runner,
                &mut state,
                msg_rx,
                &info_sender,
                stopped.clone(),
                game_over.clone(),
            )
            .await;
        })
        .await;

        assert!(result.is_ok(), "turn_loop should complete within timeout");

        let iters = iterations.load(Ordering::Relaxed);
        assert_eq!(
            iters, 0,
            "think() should not run when GameOver is pending, but spun {iters} iterations"
        );
    }
}
