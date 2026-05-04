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
//! 1. `Match::run()` is a droppable async future — when the surrounding
//!    `select!` resolves on the cancel arm, dropping the run future returns
//!    promptly without hanging.
//! 2. `accept_players()` is droppable — cancellation may fire mid-handshake,
//!    before any `Match` exists.
//! 3. `BotProcesses::Drop` reaps spawned children when the *async task* that
//!    owns it is dropped (the simple sync drop case is already covered by
//!    `launch::tests::drop_kills_process`).
//!
//! Each test below pins one invariant. The conclusion: **no host change
//! needed for cancellation.** The mechanism is RAII — `BotProcesses` for
//! children, the existing async/await graph for everything else.

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
    use std::process::Command;
    Command::new("kill")
        .args(["-0", &pid.to_string()])
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

// ── Tests ─────────────────────────────────────────────

/// Invariant 1: dropping `Match::run()` from a `select!` cancel arm returns
/// promptly. Both players are silent at the recv layer; without cancel the
/// match would hang at `setup()`'s `recv(Ready)`.
#[tokio::test]
async fn match_run_drops_cleanly_on_select_cancel() {
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
