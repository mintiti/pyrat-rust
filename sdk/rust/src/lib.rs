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

    let identify = build_identify(name, author, &agent_id, &bot.option_defs());
    send_frame(&mut writer, &identify).await;
    send_frame(&mut writer, &build_ready()).await;

    let mut state = setup_phase(bot, &mut reader).await;

    // Repurpose the async writer as a persistent writer task behind a channel.
    // This avoids the O_NONBLOCK sharing bug from try_clone()'d TcpStreams.
    let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(writer_task(writer, write_rx));
    let info_sender = bot::InfoSender::new(write_tx);

    let stopped = Arc::new(AtomicBool::new(false));
    let game_over = Arc::new(AtomicBool::new(false));
    let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel();

    // Spawn persistent reader task — reads frames, sets stop flag, handles Ping.
    let pong_sender = info_sender.clone();
    let reader_stopped = stopped.clone();
    let reader_game_over = game_over.clone();
    tokio::spawn(async move {
        reader_task(
            reader,
            msg_tx,
            reader_stopped,
            reader_game_over,
            pong_sender,
        )
        .await;
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

    let Some(state) = game_state else {
        eprintln!("[sdk] MatchConfig never received before StartPreprocessing");
        std::process::exit(1);
    };
    state
}

// ── Reader task ──────────────────────────────────────

/// Persistent reader — owns the socket read half, forwards messages to the
/// turn loop via a channel, sets the stop flag on Stop/Timeout, and handles
/// Ping directly so the host doesn't time out during long think() calls.
async fn reader_task<R: AsyncRead + Unpin>(
    mut reader: FrameReader<R>,
    msg_tx: tokio::sync::mpsc::UnboundedSender<HostMsg>,
    stopped: Arc<AtomicBool>,
    game_over: Arc<AtomicBool>,
    pong_sender: bot::InfoSender,
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
        match extract_host_msg(frame) {
            Ok(HostMsg::Ping) => {
                pong_sender.send(&build_pong());
            },
            Ok(msg) => {
                if matches!(
                    &msg,
                    HostMsg::Stop | HostMsg::Timeout { .. } | HostMsg::GameOver { .. }
                ) {
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
    // Preprocessing
    {
        let deadline =
            Instant::now() + Duration::from_millis(state.preprocessing_timeout_ms().into());
        let ctx = Context::new(deadline, None, stopped.clone(), game_over.clone());
        tokio::task::block_in_place(|| {
            bot.runner_preprocess(state, &ctx);
        });
        info_sender.send(&build_preprocessing_done());
    }

    // Play turns
    while let Some(msg) = msg_rx.recv().await {
        match msg {
            HostMsg::TurnState(ts) => {
                state.update(ts);

                let raw_ms = u64::from(state.move_timeout_ms());
                let deadline = if raw_ms == 0 {
                    Instant::now() + Duration::from_secs(86400)
                } else {
                    Instant::now()
                        + Duration::from_millis(raw_ms.saturating_sub(MOVE_SAFETY_MARGIN_MS))
                };

                stopped.store(false, Ordering::Relaxed);
                let ctx = Context::new(
                    deadline,
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

                let turn = state.turn();
                for (player, direction) in actions {
                    info_sender.send(&build_action(player, direction, turn));
                }
            },
            HostMsg::Timeout { .. } => {
                eprintln!("[sdk] timeout received");
            },
            HostMsg::GameOver(go) => {
                bot.runner_on_game_over(go.result, (go.player1_score, go.player2_score));
                break;
            },
            HostMsg::Stop => {},
            other => {
                eprintln!("[sdk] unexpected message during play: {}", msg_name(&other));
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

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat_wire::{self as wire, HostMessage};
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc;

    use crate::wire::HostMsg;

    fn build_host_packet<F>(msg_type: HostMessage, build_msg: F) -> Vec<u8>
    where
        F: FnOnce(
            &mut flatbuffers::FlatBufferBuilder,
        ) -> flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>,
    {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let msg_offset = build_msg(&mut fbb);
        let packet = wire::HostPacket::create(
            &mut fbb,
            &wire::HostPacketArgs {
                message_type: msg_type,
                message: Some(msg_offset),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    fn dummy_info_sender() -> (bot::InfoSender, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (bot::InfoSender::new(tx), rx)
    }

    /// Write length-prefixed frames to a tokio duplex writer.
    async fn write_frames(
        mut writer: pyrat_wire::framing::FrameWriter<tokio::io::DuplexStream>,
        frames: Vec<Vec<u8>>,
    ) {
        for frame in &frames {
            writer.write_frame(frame).await.unwrap();
        }
        // drop writer → EOF → reader_task exits
    }

    #[tokio::test]
    async fn reader_stop_sets_flag_and_forwards() {
        let frame = build_host_packet(HostMessage::Stop, |fbb| {
            wire::Stop::create(fbb, &wire::StopArgs {}).as_union_value()
        });

        let (client, server) = tokio::io::duplex(4096);
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);
        let mut fw = pyrat_wire::framing::FrameWriter::with_default_max(server);

        let (info_sender, _write_rx) = dummy_info_sender();
        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        // Write Stop but keep writer alive — no Disconnect yet.
        fw.write_frame(&frame).await.unwrap();

        let r_stopped = stopped.clone();
        let r_game_over = game_over.clone();
        tokio::spawn(async move {
            reader_task(reader, msg_tx, r_stopped, r_game_over, info_sender).await;
        });

        match msg_rx.recv().await {
            Some(HostMsg::Stop) => {},
            other => panic!("expected Stop, got {other:#?}"),
        }

        assert!(stopped.load(Ordering::Relaxed));
        // Stop is non-terminal — only GameOver and Disconnected set game_over.
        assert!(!game_over.load(Ordering::Relaxed));

        drop(fw); // Let reader_task exit cleanly.
    }

    #[tokio::test]
    async fn reader_timeout_sets_flag_and_forwards() {
        let frame = build_host_packet(HostMessage::Timeout, |fbb| {
            wire::Timeout::create(
                fbb,
                &wire::TimeoutArgs {
                    default_move: wire::Direction::Stay,
                },
            )
            .as_union_value()
        });

        let (client, server) = tokio::io::duplex(4096);
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);
        let fw = pyrat_wire::framing::FrameWriter::with_default_max(server);

        let (info_sender, _write_rx) = dummy_info_sender();
        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        tokio::spawn(write_frames(fw, vec![frame]));
        reader_task(
            reader,
            msg_tx,
            stopped.clone(),
            game_over.clone(),
            info_sender,
        )
        .await;

        assert!(stopped.load(Ordering::Relaxed));
        // game_over is also true here because the writer dropped after
        // sending the Timeout frame, triggering a Disconnected → game_over.
        // That's fine — Disconnected is game-ending regardless.
        match msg_rx.recv().await {
            Some(HostMsg::Timeout { .. }) => {},
            other => panic!("expected Timeout, got {other:#?}"),
        }
    }

    #[tokio::test]
    async fn reader_clean_disconnect() {
        let (client, server) = tokio::io::duplex(4096);
        drop(server); // EOF on the read side
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);

        let (info_sender, _write_rx) = dummy_info_sender();
        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        reader_task(
            reader,
            msg_tx,
            stopped.clone(),
            game_over.clone(),
            info_sender,
        )
        .await;

        assert!(!stopped.load(Ordering::Relaxed));
        assert!(game_over.load(Ordering::Relaxed)); // Disconnect is game-ending
        assert!(msg_rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn reader_ping_sends_pong_not_forwarded() {
        let frame = build_host_packet(HostMessage::Ping, |fbb| {
            wire::Ping::create(fbb, &wire::PingArgs {}).as_union_value()
        });

        let (client, server) = tokio::io::duplex(4096);
        let reader = pyrat_wire::framing::FrameReader::with_default_max(client);
        let fw = pyrat_wire::framing::FrameWriter::with_default_max(server);

        let (info_sender, mut write_rx) = dummy_info_sender();
        let stopped = Arc::new(AtomicBool::new(false));
        let game_over = Arc::new(AtomicBool::new(false));
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        tokio::spawn(write_frames(fw, vec![frame]));
        reader_task(
            reader,
            msg_tx,
            stopped.clone(),
            game_over.clone(),
            info_sender,
        )
        .await;

        // No messages forwarded — channel is closed, recv returns None.
        assert!(msg_rx.recv().await.is_none());
        assert!(!stopped.load(Ordering::Relaxed));

        // Read the Pong from the write channel.
        let pong_buf = write_rx
            .recv()
            .await
            .expect("expected Pong frame in channel");
        let packet = flatbuffers::root::<wire::BotPacket>(&pong_buf).unwrap();
        assert_eq!(packet.message_type(), wire::BotMessage::Pong);
    }

    // ── InfoSender regression test ─────────────────────

    /// Verifies that many rapid Info writes all arrive intact through the
    /// channel-based InfoSender + async writer task.
    ///
    /// This is the fix for the O_NONBLOCK sharing bug: try_clone()'d
    /// TcpStreams share the file description, so set_nonblocking(true) on
    /// one side also makes the clone non-blocking. write_all() then fails
    /// with EAGAIN or produces partial writes that corrupt framing.
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
        // Drop sender → writer task drains remaining frames → closes writer → EOF.
        drop(info_sender);

        let mut received = Vec::new();
        while let Ok(frame) = reader.read_frame().await {
            received.push(frame.to_vec());
        }
        assert_eq!(received.len(), N, "expected all {N} frames to arrive");
        for (i, frame) in received.iter().enumerate() {
            assert_eq!(frame, &format!("frame-{i}").into_bytes());
        }
    }

    // ── Turn-loop regression tests ──────────────────────

    /// Reproduces the stopped-flag race: reader_task sets stopped=true for a
    /// queued GameOver, but turn_loop resets it to false when processing a
    /// TurnState that sits between the Timeout and GameOver in the channel.
    ///
    /// With the bug: think() spins for the full move timeout (~95 ms).
    /// With the fix: think() sees the game is over and exits immediately.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn turn_state_does_not_clobber_game_over_stopped_flag() {
        use crate::wire::{GameOverData, MatchConfigData, TurnStateData};
        use std::sync::atomic::AtomicU32;

        // Bot that spins in think(), counting iterations until should_stop().
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

        // Minimal game state with a short move timeout.
        let cfg = MatchConfigData {
            width: 3,
            height: 3,
            max_turns: 10,
            walls: vec![],
            mud: vec![],
            cheese: vec![pyrat::Coordinates::new(1, 1)],
            player1_start: pyrat::Coordinates::new(0, 0),
            player2_start: pyrat::Coordinates::new(2, 2),
            controlled_players: vec![Player::Player1],
            timing: wire::TimingMode::Wait,
            move_timeout_ms: 100,
            preprocessing_timeout_ms: 100,
        };
        let mut state = crate::state::GameState::from_config(&cfg).unwrap();

        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        // Simulate: reader_task already processed Timeout + GameOver.
        let stopped = Arc::new(AtomicBool::new(true));
        let game_over = Arc::new(AtomicBool::new(true));

        let (info_sender, _write_rx) = dummy_info_sender();
        let iterations = Arc::new(AtomicU32::new(0));

        // Channel contents: TurnState → GameOver.
        // This mirrors the real race: bot was slow on the previous turn, got timed
        // out, and by the time the turn_loop drains the queue it finds a new
        // TurnState followed immediately by GameOver.
        msg_tx
            .send(HostMsg::TurnState(TurnStateData {
                turn: 2,
                player1_position: pyrat::Coordinates::new(0, 0),
                player2_position: pyrat::Coordinates::new(2, 2),
                player1_score: 0.0,
                player2_score: 0.0,
                player1_mud_turns: 0,
                player2_mud_turns: 0,
                cheese: vec![pyrat::Coordinates::new(1, 1)],
                player1_last_move: pyrat::Direction::Stay,
                player2_last_move: pyrat::Direction::Stay,
            }))
            .unwrap();

        msg_tx
            .send(HostMsg::GameOver(GameOverData {
                result: GameResult::Draw,
                player1_score: 0.0,
                player2_score: 0.0,
            }))
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

        // With the bug: stopped is reset to false by TurnState processing,
        // so think() spins for ~95ms producing ~95 iterations.
        // With the fix: think() exits immediately, 0 iterations.
        let iters = iterations.load(Ordering::Relaxed);
        assert_eq!(
            iters, 0,
            "think() should not run when GameOver is pending, but spun {iters} iterations"
        );
    }
}
