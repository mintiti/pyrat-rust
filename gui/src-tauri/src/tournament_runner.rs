//! Bridges an `EvalSession` to Tauri events.
//!
//! Mirrors the CLI's wiring (`tournament_run.rs`): create the tournament,
//! build a planner, start the session, then drive a forwarding loop that
//! translates lifecycle events into frontend events. The session is the same
//! one the CLI drives, so the store rows are byte-identical — research-grade
//! by construction.
//!
//! The loop reads everything verdict-bearing (scores, standings) from the
//! lossless `SessionEvent` + `TournamentState` path. Per-turn liveness rides
//! the lossy `live_events()` channel (slice B).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pyrat::game::builder::GameConfig;
use pyrat_eval::{
    failure_reason_string, gauntlet_slot_order, split_gauntlet_players, EvalMatchDescriptor,
    EvalSession, GauntletPlanner, GauntletPlannerConfig, MatchupKey, MatchupOutcome, Planner,
    ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig, SeatPolicy, SessionConfig,
    SessionEvent, SessionMode, TournamentState,
};
use pyrat_eval_store::{compute_elo_with_uncertainty, EloOptions, EvalStore, TournamentId};
use pyrat_host::match_host::MatchEvent;
use pyrat_host::wire::Player as PlayerSlot;
use pyrat_orchestrator::{
    DirectoryWriter, FailureReason, MatchSink, OrchestratorConfig, OrchestratorEvent, ReplaySink,
    SinkRole, TimeoutPhase as CoreTimeoutPhase, Timing,
};
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::tournament_config;
use crate::tournament_events::{
    FailureKind, NowPlayingEvent, StandingRow, StandingsUpdatedEvent, TimeoutPhase,
    TournamentAbortedEvent, TournamentFinishedEvent, TournamentMatchFailedEvent,
    TournamentMatchFinishedEvent, TournamentMatchStartedEvent, TournamentStartedEvent,
};

/// Below this many games a non-anchor player's Elo is too noisy to show.
const MIN_GAMES_FOR_ESTIMATE: u32 = 4;

/// Which schedule to run. The challenger/opponents split is resolved by the
/// caller from the launch params (star → gauntlet, no star → round-robin).
pub enum RunnerFormat {
    RoundRobin,
    Gauntlet {
        challenger: String,
        opponents: Vec<String>,
    },
}

/// Everything the runner needs, resolved by the command from launch params.
/// The tournament row is created by the command (so it can return the id and
/// register the phase); the runner drives the already-created tournament.
pub struct TournamentRun {
    pub tournament_id: TournamentId,
    pub game_config_id: String,
    pub tournament_seed: u64,
    pub name: Option<String>,
    /// Stored format string: "gauntlet" or "round_robin".
    pub format_str: String,
    pub players: Vec<ResolvedPlayer>,
    pub format: RunnerFormat,
    /// Gauntlet target (the measured bot). `None` for round-robin.
    pub target: Option<String>,
    pub anchor_id: String,
    pub plan_summary: String,
    pub total_games: u32,
    /// Ordered player ids (slot order) for standings rows.
    pub player_ids: Vec<String>,
    // Resolved measurement conditions, built once by the command from the
    // launch params (no longer pinned constants). The runner consumes these
    // rather than reaching back into `tournament_config`.
    /// The game-instance distribution (board / maze / starts / cheese).
    pub game_config: GameConfig,
    /// Games per matchup (= 2 × mazes for the paired schedule).
    pub target_games_per_matchup: u32,
    pub seat_policy: SeatPolicy,
    pub timing: Timing,
    pub orchestrator_config: OrchestratorConfig,
    /// Where per-match `ReplayFile` JSONs are written (one per match), for
    /// thumbnails and the game page. An open failure disables thumbnails for
    /// this run but never blocks the tournament.
    pub replay_dir: PathBuf,
}

/// How often buffered per-turn liveness is flushed to the frontend.
const NOW_PLAYING_FLUSH: Duration = Duration::from_millis(250);

/// Run an already-created tournament to completion, emitting events along the
/// way. Spawned on a tokio task by `start_tournament`; `startup` resolves once
/// the session is live (or carries its startup failure), and `cancel` is fired
/// by `stop_tournament` or on app shutdown.
pub async fn run_tournament(
    app: AppHandle,
    store: Arc<Mutex<EvalStore>>,
    run: TournamentRun,
    cancel: CancellationToken,
    startup: oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    let mut startup = Some(startup);
    let TournamentRun {
        tournament_id,
        game_config_id,
        tournament_seed,
        name,
        format_str,
        players,
        format,
        target,
        anchor_id,
        plan_summary,
        total_games,
        player_ids,
        replay_dir,
        game_config,
        target_games_per_matchup,
        seat_policy,
        timing,
        orchestrator_config,
    } = run;
    let max_parallel = orchestrator_config.max_parallel as u32;

    // Build the planner for the chosen format from the resolved conditions.
    let elo_options = tournament_config::elo_options(&anchor_id);
    let planner: Box<dyn Planner> = match &format {
        RunnerFormat::RoundRobin => Box::new(RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players: players.clone(),
            game_config: game_config.clone(),
            game_config_id: game_config_id.clone(),
            timing,
            tournament_id,
            target_per_pair: target_games_per_matchup,
            max_failures_per_pair: tournament_config::MAX_FAILURES_PER_PAIR,
            seat_policy,
            tournament_seed,
        })),
        RunnerFormat::Gauntlet {
            challenger,
            opponents,
        } => {
            let (challenger_p, opponent_ps) =
                match split_gauntlet_players(&players, challenger, opponents) {
                    Ok(players) => players,
                    Err(error) => {
                        let reason = error.to_string();
                        if let Some(signal) = startup.take() {
                            let _ = signal.send(Err(reason.clone()));
                        }
                        return Err(reason);
                    },
                };
            Box::new(GauntletPlanner::new(GauntletPlannerConfig {
                challenger: challenger_p,
                opponents: opponent_ps,
                game_config: game_config.clone(),
                game_config_id: game_config_id.clone(),
                timing,
                tournament_id,
                target_each: target_games_per_matchup,
                max_failures_per_pair: tournament_config::MAX_FAILURES_PER_PAIR,
                seat_policy,
                tournament_seed,
            }))
        },
    };

    // 3. Attach the replay sink (Optional — a write failure never blocks the
    //    tournament, it just leaves a match without a thumbnail).
    let mut extra_sinks: Vec<(SinkRole, Arc<dyn MatchSink<EvalMatchDescriptor>>)> = Vec::new();
    match DirectoryWriter::new(replay_dir.clone()) {
        Ok(writer) => {
            let sink = ReplaySink::new(Arc::new(writer))
                .with_engine_version(format!("pyrat-gui/{}", env!("CARGO_PKG_VERSION")));
            extra_sinks.push((SinkRole::Optional, Arc::new(sink)));
        },
        Err(e) => warn!(
            error = %e,
            dir = %replay_dir.display(),
            "replay dir unavailable; match thumbnails disabled for this run"
        ),
    }

    // 4. Start the session.
    let session = match EvalSession::start_with_extra_sinks(
        store,
        SessionMode { tournament_id },
        planner,
        orchestrator_config,
        elo_options.clone(),
        SessionConfig::default(),
        extra_sinks,
    )
    .await
    {
        Ok(session) => session,
        Err(error) => {
            let reason = format!("start session: {error}");
            if let Some(signal) = startup.take() {
                let _ = signal.send(Err(reason.clone()));
            }
            return Err(reason);
        },
    };

    // 5. Subscribe: lossless lifecycle (snapshot + tail), the state watch, and
    //    the lossy per-turn stream for now-playing.
    let (_initial_state, mut events) = session.subscribe();
    let state_rx = session.state();
    let mut live = session.live_events();
    let mut live_open = true;
    let mut now_playing: HashMap<u64, (u16, f32, f32)> = HashMap::new();
    // Seat orientation per in-flight match, learned at MatchStarted. The
    // live `TurnPlayed` scores arrive in seat order (slot 0 = Rat); the
    // frontend keys live rows under the canonical player ids, so a Flipped
    // game's scores must be canonicalized before emitting or the live row
    // would show canonical labels with swapped scores.
    let mut orientation_by_match: HashMap<u64, pyrat_eval::SeatOrientation> = HashMap::new();
    let mut flush = tokio::time::interval(NOW_PLAYING_FLUSH);

    // 6. Announce.
    let _ = TournamentStartedEvent {
        tournament_id: tournament_id.0,
        name,
        format: format_str,
        target,
        total_games,
        games_per_matchup: target_games_per_matchup,
        paired: matches!(seat_policy, SeatPolicy::Paired),
        max_parallel,
        anchor_id: anchor_id.clone(),
        plan_summary,
        players: player_ids
            .iter()
            .map(|id| crate::tournament_events::PlayerLite {
                player_id: id.clone(),
            })
            .collect(),
    }
    .emit(&app);

    // `start_tournament` does not resolve successfully until the session is
    // genuinely live. This closes the command/event gap where a spawned task
    // could fail before `TournamentStartedEvent` and leave the frontend stuck
    // in its global Starting state forever.
    if let Some(signal) = startup.take() {
        let _ = signal.send(Ok(()));
    }

    // 7. Forwarding loop. Verdict-bearing data (scores, standings) comes from
    //    the lossless `events`/state path; now-playing rides the lossy `live`.
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = session.shutdown().await;
                let _ = TournamentAbortedEvent {
                    tournament_id: tournament_id.0,
                    reason: "stopped".into(),
                }
                .emit(&app);
                return Ok(());
            }
            recv = events.recv() => {
                match recv {
                    Ok(SessionEvent::MatchFinished { descriptor }) => {
                        now_playing.remove(&descriptor.match_id.0);
                        orientation_by_match.remove(&descriptor.match_id.0);
                        let state = state_rx.borrow().clone();
                        emit_match_finished(&app, tournament_id.0, &descriptor, &state);
                        emit_standings(&app, tournament_id.0, &state, &elo_options, &player_ids,
                                       &anchor_id, total_games);
                    }
                    Ok(SessionEvent::MatchFailed { descriptor, reason, .. }) => {
                        now_playing.remove(&descriptor.match_id.0);
                        orientation_by_match.remove(&descriptor.match_id.0);
                        let failing_player_id = implicated_player_id(&descriptor, &reason);
                        let (kind, timeout_phase) = failure_kind_for_wire(&reason);
                        // Tell the frontend to drop the now-playing row (a failed
                        // match emits no scored event, so its live row would
                        // otherwise freeze) and accumulate per-bot health.
                        let _ = TournamentMatchFailedEvent {
                            tournament_id: tournament_id.0,
                            match_id: descriptor.match_id.0,
                            player1_id: descriptor.player1_id.clone(),
                            player2_id: descriptor.player2_id.clone(),
                            repetition_index: descriptor.repetition_index,
                            rat_id: rat_id(&descriptor),
                            failing_player_id,
                            kind,
                            timeout_phase,
                            reason: failure_reason_string(&reason),
                        }
                        .emit(&app);
                        let state = state_rx.borrow().clone();
                        emit_standings(&app, tournament_id.0, &state, &elo_options, &player_ids,
                                       &anchor_id, total_games);
                    }
                    Ok(SessionEvent::TournamentFinished) => {
                        let _ = TournamentFinishedEvent { tournament_id: tournament_id.0 }.emit(&app);
                        break;
                    }
                    Ok(SessionEvent::TournamentAborted { reason }) => {
                        let _ = TournamentAbortedEvent { tournament_id: tournament_id.0, reason }
                            .emit(&app);
                        break;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "tournament session events lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            live_recv = live.recv(), if live_open => {
                match live_recv {
                    Ok(OrchestratorEvent::MatchStarted { descriptor, .. }) => {
                        orientation_by_match
                            .insert(descriptor.match_id.0, descriptor.orientation);
                        let _ = TournamentMatchStartedEvent {
                            tournament_id: tournament_id.0,
                            match_id: descriptor.match_id.0,
                            player1_id: descriptor.player1_id.clone(),
                            player2_id: descriptor.player2_id.clone(),
                            repetition_index: descriptor.repetition_index,
                        }
                        .emit(&app);
                    }
                    Ok(OrchestratorEvent::MatchEvent {
                        id,
                        event: MatchEvent::TurnPlayed { state, .. },
                    }) => {
                        // Latest-wins per match; flushed on the interval tick.
                        now_playing.insert(
                            id.0,
                            (state.turn, state.player1_score, state.player2_score),
                        );
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {} // lossy by design
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => live_open = false,
                }
            }
            _ = flush.tick() => {
                for (match_id, (turn, p1_seat, p2_seat)) in now_playing.drain() {
                    // Canonicalize seat-order scores to (lex-min, lex-max) so
                    // the live row's scores match its canonical player labels.
                    let orientation = orientation_by_match
                        .get(&match_id)
                        .copied()
                        .unwrap_or_default();
                    let (player1_score, player2_score) =
                        orientation.canonicalize(p1_seat, p2_seat);
                    let _ = NowPlayingEvent {
                        tournament_id: tournament_id.0,
                        match_id,
                        turn,
                        player1_score,
                        player2_score,
                    }
                    .emit(&app);
                }
            }
        }
    }

    session
        .join()
        .await
        .map_err(|e| format!("session join: {e}"))
}

/// Resolve the bot a failure points at (a timeout or clean disconnect) from the
/// engine seat. The `MatchError` slot is an *engine seat*;
/// `descriptor.orientation` records which canonical id sat there, and
/// `SeatOrientation::canonicalize` is its own inverse (identity or a 2-swap), so
/// applying it to the canonical ids yields the seat assignment to index by slot.
/// `None` for failures with no single seat (spawn, sink, internal). This is the
/// load-bearing attribution step behind the per-bot health marker — get it wrong
/// and a flipped-seat timeout accuses the wrong bot, hence the table test.
fn implicated_player_id(
    descriptor: &EvalMatchDescriptor,
    reason: &FailureReason,
) -> Option<String> {
    let slot = reason.implicated_slot()?;
    let (seat0_id, seat1_id) = descriptor
        .orientation
        .canonicalize(descriptor.player1_id.clone(), descriptor.player2_id.clone());
    Some(if slot == PlayerSlot::Player1 {
        seat0_id
    } else {
        seat1_id
    })
}

/// Map a `FailureReason` to the wire failure kind and, for timeouts, the
/// phase. Payload strings are dropped (the implicated bot rides
/// `failing_player_id`); `FailureReason` is `#[non_exhaustive]`, so the
/// catch-all also covers panic / sink / internal failures as `Other`.
fn failure_kind_for_wire(reason: &FailureReason) -> (FailureKind, Option<TimeoutPhase>) {
    match reason {
        FailureReason::Timeout { phase, .. } => {
            let phase = match phase {
                CoreTimeoutPhase::Setup => TimeoutPhase::Setup,
                CoreTimeoutPhase::Preprocessing => TimeoutPhase::Preprocessing,
                CoreTimeoutPhase::Sync => TimeoutPhase::Sync,
                CoreTimeoutPhase::Move => TimeoutPhase::Move,
            };
            (FailureKind::Timeout, Some(phase))
        },
        FailureReason::Disconnected(_) => (FailureKind::Disconnected, None),
        FailureReason::SpawnFailed => (FailureKind::SpawnFailed, None),
        FailureReason::HandshakeTimeout => (FailureKind::HandshakeTimeout, None),
        FailureReason::ProtocolError(_) => (FailureKind::ProtocolError, None),
        FailureReason::Cancelled => (FailureKind::Cancelled, None),
        // Infra failures — not the bot's fault; surface as a distinct kind.
        FailureReason::Panic | FailureReason::SinkFlushError(_) | FailureReason::Internal(_) => {
            (FailureKind::Internal, None)
        },
        // `FailureReason` is `#[non_exhaustive]`; only genuinely-future variants
        // land here.
        _ => (FailureKind::Other, None),
    }
}

fn emit_match_finished(
    app: &AppHandle,
    tid: i64,
    descriptor: &EvalMatchDescriptor,
    state: &TournamentState,
) {
    let key = MatchupKey::from_descriptor(descriptor);
    let Some((p1, p2)) = canonical_finished_scores(state, &key, descriptor.attempt_index) else {
        return;
    };
    let _ = TournamentMatchFinishedEvent {
        tournament_id: tid,
        player1_id: key.player1_id().to_string(),
        player2_id: key.player2_id().to_string(),
        repetition_index: key.repetition_index(),
        rat_id: rat_id(descriptor),
        player1_score: p1,
        player2_score: p2,
        match_id: descriptor.match_id.0,
    }
    .emit(app);
}

fn rat_id(descriptor: &EvalMatchDescriptor) -> String {
    match descriptor.orientation {
        pyrat_eval::SeatOrientation::Canonical => descriptor.player1_id.clone(),
        pyrat_eval::SeatOrientation::Flipped => descriptor.player2_id.clone(),
    }
}

/// Scores for the just-finished attempt of `key`, read from history — which
/// `TournamentState::apply` stores in **canonical** order (player1_score is
/// the lex-min player's, regardless of who was Rat). This is the contract the
/// frontend's W/L/D dots rely on: the seat fix must keep emitting canonical,
/// never raw seat-order, scores. Returns `None` for a failure attempt.
fn canonical_finished_scores(
    state: &TournamentState,
    key: &MatchupKey,
    attempt_index: u32,
) -> Option<(f64, f64)> {
    state.history.get(key).and_then(|attempts| {
        attempts
            .iter()
            .rev()
            .find(|a| a.attempt_index == attempt_index)
            .and_then(|a| match a.outcome {
                MatchupOutcome::Success {
                    player1_score,
                    player2_score,
                } => Some((player1_score, player2_score)),
                MatchupOutcome::Failure => None,
            })
    })
}

fn emit_standings(
    app: &AppHandle,
    tid: i64,
    state: &TournamentState,
    elo_options: &EloOptions,
    player_ids: &[String],
    anchor_id: &str,
    total: u32,
) {
    let (mut success, mut failure) = (0u32, 0u32);
    for attempts in state.history.values() {
        for a in attempts {
            match a.outcome {
                MatchupOutcome::Success { .. } => success += 1,
                MatchupOutcome::Failure => failure += 1,
            }
        }
    }

    let standings = build_standings(state, elo_options, player_ids, anchor_id);
    let _ = StandingsUpdatedEvent {
        tournament_id: tid,
        done: success,
        total,
        success,
        failure,
        standings,
    }
    .emit(app);
}

/// Build standings rows for every player. Elo + CI come from
/// `compute_elo_with_uncertainty` (the same math as the CLI); players with too
/// few games, or absent from the Elo result (disconnected early), are
/// `pending`. The anchor is never pending.
pub(crate) fn build_standings(
    state: &TournamentState,
    elo_options: &EloOptions,
    player_ids: &[String],
    anchor_id: &str,
) -> Vec<StandingRow> {
    let h2h = state.head_to_head();
    let computed = compute_elo_with_uncertainty(&h2h, elo_options).ok();

    player_ids
        .iter()
        .map(|id| {
            let games = games_for(state, id);
            let is_anchor = id == anchor_id;
            match computed
                .as_ref()
                .and_then(|(res, unc)| res.get_elo(id).map(|elo| (elo, unc.stderr(id))))
            {
                Some((elo, stderr)) => {
                    let half = 1.96 * stderr.unwrap_or(0.0);
                    StandingRow {
                        player_id: id.clone(),
                        elo,
                        elo_ci_low: elo - half,
                        elo_ci_high: elo + half,
                        games,
                        pending: !is_anchor && games < MIN_GAMES_FOR_ESTIMATE,
                    }
                },
                None => StandingRow {
                    player_id: id.clone(),
                    elo: 0.0,
                    elo_ci_low: 0.0,
                    elo_ci_high: 0.0,
                    games,
                    pending: !is_anchor,
                },
            }
        })
        .collect()
}

/// Count completed (successful) games involving `player_id`.
fn games_for(state: &TournamentState, player_id: &str) -> u32 {
    let mut n = 0;
    for (key, attempts) in &state.history {
        if key.player1_id() == player_id || key.player2_id() == player_id {
            n += attempts
                .iter()
                .filter(|a| matches!(a.outcome, MatchupOutcome::Success { .. }))
                .count() as u32;
        }
    }
    n
}

/// Number of games a complete tournament will produce, for the progress bar.
/// `target_per_matchup` is the resolved games-per-matchup (2 × mazes for the
/// paired schedule), no longer a pinned constant.
pub fn total_games(format: &RunnerFormat, player_count: usize, target_per_matchup: u32) -> u32 {
    let matchups = match format {
        RunnerFormat::RoundRobin => player_count * player_count.saturating_sub(1) / 2,
        RunnerFormat::Gauntlet { opponents, .. } => opponents.len(),
    };
    matchups as u32 * target_per_matchup
}

/// Order players canonically for a gauntlet (challenger first), matching the
/// store's slot order so standings and the bot-page agree. Round-robin keeps
/// the given order.
pub fn ordered_player_ids(format: &RunnerFormat, players: &[ResolvedPlayer]) -> Vec<String> {
    match format {
        RunnerFormat::RoundRobin => players.iter().map(|p| p.id.clone()).collect(),
        RunnerFormat::Gauntlet {
            challenger,
            opponents,
        } => match split_gauntlet_players(players, challenger, opponents) {
            Ok((c, ops)) => gauntlet_slot_order(&c, &ops)
                .map(|p| p.id.clone())
                .collect(),
            Err(_) => players.iter().map(|p| p.id.clone()).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat_eval::{MatchupAttempt, MatchupOutcome};

    /// Insert one finished game for a pair, with scores in the player-id order
    /// given (the helper canonicalises to the lex-sorted matchup key).
    fn push_game(
        state: &mut TournamentState,
        a: &str,
        b: &str,
        gcid: &str,
        rep: u32,
        score_a: f64,
        score_b: f64,
    ) {
        let key = MatchupKey::from_pair(a, b, gcid, rep);
        let (p1, p2) = if a <= b {
            (score_a, score_b)
        } else {
            (score_b, score_a)
        };
        state.history.insert(
            key,
            vec![MatchupAttempt {
                attempt_index: 0,
                outcome: MatchupOutcome::Success {
                    player1_score: p1,
                    player2_score: p2,
                },
            }],
        );
    }

    /// Contract: the scores `emit_match_finished` forwards are canonical
    /// (player1_score = lex-min player's), even when the game was played with
    /// flipped seats. The frontend's W/L/D dots invert if this ever emits raw
    /// seat-order scores, so the paired-games seat fix is pinned here
    /// end-to-end: a `Flipped` MatchFinished with seat-order scores must land
    /// in history — and thus in the emit — as canonical order.
    #[test]
    fn finished_scores_are_canonical_even_for_flipped_seating() {
        use pyrat_eval::orchestrator::{DriverEvent, MatchId, MatchOutcome};
        use pyrat_eval::{EvalMatchDescriptor, SeatOrientation};
        use pyrat_host::match_host::MatchResult;
        use pyrat_host::player::PlayerIdentity;
        use pyrat_host::wire::{GameResult, Player};

        // Canonical pair (a, b); b (lex-max) is Rat (slot 0) → Flipped.
        let descriptor = EvalMatchDescriptor {
            match_id: MatchId(0),
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: 0,
            repetition_index: 1,
            attempt_index: 0,
            orientation: SeatOrientation::Flipped,
            planned_at: std::time::SystemTime::UNIX_EPOCH,
        };
        let identity = |slot| PlayerIdentity {
            name: "x".into(),
            author: "x".into(),
            agent_id: "x".into(),
            slot,
        };
        let mut state = TournamentState::empty(TournamentId(1));
        // Seat order: slot 0 (b, Rat) scored 7, slot 1 (a) scored 3.
        state.apply(&DriverEvent::MatchFinished {
            outcome: MatchOutcome {
                descriptor: descriptor.clone(),
                started_at: std::time::SystemTime::UNIX_EPOCH,
                finished_at: std::time::SystemTime::UNIX_EPOCH,
                result: MatchResult {
                    result: GameResult::Player1,
                    player1_score: 7.0,
                    player2_score: 3.0,
                    turns_played: 10,
                },
                players: [identity(Player::Player1), identity(Player::Player2)],
            },
        });

        let key = MatchupKey::from_descriptor(&descriptor);
        let (p1, p2) = canonical_finished_scores(&state, &key, 0).expect("scores present");
        // Canonical: player1 = lex-min "a" scored 3; player2 = "b" scored 7.
        assert_eq!(key.player1_id(), "a");
        assert_eq!((p1, p2), (3.0, 7.0));
    }

    /// The load-bearing attribution behind the health marker: a timeout's
    /// engine slot must resolve to the *canonical* bot through the seat
    /// orientation. The neighboring flipped-seating test covers the *score*
    /// direction of `canonicalize`; this pins the inverse (seat→id) use, which
    /// a regression could silently break (accusing the wrong bot) with no other
    /// test failing.
    #[test]
    fn implicated_player_id_resolves_seat_through_orientation() {
        use pyrat_eval::orchestrator::{FailureReason, MatchId, TimeoutPhase};
        use pyrat_eval::{EvalMatchDescriptor, SeatOrientation};
        use pyrat_host::wire::Player;

        // Canonical pair (a, b): a = lex-min, b = lex-max.
        let desc = |orientation| EvalMatchDescriptor {
            match_id: MatchId(0),
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: 0,
            repetition_index: 0,
            attempt_index: 0,
            orientation,
            planned_at: std::time::SystemTime::UNIX_EPOCH,
        };
        let timeout = |slot| FailureReason::Timeout {
            slot,
            phase: TimeoutPhase::Move,
        };
        let id = |o, slot| implicated_player_id(&desc(o), &timeout(slot));

        // Canonical: engine seat0 = a, seat1 = b.
        assert_eq!(
            id(SeatOrientation::Canonical, Player::Player1).as_deref(),
            Some("a")
        );
        assert_eq!(
            id(SeatOrientation::Canonical, Player::Player2).as_deref(),
            Some("b")
        );
        // Flipped: lex-max (b) was Rat (seat0), so seat0 = b, seat1 = a.
        assert_eq!(
            id(SeatOrientation::Flipped, Player::Player1).as_deref(),
            Some("b")
        );
        assert_eq!(
            id(SeatOrientation::Flipped, Player::Player2).as_deref(),
            Some("a")
        );
        // No implicated slot → no attribution.
        assert_eq!(
            implicated_player_id(
                &desc(SeatOrientation::Canonical),
                &FailureReason::SpawnFailed
            ),
            None
        );
    }

    /// The GUI's standings (elo + CI) must equal a direct
    /// `compute_elo_with_uncertainty` on the same records — the exact call the
    /// CLI's `render_standings` makes. This is the CLI-parity guarantee: same
    /// math on the same store-derived head-to-head records.
    #[test]
    fn build_standings_matches_direct_elo_with_uncertainty() {
        let gcid = "cfg";
        let players = vec![
            "greedy".to_string(),
            "my-bot".to_string(),
            "search".to_string(),
        ];
        let mut state = TournamentState::empty(TournamentId(1));
        // A connected graph across all pairs with a clear strength ordering
        // (greedy > my-bot > search), enough games for a stable fit.
        for rep in 0..8 {
            push_game(&mut state, "greedy", "my-bot", gcid, rep, 7.0, 3.0);
            push_game(&mut state, "my-bot", "search", gcid, rep + 100, 7.0, 3.0);
            push_game(&mut state, "greedy", "search", gcid, rep + 200, 8.0, 2.0);
        }

        let elo_options = tournament_config::elo_options("greedy");
        let rows = build_standings(&state, &elo_options, &players, "greedy");

        let h2h = state.head_to_head();
        let (result, unc) = compute_elo_with_uncertainty(&h2h, &elo_options).unwrap();

        for row in &rows {
            let elo = result.get_elo(&row.player_id).unwrap();
            let stderr = unc.stderr(&row.player_id).unwrap();
            assert!(
                (row.elo - elo).abs() < 1e-9,
                "{} elo {} != direct {elo}",
                row.player_id,
                row.elo,
            );
            let half = 1.96 * stderr;
            assert!((row.elo_ci_low - (elo - half)).abs() < 1e-9);
            assert!((row.elo_ci_high - (elo + half)).abs() < 1e-9);
            assert!(!row.pending, "{} should be rated", row.player_id);
        }
        // Anchor pinned at 1000; ordering preserved.
        let greedy = rows.iter().find(|r| r.player_id == "greedy").unwrap();
        assert!((greedy.elo - 1000.0).abs() < 1e-9);
    }
}
