//! Per-match work spawned on the orchestrator's `JoinSet`.
//!
//! Each call:
//! 1. Publishes `MatchQueued` and waits for a parallelism slot.
//! 2. Sets up players (subprocess via TCP, embedded in-process, or mixed).
//! 3. Calls `sink.on_match_started`. A Required failure here demotes
//!    immediately to `MatchFailed { SinkFlushError, durable_record: false }`.
//!    `Match` is never constructed, so player handles are explicitly
//!    closed to reap subprocess sessions.
//! 4. Constructs `Match`, publishes `MatchStarted`, and races
//!    `Match::run` against root cancellation, draining host events through
//!    a per-match `mpsc::UnboundedReceiver`.
//! 5. Routes events: `MatchOver` is suppressed; everything else goes
//!    through `sink.on_match_event` → live broadcast. A Required event
//!    error stops broadcast forwarding but keeps draining so the engine
//!    loop completes cleanly.
//! 6. Builds the terminal lifecycle event from `Match::run`'s returned
//!    `MatchResult` (success), the `MatchError` (failure), or cancellation
//!    state. Calls the appropriate sink terminal callback; demotes to
//!    `SinkFlushError` if the Required sink errors there too.
//!
//! Cancel-safety is by Drop: the `Match::run` future is dropped from a
//! biased `select!` arm. Subprocess children are reaped through
//! `BotProcesses`'s `Drop` (held as a local until function exit).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use pyrat::Direction;
use pyrat_bot_api::Options;
use pyrat_host::launch::{launch_bots, BotConfig, BotProcesses, LaunchError};
use pyrat_host::match_config::build_match_config;
use pyrat_host::match_host::{Match, MatchError, MatchEvent, MatchResult};
use pyrat_host::player::{
    accept_players, AcceptError, EmbeddedBot, EmbeddedCtx, EmbeddedPlayer, EventSink, Player,
    PlayerError, PlayerIdentity, TcpPlayer,
};
use pyrat_host::wire::{GameResult, Player as PlayerSlot};
use pyrat_protocol::HashedTurnState;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::descriptor::Descriptor;
use crate::event::OrchestratorEvent;
use crate::executor::{ExecutorInner, RunConfig};
use crate::id::MatchId;
use crate::matchup::{Matchup, PlayerSpec};
use crate::outcome::{FailureReason, MatchFailure, MatchOutcome};
use crate::sink::SinkError;

/// Adapter so a `Box<dyn EmbeddedBot>` (what factories produce) can be
/// handed to `EmbeddedPlayer::accept<B: EmbeddedBot>`.
///
/// The host crate's `EmbeddedBot` is sized, so `Box<dyn EmbeddedBot>` does
/// not auto-impl it. The alternative is adding a blanket
/// `impl<T: ?Sized + EmbeddedBot> EmbeddedBot for Box<T>` upstream, which
/// would touch the host trait. Wrapping at the orchestrator boundary
/// keeps the blast radius local.
struct BoxedEmbeddedBot(Box<dyn EmbeddedBot>);

impl Options for BoxedEmbeddedBot {
    fn option_defs(&self) -> Vec<pyrat_protocol::OptionDef> {
        self.0.option_defs()
    }

    fn apply_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.0.apply_option(name, value)
    }
}

impl EmbeddedBot for BoxedEmbeddedBot {
    fn think(&mut self, state: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        self.0.think(state, ctx)
    }

    fn preprocess(&mut self, state: &HashedTurnState, ctx: &EmbeddedCtx) {
        self.0.preprocess(state, ctx)
    }

    fn on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        self.0.on_game_over(result, scores)
    }
}

pub(crate) async fn run_match<D: Descriptor>(
    inner: Arc<ExecutorInner<D>>,
    matchup: Matchup<D>,
    cancel: CancellationToken,
    semaphore: Arc<Semaphore>,
    cfg: Arc<RunConfig>,
) {
    let id = matchup.descriptor.match_id();

    // ── 1. Publish MatchQueued. Driver-dropped is fatal upstream.
    let queued = OrchestratorEvent::MatchQueued {
        id,
        descriptor: matchup.descriptor.clone(),
    };
    if inner.publish_lifecycle(queued).await.is_err() {
        return;
    }

    // ── 2. Wait for a parallelism slot.
    let _permit: OwnedSemaphorePermit = match acquire_permit(&semaphore, &cancel).await {
        Some(p) => p,
        None => {
            // Cancelled before slot. No on_match_started fired, so don't
            // call sink.on_match_failed (sinks see started before they
            // see terminals).
            publish_failure(
                &inner,
                &matchup,
                None,
                FailureReason::Cancelled,
                None,
                false,
            )
            .await;
            return;
        },
    };

    // ── 3. Build the per-match event channel up front. The same `event_tx`
    // is wrapped as an `EventSink` for the player handshake (where pre-Match
    // sideband would land if it ever fired) and handed to `Match::new`.
    // One channel, one drain loop.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let handshake_sink = EventSink::new(event_tx.clone());

    // ── 4. Player setup.
    let setup = match setup_players(&matchup, &cfg, &handshake_sink, &cancel).await {
        Ok(s) => s,
        Err(setup_err) => {
            let reason = setup_err.into_failure_reason();
            // Setup happens before `on_match_started` ever fires, so we
            // intentionally skip `sink.on_match_failed`. Sinks see
            // started before they see terminals.
            publish_failure(&inner, &matchup, None, reason, None, false).await;
            return;
        },
    };
    // `_bot_procs` is held to function exit so subprocess children are
    // reaped via RAII on every path, including a dropped run future.
    let SetupOutput {
        players,
        identities,
        bot_procs: _bot_procs,
    } = setup;

    // ── 5. Pre-Match sink callback. Required failure here demotes to
    // `SinkFlushError, durable_record: false, started_at: None` per the
    // contract: we never construct `Match`, so the match never started.
    if let Err(e) = inner
        .sink
        .on_match_started(&matchup.descriptor, &identities)
        .await
    {
        // Close player handles so subprocess sessions reap. We can't call
        // sink.on_match_failed (the sink is the broken one).
        let [p1, p2] = players;
        let _ = p1.close().await;
        let _ = p2.close().await;
        let failure = MatchFailure {
            descriptor: matchup.descriptor.clone(),
            started_at: None,
            failed_at: SystemTime::now(),
            reason: FailureReason::SinkFlushError(e.to_string()),
            players: Some(identities),
            durable_record: false,
        };
        let _ = inner
            .publish_lifecycle(OrchestratorEvent::MatchFailed { failure })
            .await;
        return;
    }

    // ── 6. Build engine state + protocol-level config.
    let game = match matchup.game_config.create(Some(matchup.seed())) {
        Ok(g) => g,
        Err(s) => {
            let failure = MatchFailure {
                descriptor: matchup.descriptor.clone(),
                started_at: None,
                failed_at: SystemTime::now(),
                reason: FailureReason::Internal(format!("game state build failed: {s}")),
                players: Some(identities.clone()),
                durable_record: false,
            };
            let _ = inner.sink.on_match_failed(&failure).await;
            let _ = inner
                .publish_lifecycle(OrchestratorEvent::MatchFailed { failure })
                .await;
            return;
        },
    };
    let match_config = build_match_config(
        &game,
        matchup.timing.mode,
        matchup.timing.move_timeout_ms,
        matchup.timing.preprocessing_timeout_ms,
    );

    // ── 7. Publish MatchStarted, mark started_at.
    let started_at = SystemTime::now();
    let started_event = OrchestratorEvent::MatchStarted {
        id,
        descriptor: matchup.descriptor.clone(),
        players: identities.clone(),
    };
    if inner.publish_lifecycle(started_event).await.is_err() {
        return;
    }

    // ── 8. Construct Match and drive the run loop.
    let game_match = Match::new(
        game,
        players,
        match_config,
        [vec![], vec![]],
        cfg.setup_timing.clone(),
        cfg.playing_config.clone(),
        Some(event_tx),
    );

    let mut run_fut = std::pin::pin!(game_match.run());
    let mut sink_event_error: Option<SinkError> = None;
    let mut rx_open = true;

    let run_result: Option<Result<MatchResult, MatchError>> = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break None,
            res = &mut run_fut => {
                // Drain anything buffered before exiting the loop. The
                // `try_recv` keeps draining cheap; per-event errors are
                // recorded into `sink_event_error` and decided after.
                while let Ok(evt) = event_rx.try_recv() {
                    handle_event(&inner, id, evt, &mut sink_event_error).await;
                }
                break Some(res);
            }
            evt = event_rx.recv(), if rx_open => match evt {
                Some(e) => handle_event(&inner, id, e, &mut sink_event_error).await,
                // Defensive: in normal operation `Match::run` holds the
                // sender via `MatchCtx` until `finalize` consumes it, so
                // `recv` cannot return None before `run_fut` resolves.
                // The guard makes the loop non-spinning if that invariant
                // ever breaks.
                None => { rx_open = false; }
            }
        }
    };

    // ── 9. Build and publish the terminal event.
    let finished_at = SystemTime::now();
    let terminal = build_terminal(
        &inner,
        &matchup,
        &identities,
        started_at,
        finished_at,
        run_result,
        sink_event_error,
    )
    .await;
    let _ = inner.publish_lifecycle(terminal).await;
}

/// Apply one host event to the sink+broadcast pipe, with the Required-error
/// gate. Suppresses `MatchOver` (the canonical terminal value comes from
/// `Match::run()`'s returned `MatchResult`).
async fn handle_event<D: Descriptor>(
    inner: &Arc<ExecutorInner<D>>,
    id: MatchId,
    event: MatchEvent,
    sink_err: &mut Option<SinkError>,
) {
    if matches!(event, MatchEvent::MatchOver { .. }) {
        return;
    }
    if sink_err.is_some() {
        // Required sink already errored; keep draining the engine loop
        // but don't talk to the sink and don't publish.
        return;
    }
    if let Err(e) = inner.sink.on_match_event(id, &event).await {
        *sink_err = Some(e);
        return;
    }
    inner.publish_per_turn(OrchestratorEvent::MatchEvent { id, event });
}

async fn build_terminal<D: Descriptor>(
    inner: &Arc<ExecutorInner<D>>,
    matchup: &Matchup<D>,
    identities: &[PlayerIdentity; 2],
    started_at: SystemTime,
    finished_at: SystemTime,
    run_result: Option<Result<MatchResult, MatchError>>,
    sink_event_error: Option<SinkError>,
) -> OrchestratorEvent<D> {
    let make_failure = |reason: FailureReason, durable_record: bool| MatchFailure {
        descriptor: matchup.descriptor.clone(),
        started_at: Some(started_at),
        failed_at: finished_at,
        reason,
        players: Some(identities.clone()),
        durable_record,
    };
    let demote_to_sink_flush = |e: SinkError| OrchestratorEvent::MatchFailed {
        failure: make_failure(FailureReason::SinkFlushError(e.to_string()), false),
    };

    match (run_result, sink_event_error) {
        // Cancelled while Match::run was alive.
        (None, _) => {
            let failure = make_failure(FailureReason::Cancelled, false);
            // Best-effort sink terminal. Required errors here demote to
            // SinkFlushError (durable_record was already false).
            if let Err(e) = inner.sink.on_match_failed(&failure).await {
                return demote_to_sink_flush(e);
            }
            OrchestratorEvent::MatchFailed { failure }
        },

        // Sink errored mid-event. Don't talk to the sink again (it's the
        // broken one). Just publish.
        (Some(_), Some(sink_err)) => demote_to_sink_flush(sink_err),

        // Match::run returned a result.
        (Some(Ok(result)), None) => {
            let outcome = MatchOutcome {
                descriptor: matchup.descriptor.clone(),
                started_at,
                finished_at,
                result,
                players: identities.clone(),
            };
            match inner.sink.on_match_finished(&outcome).await {
                Ok(()) => OrchestratorEvent::MatchFinished { outcome },
                Err(e) => demote_to_sink_flush(e),
            }
        },

        // Match::run returned a host-level error.
        (Some(Err(match_err)), None) => {
            let failure = make_failure(failure_reason_from_match_error(&match_err), true);
            match inner.sink.on_match_failed(&failure).await {
                Ok(()) => OrchestratorEvent::MatchFailed { failure },
                Err(e) => demote_to_sink_flush(e),
            }
        },
    }
}

/// Publish a pre-MatchStarted failure (no sink terminal call). Used for
/// cancel-while-queued, setup errors (bind/launch/accept/embedded-accept),
/// and engine-state build errors. Sinks haven't seen `on_match_started` for
/// this match, so calling `on_match_failed` would be incoherent.
async fn publish_failure<D: Descriptor>(
    inner: &Arc<ExecutorInner<D>>,
    matchup: &Matchup<D>,
    started_at: Option<SystemTime>,
    reason: FailureReason,
    players: Option<[PlayerIdentity; 2]>,
    durable_record: bool,
) {
    let failure = MatchFailure {
        descriptor: matchup.descriptor.clone(),
        started_at,
        failed_at: SystemTime::now(),
        reason,
        players,
        durable_record,
    };
    let _ = inner
        .publish_lifecycle(OrchestratorEvent::MatchFailed { failure })
        .await;
}

async fn acquire_permit(
    semaphore: &Arc<Semaphore>,
    cancel: &CancellationToken,
) -> Option<OwnedSemaphorePermit> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        p = Arc::clone(semaphore).acquire_owned() => p.ok(),
    }
}

// ── Player setup ─────────────────────────────────────────────────────

struct SetupOutput {
    players: [Box<dyn Player>; 2],
    identities: [PlayerIdentity; 2],
    /// `Some` when at least one slot launched a subprocess, `None` for
    /// pure-embedded matches.
    bot_procs: Option<BotProcesses>,
}

#[derive(Debug)]
enum SetupError {
    Cancelled,
    Bind(std::io::Error),
    Launch(LaunchError),
    Accept(AcceptError),
    EmbeddedAccept {
        slot: PlayerSlot,
        source: PlayerError,
    },
    AcceptIncomplete {
        slot: PlayerSlot,
    },
    Internal(String),
}

impl SetupError {
    fn into_failure_reason(self) -> FailureReason {
        match self {
            Self::Cancelled => FailureReason::Cancelled,
            Self::Bind(e) => FailureReason::Internal(format!("bind: {e}")),
            Self::Launch(e) => {
                tracing::warn!(error = %e, "launch_bots failed");
                FailureReason::SpawnFailed
            },
            Self::Accept(AcceptError::Timeout) => FailureReason::HandshakeTimeout,
            Self::Accept(other) => FailureReason::Internal(other.to_string()),
            Self::EmbeddedAccept { slot, source } => match source {
                PlayerError::CleanClose | PlayerError::TransportError(_) => {
                    FailureReason::Disconnected(slot)
                },
                PlayerError::ProtocolError(s) => FailureReason::ProtocolError(s),
                PlayerError::Timeout => FailureReason::HandshakeTimeout,
            },
            Self::AcceptIncomplete { slot } => FailureReason::ProtocolError(format!(
                "accept_players returned without filling slot {slot:?}"
            )),
            Self::Internal(s) => FailureReason::Internal(s),
        }
    }
}

async fn setup_players<D: Descriptor>(
    matchup: &Matchup<D>,
    cfg: &RunConfig,
    handshake_sink: &EventSink,
    cancel: &CancellationToken,
) -> Result<SetupOutput, SetupError> {
    let subprocess_count = matchup
        .players
        .iter()
        .filter(|p| matches!(p, PlayerSpec::Subprocess { .. }))
        .count();

    let (mut tcp_slots, bot_procs): ([Option<TcpPlayer>; 2], Option<BotProcesses>) =
        if subprocess_count > 0 {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(SetupError::Bind)?;
            let port = listener.local_addr().map_err(SetupError::Bind)?.port();
            let mut bot_configs: Vec<BotConfig> = Vec::with_capacity(subprocess_count);
            let mut expected: Vec<(PlayerSlot, String)> = Vec::with_capacity(subprocess_count);
            for (idx, spec) in matchup.players.iter().enumerate() {
                let slot = slot_for(idx);
                if let PlayerSpec::Subprocess {
                    agent_id,
                    command,
                    working_dir,
                } = spec
                {
                    bot_configs.push(BotConfig {
                        run_command: command.clone(),
                        working_dir: working_dir.clone().unwrap_or_else(|| PathBuf::from(".")),
                        agent_id: agent_id.clone(),
                    });
                    expected.push((slot, agent_id.clone()));
                }
            }
            let procs = launch_bots(&bot_configs, port).map_err(SetupError::Launch)?;
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(SetupError::Cancelled),
                r = accept_players(&listener, &expected, handshake_sink.clone(), cfg.handshake_timeout) => r,
            };
            let players = result.map_err(SetupError::Accept)?;
            (players, Some(procs))
        } else {
            ([None, None], None)
        };

    let mut placed: [Option<Box<dyn Player>>; 2] = [None, None];
    for (idx, spec) in matchup.players.iter().enumerate() {
        let slot = slot_for(idx);
        match spec {
            PlayerSpec::Subprocess { .. } => {
                let p = tcp_slots[idx]
                    .take()
                    .ok_or(SetupError::AcceptIncomplete { slot })?;
                placed[idx] = Some(Box::new(p) as Box<dyn Player>);
            },
            PlayerSpec::Embedded {
                agent_id,
                name,
                author,
                factory,
            } => {
                let identity = PlayerIdentity {
                    name: name.clone(),
                    author: author.clone(),
                    agent_id: agent_id.clone(),
                    slot,
                };
                let bot = BoxedEmbeddedBot(factory());
                let p = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(SetupError::Cancelled),
                    r = EmbeddedPlayer::accept(bot, identity, handshake_sink.clone()) => {
                        r.map_err(|source| SetupError::EmbeddedAccept { slot, source })?
                    }
                };
                placed[idx] = Some(Box::new(p) as Box<dyn Player>);
            },
        }
    }

    let p1 = placed[0]
        .take()
        .ok_or_else(|| SetupError::Internal("slot 0 empty after setup".into()))?;
    let p2 = placed[1]
        .take()
        .ok_or_else(|| SetupError::Internal("slot 1 empty after setup".into()))?;
    let identities = [p1.identity().clone(), p2.identity().clone()];
    Ok(SetupOutput {
        players: [p1, p2],
        identities,
        bot_procs,
    })
}

fn slot_for(idx: usize) -> PlayerSlot {
    match idx {
        0 => PlayerSlot::Player1,
        1 => PlayerSlot::Player2,
        _ => unreachable!("Matchup carries exactly 2 player specs"),
    }
}

fn failure_reason_from_match_error(err: &MatchError) -> FailureReason {
    match err {
        MatchError::SetupTimeout(_)
        | MatchError::PreprocessingTimeout(_)
        | MatchError::SyncTimeout(_)
        | MatchError::ActionTimeout(_)
        | MatchError::ReadyHashMismatch { .. }
        | MatchError::ActionHashMismatch { .. }
        | MatchError::PersistentDesync(_)
        | MatchError::UnexpectedMessage { .. } => FailureReason::ProtocolError(err.to_string()),
        MatchError::BotDisconnected(slot) => FailureReason::Disconnected(*slot),
        MatchError::PlayerError { slot, source } => match source {
            PlayerError::CleanClose | PlayerError::TransportError(_) => {
                FailureReason::Disconnected(*slot)
            },
            PlayerError::ProtocolError(_) | PlayerError::Timeout => {
                FailureReason::ProtocolError(err.to_string())
            },
        },
        MatchError::Internal(s) => FailureReason::Internal(s.clone()),
    }
}
