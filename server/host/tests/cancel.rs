//! Orchestrator cancellation seam.
//!
//! The orchestrator (`pyrat-orchestrator`, future PR) runs each match as a
//! task that owns a `BotProcesses` RAII guard and a `Match`, racing the
//! match-driving future against a cancellation signal:
//!
//! ```ignore
//! async fn run_match(matchup, cancel) -> ... {
//!     let _procs = launch_bots(&matchup.bots, port)?;   // RAII guard
//!     let players = accept_players(&listener, ...).await?;
//!     let m = Match::new(game, players, ...);
//!     tokio::select! {
//!         res = m.run() => res,
//!         _ = cancel.cancelled() => Err(Cancelled),
//!     }
//!     // _procs drops on either branch → children reaped via BotProcesses::Drop.
//! }
//! ```
//!
//! Three invariants need to hold for this shape to be cancel-safe with no
//! source changes in `pyrat-host` or `pyrat-protocol`:
//!
//! 1. `Match::run()` is a droppable async future — dropping the run future
//!    from a `select!` cancel arm returns promptly. Two tests cover this:
//!    cancel during `setup()` (first inbound await on `BotMsg::Ready`) and
//!    cancel during `step()` (playing-loop await on `BotMsg::Action`).
//! 2. `accept_players()` is droppable — cancellation may fire mid-handshake,
//!    before any `Match` exists.
//! 3. `BotProcesses::Drop` reaps spawned children when the *async task* that
//!    owns it is dropped (the simple sync drop case is already covered by
//!    `launch::tests::drop_kills_process`).
//!
//! Each test below pins one invariant (invariant 1 is split into two
//! tests covering the setup and step phases of `Match::run`). The
//! conclusion: **no host change needed for cancellation.** The mechanism
//! is RAII — `BotProcesses` for children, the existing async/await graph
//! for everything else.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use pyrat::{Coordinates, Direction, GameBuilder, GameState};
use pyrat_host::launch::{launch_bots, BotConfig};
use pyrat_host::match_config::build_match_config;
use pyrat_host::match_host::{Match, PlayingConfig, SetupTiming};
use pyrat_host::player::{accept_players, EventSink, Player, PlayerError, PlayerIdentity};
use pyrat_protocol::{BotMsg, HostMsg};
use pyrat_wire::{Player as PlayerSlot, TimingMode};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::sleep;

/// Time to give the OS to reap a SIGKILL'd subprocess before re-checking
/// liveness. Matches `launch::tests::drop_kills_process`.
const REAP_GRACE: Duration = Duration::from_millis(50);

const IDLE_AGENT_ID: &str = "pyrat/test/idle";

// ── Fixtures ──────────────────────────────────────────

fn make_game() -> GameState {
    GameBuilder::new(5, 5)
        .with_max_turns(5)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(4, 4))
        .with_custom_cheese(vec![Coordinates::new(2, 2)])
        .build()
        .create(Some(42))
        .expect("create game")
}

fn identity(slot: PlayerSlot, name: &str) -> PlayerIdentity {
    PlayerIdentity {
        name: name.into(),
        author: "tests".into(),
        agent_id: format!("pyrat/test/{name}"),
        slot,
    }
}

fn long_setup() -> SetupTiming {
    // Long enough that the test's cancel always wins the race against
    // any internal timeout.
    SetupTiming {
        configure_timeout: Duration::from_secs(30),
        preprocessing_timeout: Duration::from_secs(30),
    }
}

fn fast_playing() -> PlayingConfig {
    PlayingConfig {
        move_timeout: Duration::from_secs(30),
        network_grace: Duration::from_millis(50),
        ..Default::default()
    }
}

fn idle_command() -> String {
    if cfg!(unix) {
        "sleep 30".into()
    } else {
        "timeout /t 30 /nobreak >nul".into()
    }
}

fn idle_bot_config() -> BotConfig {
    BotConfig {
        run_command: idle_command(),
        working_dir: PathBuf::from("."),
        agent_id: IDLE_AGENT_ID.into(),
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    use std::process::{Command, Stdio};
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn a task that fires `cancel` after a short delay. The delay must be
/// long enough for the future-under-test to reach an await point but short
/// enough to keep the test fast.
fn schedule_cancel(cancel: Arc<Notify>, after: Duration) {
    tokio::spawn(async move {
        sleep(after).await;
        cancel.notify_waiters();
    });
}

/// Player that accepts every send and never returns from recv. Stand-in for
/// any peer that's silent at the protocol layer (no clean close, no
/// disconnect — just ∞ latency).
struct HangingPlayer {
    identity: PlayerIdentity,
}

#[async_trait]
impl Player for HangingPlayer {
    fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    async fn send(&mut self, _: HostMsg) -> Result<(), PlayerError> {
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
        std::future::pending::<()>().await;
        unreachable!()
    }

    fn take_provisional(&mut self, _: u16, _: u64) -> Option<Direction> {
        None
    }

    async fn close(self: Box<Self>) -> Result<(), PlayerError> {
        Ok(())
    }
}

/// Player that completes setup with canned replies, then hangs forever in
/// recv. Used to land the cancel during the playing loop's `recv(Action)`,
/// past `setup()`'s two recv points. Fires `step_reached` (via
/// `notify_one`, so the permit is stored if no waiter is set up yet) when
/// it observes `HostMsg::Go` — that's the precise point at which the host
/// has entered the step await graph.
struct StepHangingPlayer {
    identity: PlayerIdentity,
    canned: VecDeque<BotMsg>,
    step_reached: Arc<Notify>,
}

impl StepHangingPlayer {
    fn new(identity: PlayerIdentity, state_hash: u64, step_reached: Arc<Notify>) -> Self {
        let canned = VecDeque::from(vec![
            BotMsg::Ready { state_hash },
            BotMsg::PreprocessingDone,
        ]);
        Self {
            identity,
            canned,
            step_reached,
        }
    }
}

#[async_trait]
impl Player for StepHangingPlayer {
    fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError> {
        if matches!(msg, HostMsg::Go { .. }) {
            self.step_reached.notify_one();
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
        if let Some(msg) = self.canned.pop_front() {
            return Ok(Some(msg));
        }
        std::future::pending::<()>().await;
        unreachable!()
    }

    fn take_provisional(&mut self, _: u16, _: u64) -> Option<Direction> {
        None
    }

    async fn close(self: Box<Self>) -> Result<(), PlayerError> {
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────

/// Invariant 1, setup phase: dropping `Match::run()` from a `select!`
/// cancel arm returns promptly when the bots are silent at the very first
/// recv. Without cancel the match would hang at `setup()`'s `recv(Ready)`.
#[tokio::test]
async fn match_run_drops_cleanly_during_setup() {
    let game = make_game();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let p1: Box<dyn Player> = Box::new(HangingPlayer {
        identity: identity(PlayerSlot::Player1, "p1"),
    });
    let p2: Box<dyn Player> = Box::new(HangingPlayer {
        identity: identity(PlayerSlot::Player2, "p2"),
    });

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        long_setup(),
        fast_playing(),
        None,
    );

    let cancel = Arc::new(Notify::new());
    schedule_cancel(cancel.clone(), Duration::from_millis(50));

    let started = Instant::now();
    let cancelled = tokio::select! {
        res = m.run() => {
            panic!("match.run() should not complete with hanging players, got {res:?}");
        }
        _ = cancel.notified() => true,
    };
    let elapsed = started.elapsed();

    assert!(cancelled);
    assert!(
        elapsed < Duration::from_millis(500),
        "cancel-then-drop took {elapsed:?} — match.run() did not drop promptly"
    );
}

/// Invariant 1, step phase: dropping `Match::run()` from a `select!` cancel
/// arm returns promptly when the bots reach the playing loop. Setup
/// completes via canned replies; then both players hang at the first
/// `recv(Action)`. This is the orchestrator's actual cancel risk surface —
/// a bot that completes setup, plays N turns, then stalls during a move.
/// Sibling to `match_run_drops_cleanly_during_setup` — same invariant, the
/// other half of `Match::run`'s await graph.
///
/// The cancel is triggered by the players observing `HostMsg::Go` (not a
/// timer) so the test can't accidentally cancel during setup if setup ever
/// slows down. The outer `select!` carries a 500ms timeout arm: if `Go` is
/// never reached or cancel doesn't propagate, the test fails fast instead
/// of hanging.
#[tokio::test]
async fn match_run_drops_cleanly_during_step() {
    let game = make_game();
    let state_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let step_reached = Arc::new(Notify::new());
    let p1: Box<dyn Player> = Box::new(StepHangingPlayer::new(
        identity(PlayerSlot::Player1, "p1"),
        state_hash,
        step_reached.clone(),
    ));
    let p2: Box<dyn Player> = Box::new(StepHangingPlayer::new(
        identity(PlayerSlot::Player2, "p2"),
        state_hash,
        step_reached.clone(),
    ));

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        long_setup(),
        fast_playing(),
        None,
    );

    let cancel = Arc::new(Notify::new());
    let cancel_trigger = cancel.clone();
    tokio::spawn(async move {
        step_reached.notified().await;
        cancel_trigger.notify_waiters();
    });

    let started = Instant::now();
    tokio::select! {
        res = m.run() => {
            panic!("match.run() should not complete while bots hang at step, got {res:?}");
        }
        _ = cancel.notified() => {}
        _ = sleep(Duration::from_millis(500)) => {
            panic!("timeout: match never reached Go in 500ms, or cancel did not arm");
        }
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "cancel-then-drop took {elapsed:?} — `m.run()` did not drop promptly after cancel fired"
    );
}

/// Invariant 2: dropping `accept_players` mid-handshake returns promptly.
/// The bot subprocess is `sleep 30` — it never connects to the listener, so
/// `accept_players` would otherwise block until its overall_timeout (30s).
/// `BotProcesses` is owned by the same async block, so this also exercises
/// the orchestrator-shaped drop path for invariant 3.
#[tokio::test]
async fn accept_players_drops_cleanly_on_select_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");

    let port = listener.local_addr().unwrap().port();
    let procs = launch_bots(&[idle_bot_config()], port).expect("spawn idle bot");
    #[cfg(unix)]
    let pid = procs.pid(0).expect("spawned pid");

    let cancel = Arc::new(Notify::new());
    schedule_cancel(cancel.clone(), Duration::from_millis(50));

    let started = Instant::now();
    let work = async move {
        // procs is owned by the work future so it drops (and reaps children)
        // when select! cancels — that's the orchestrator-shaped invariant.
        let _procs = procs;
        let _ = accept_players(
            &listener,
            &[(PlayerSlot::Player1, IDLE_AGENT_ID.into())],
            EventSink::noop(),
            Duration::from_secs(30),
        )
        .await;
    };

    tokio::select! {
        () = work => panic!("accept_players completed unexpectedly"),
        _ = cancel.notified() => {}
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "cancel-then-drop took {elapsed:?} — accept_players did not drop promptly"
    );

    sleep(REAP_GRACE).await;

    #[cfg(unix)]
    assert!(
        !pid_alive(pid),
        "pid {pid} should be dead after async task drop reaped BotProcesses"
    );
}

/// Invariant 3 (focused): `BotProcesses::Drop` reaps children when the
/// owning async future is dropped via `select!` cancel. Sibling to the sync
/// `launch::tests::drop_kills_process` — that test drops `BotProcesses`
/// from a sync `drop()` call; this one drops it via async future drop, which
/// is the orchestrator's actual mode.
#[tokio::test]
async fn bot_processes_drop_kills_children_when_async_task_drops() {
    let procs = launch_bots(&[idle_bot_config()], 9_999).expect("spawn idle bot");
    #[cfg(unix)]
    let pid = procs.pid(0).expect("spawned pid");

    #[cfg(unix)]
    assert!(pid_alive(pid), "sanity: pid {pid} should be alive at start");

    let cancel = Arc::new(Notify::new());
    schedule_cancel(cancel.clone(), Duration::from_millis(50));

    let work = async move {
        let _procs = procs;
        std::future::pending::<()>().await;
    };

    tokio::select! {
        () = work => unreachable!(),
        _ = cancel.notified() => {}
    }

    sleep(REAP_GRACE).await;

    #[cfg(unix)]
    assert!(
        !pid_alive(pid),
        "pid {pid} should be dead after async future drop"
    );
}
