//! Playing phase: the core turn loop that drives a match to completion.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use pyrat::game::game_logic::GameState;
use pyrat::{Coordinates, Direction as EngineDirection};

use crate::session::messages::{HostCommand, OwnedTurnState, SessionId, SessionMsg};
use crate::wire::{Direction as WireDirection, GameResult, Player};

use super::config::{PlayingConfig, SessionHandle};
use super::events::{emit, MatchEvent};

// ── Public types ─────────────────────────────────────

/// Result of a completed match.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub result: GameResult,
    pub player1_score: f32,
    pub player2_score: f32,
    pub turns_played: u16,
}

/// State that persists across turns during the playing phase.
pub struct PlayingState {
    session_players: HashMap<SessionId, Vec<Player>>,
    disconnected: HashSet<SessionId>,
    last_p1: WireDirection,
    last_p2: WireDirection,
}

impl PlayingState {
    /// Build initial state from the session list returned by setup.
    ///
    /// The same `sessions` slice must be passed to [`run_one_turn`] on every
    /// call — `PlayingState` tracks session IDs internally and assumes
    /// the slice identity is stable.
    pub fn new(sessions: &[SessionHandle]) -> Self {
        let session_players = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();
        Self {
            session_players,
            disconnected: HashSet::new(),
            last_p1: WireDirection::Stay,
            last_p2: WireDirection::Stay,
        }
    }
}

/// Outcome of a single turn.
#[derive(Debug, PartialEq)]
pub enum TurnOutcome {
    /// Game continues — more turns to play.
    Continue,
    /// Game ended this turn — carries the final result.
    GameOver(MatchResult),
}

/// What can go wrong during the playing phase.
#[derive(Debug, thiserror::Error)]
pub enum PlayingError {
    #[error("game channel closed — all sessions gone")]
    AllDisconnected,
}

// ── Direction conversion ─────────────────────────────

/// Convert a wire Direction (u8 newtype) to an engine Direction enum.
///
/// Same discriminant values: Up=0, Right=1, Down=2, Left=3, Stay=4.
fn wire_to_engine(d: WireDirection) -> EngineDirection {
    EngineDirection::try_from(d.0).unwrap_or(EngineDirection::Stay)
}

fn engine_to_wire(d: EngineDirection) -> WireDirection {
    WireDirection(d as u8)
}

// ── Turn loop ────────────────────────────────────────

/// Execute one turn of the playing phase: send turn state, collect actions,
/// step the engine, emit event, check for game over.
///
/// Does **not** send `GameOver` to sessions or emit `MatchOver` — the caller
/// owns end-of-match signaling. See [`run_playing`] for an implementation
/// that handles the full lifecycle.
///
/// With infinite timeout (`move_timeout = Duration::ZERO`), this function
/// blocks until all actions arrive or sessions disconnect. The caller can
/// send [`HostCommand::Stop`] to prompt bots to commit their moves.
pub async fn run_one_turn(
    state: &mut PlayingState,
    game: &mut GameState,
    sessions: &[SessionHandle],
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    config: &PlayingConfig,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
) -> Result<TurnOutcome, PlayingError> {
    let turn_state = build_turn_state(game, state.last_p1, state.last_p2);
    let current_turn = game.turn;

    // Send TurnState to each connected session.
    for s in sessions {
        if !state.disconnected.contains(&s.session_id)
            && s.cmd_tx
                .send(HostCommand::TurnState(Box::new(turn_state.clone())))
                .await
                .is_err()
        {
            debug!(
                session = s.session_id.0,
                "TurnState send failed — marking disconnected"
            );
            state.disconnected.insert(s.session_id);
        }
    }

    // Collect actions.
    let (p1_wire, p2_wire) = collect_actions(
        game_rx,
        current_turn,
        sessions,
        &state.session_players,
        &mut state.disconnected,
        config.move_timeout,
        event_tx,
    )
    .await?;

    // Step the engine.
    let p1_move = wire_to_engine(p1_wire);
    let p2_move = wire_to_engine(p2_wire);
    let result = game.process_turn(p1_move, p2_move);

    state.last_p1 = engine_to_wire(p1_move);
    state.last_p2 = engine_to_wire(p2_move);

    // Emit TurnPlayed event.
    emit(
        event_tx,
        MatchEvent::TurnPlayed {
            state: build_turn_state(game, state.last_p1, state.last_p2),
            p1_action: p1_wire,
            p2_action: p2_wire,
        },
    );

    if result.game_over {
        Ok(TurnOutcome::GameOver(determine_result(game)))
    } else {
        Ok(TurnOutcome::Continue)
    }
}

/// Run the playing phase: send turn state, collect actions, step the engine,
/// check for game over, repeat.
///
/// Returns `MatchResult` when the game ends (cheese collected, max turns, or
/// score majority). Returns `PlayingError::AllDisconnected` only if the
/// channel itself closes (all senders dropped).
pub async fn run_playing(
    game: &mut GameState,
    sessions: &[SessionHandle],
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    config: &PlayingConfig,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
) -> Result<MatchResult, PlayingError> {
    let mut state = PlayingState::new(sessions);

    let match_result = loop {
        match run_one_turn(&mut state, game, sessions, game_rx, config, event_tx).await? {
            TurnOutcome::Continue => continue,
            TurnOutcome::GameOver(result) => break result,
        }
    };

    // Send GameOver to all connected sessions.
    for s in sessions {
        if !state.disconnected.contains(&s.session_id)
            && s.cmd_tx
                .send(HostCommand::GameOver {
                    result: match_result.result,
                    player1_score: match_result.player1_score,
                    player2_score: match_result.player2_score,
                })
                .await
                .is_err()
        {
            debug!(
                session = s.session_id.0,
                "GameOver send failed — session already gone"
            );
        }
    }

    emit(
        event_tx,
        MatchEvent::MatchOver {
            result: match_result.clone(),
        },
    );

    Ok(match_result)
}

// ── Helpers ──────────────────────────────────────────

fn build_turn_state(
    game: &GameState,
    last_p1: WireDirection,
    last_p2: WireDirection,
) -> OwnedTurnState {
    let p1 = &game.player1;
    let p2 = &game.player2;
    OwnedTurnState {
        turn: game.turn,
        player1_position: (p1.current_pos.x, p1.current_pos.y),
        player2_position: (p2.current_pos.x, p2.current_pos.y),
        player1_score: p1.score,
        player2_score: p2.score,
        player1_mud_turns: p1.mud_timer,
        player2_mud_turns: p2.mud_timer,
        cheese: game
            .cheese
            .get_all_cheese_positions()
            .into_iter()
            .map(|c: Coordinates| (c.x, c.y))
            .collect(),
        player1_last_move: last_p1,
        player2_last_move: last_p2,
    }
}

/// Collect one action per player for this turn.
///
/// Pre-fills STAY for disconnected players. Uses a `select!` loop to receive
/// actions or detect disconnects/timeouts. First valid action per player wins.
async fn collect_actions(
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    current_turn: u16,
    sessions: &[SessionHandle],
    session_players: &HashMap<SessionId, Vec<Player>>,
    disconnected: &mut HashSet<SessionId>,
    move_timeout: Duration,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
) -> Result<(WireDirection, WireDirection), PlayingError> {
    let stay = WireDirection::Stay;
    let mut p1_action: Option<WireDirection> = None;
    let mut p2_action: Option<WireDirection> = None;

    // Pre-fill for disconnected players.
    for sid in disconnected.iter() {
        if let Some(players) = session_players.get(sid) {
            for &p in players {
                fill_action(p, stay, &mut p1_action, &mut p2_action);
            }
        }
    }

    // Track which sessions have responded (sent at least one action this turn).
    let mut responded: HashSet<SessionId> = HashSet::new();

    if both_filled(p1_action, p2_action) {
        return Ok((p1_action.unwrap(), p2_action.unwrap()));
    }

    // Duration::ZERO = infinite timeout (no deadline, wait for actions or disconnects).
    let infinite = move_timeout.is_zero();
    let deadline = Instant::now() + move_timeout;

    loop {
        tokio::select! {
            msg = game_rx.recv() => {
                let Some(msg) = msg else {
                    // Channel closed — all senders gone.
                    return Err(PlayingError::AllDisconnected);
                };
                match msg {
                    SessionMsg::Action {
                        session_id,
                        player,
                        direction,
                        turn,
                    } => {
                        if turn != current_turn {
                            debug!(turn, current_turn, "stale action ignored");
                            continue;
                        }
                        responded.insert(session_id);
                        fill_action(player, direction, &mut p1_action, &mut p2_action);
                    }
                    SessionMsg::Disconnected { session_id, reason } => {
                        debug!(session = session_id.0, ?reason, "session disconnected during play");
                        disconnected.insert(session_id);
                        if let Some(players) = session_players.get(&session_id) {
                            for &p in players {
                                fill_action(p, stay, &mut p1_action, &mut p2_action);
                                emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
                            }
                        }
                    }
                    SessionMsg::Info { session_id, info } => {
                        if let Some(players) = session_players.get(&session_id) {
                            // Hivemind sessions control multiple players — emit one
                            // BotInfo per controlled player so consumers see info
                            // attributed to each.
                            for &p in players {
                                emit(event_tx, MatchEvent::BotInfo {
                                    player: p,
                                    turn: current_turn,
                                    info: info.clone(),
                                });
                            }
                        }
                    }
                    _ => {
                        // Ignore other messages during playing phase.
                    }
                }

                if both_filled(p1_action, p2_action) {
                    return Ok((p1_action.unwrap(), p2_action.unwrap()));
                }
            }
            _ = tokio::time::sleep_until(deadline), if !infinite => {
                // Timeout: fill remaining with STAY, notify timed-out sessions.
                debug!(turn = current_turn, "move timeout — defaulting remaining players to STAY");
                if p1_action.is_none() {
                    p1_action = Some(stay);
                }
                if p2_action.is_none() {
                    p2_action = Some(stay);
                }

                // Send Timeout only to sessions that didn't respond.
                for s in sessions {
                    if !disconnected.contains(&s.session_id) && !responded.contains(&s.session_id) {
                        let _ = s.cmd_tx.send(HostCommand::Timeout {
                            default_move: stay,
                        }).await;
                        for &p in &s.controlled_players {
                            emit(event_tx, MatchEvent::BotTimeout { player: p, turn: current_turn });
                        }
                    }
                }

                return Ok((p1_action.unwrap(), p2_action.unwrap()));
            }
        }
    }
}

/// Insert a direction for the given player, first-wins.
fn fill_action(
    player: Player,
    direction: WireDirection,
    p1: &mut Option<WireDirection>,
    p2: &mut Option<WireDirection>,
) {
    match player {
        Player::Player1 => {
            if p1.is_none() {
                *p1 = Some(direction);
            }
        },
        Player::Player2 => {
            if p2.is_none() {
                *p2 = Some(direction);
            }
        },
        _ => {
            warn!(player = player.0, "unknown player in action");
        },
    }
}

fn both_filled(p1: Option<WireDirection>, p2: Option<WireDirection>) -> bool {
    p1.is_some() && p2.is_some()
}

fn determine_result(game: &GameState) -> MatchResult {
    let p1 = game.player1.score;
    let p2 = game.player2.score;
    let result = if p1 > p2 {
        GameResult::Player1
    } else if p2 > p1 {
        GameResult::Player2
    } else {
        GameResult::Draw
    };
    MatchResult {
        result,
        player1_score: p1,
        player2_score: p2,
        turns_played: game.turn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::messages::{HostCommand, SessionId, SessionMsg};
    use crate::wire::{Direction as WireDirection, Player};
    use std::collections::{HashMap, HashSet};
    use std::time::Duration;
    use tokio::sync::mpsc;

    /// Stale action (wrong turn number) is silently dropped; the player times out.
    #[tokio::test]
    async fn stale_action_is_ignored() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);

        // Two sessions, each controlling one player.
        let (cmd_tx1, mut cmd_rx1) = mpsc::channel::<HostCommand>(16);
        let (cmd_tx2, mut cmd_rx2) = mpsc::channel::<HostCommand>(16);

        let sessions = vec![
            SessionHandle {
                session_id: SessionId(1),
                cmd_tx: cmd_tx1,
                name: "Bot1".into(),
                author: "A".into(),
                agent_id: "bot-1".into(),
                controlled_players: vec![Player::Player1],
            },
            SessionHandle {
                session_id: SessionId(2),
                cmd_tx: cmd_tx2,
                name: "Bot2".into(),
                author: "B".into(),
                agent_id: "bot-2".into(),
                controlled_players: vec![Player::Player2],
            },
        ];

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 5;

        // P1: correct turn → accepted.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Right,
                turn: current_turn,
            })
            .await
            .unwrap();

        // P2: stale turn (3 != 5) → ignored, will timeout.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Right,
                turn: 3,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let (p1, p2) = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            Duration::from_millis(100),
            None,
        )
        .await
        .expect("collect_actions should not fail");

        // P1 got Right (accepted), P2 got Stay (stale → timeout default).
        assert_eq!(p1, WireDirection::Right);
        assert_eq!(p2, WireDirection::Stay);

        // Session 2 should have received a Timeout command.
        let cmd = cmd_rx2.try_recv().expect("session 2 should get Timeout");
        assert!(
            matches!(cmd, HostCommand::Timeout { default_move } if default_move == WireDirection::Stay),
            "expected Timeout with Stay, got {cmd:?}"
        );

        // Session 1 responded, so no Timeout for it.
        assert!(
            cmd_rx1.try_recv().is_err(),
            "session 1 should not receive Timeout"
        );
    }

    /// Zero timeout = infinite mode: Info relayed as BotInfo, actions collected, no timeout.
    #[tokio::test]
    async fn infinite_timeout_collects_actions_and_relays_info() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);

        let (cmd_tx1, _cmd_rx1) = mpsc::channel::<HostCommand>(16);
        let (cmd_tx2, _cmd_rx2) = mpsc::channel::<HostCommand>(16);

        let sessions = vec![
            SessionHandle {
                session_id: SessionId(1),
                cmd_tx: cmd_tx1,
                name: "Bot1".into(),
                author: "A".into(),
                agent_id: "bot-1".into(),
                controlled_players: vec![Player::Player1],
            },
            SessionHandle {
                session_id: SessionId(2),
                cmd_tx: cmd_tx2,
                name: "Bot2".into(),
                author: "B".into(),
                agent_id: "bot-2".into(),
                controlled_players: vec![Player::Player2],
            },
        ];

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 1;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

        // Bot1 sends Info first.
        game_tx
            .send(SessionMsg::Info {
                session_id: SessionId(1),
                info: crate::session::messages::OwnedInfo {
                    target: None,
                    depth: 5,
                    nodes: 100,
                    score: 0.5,
                    path: vec![],
                    message: "thinking".into(),
                },
            })
            .await
            .unwrap();

        // Then both players send actions.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Up,
                turn: current_turn,
            })
            .await
            .unwrap();
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Down,
                turn: current_turn,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let (p1, p2) = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            Duration::ZERO, // infinite
            Some(&event_tx),
        )
        .await
        .expect("collect_actions should not fail");

        assert_eq!(p1, WireDirection::Up);
        assert_eq!(p2, WireDirection::Down);

        // Info should have been relayed as BotInfo event.
        let event = event_rx.try_recv().expect("should have BotInfo event");
        assert!(
            matches!(
                event,
                MatchEvent::BotInfo {
                    player: Player::Player1,
                    turn: 1,
                    ..
                }
            ),
            "expected BotInfo for Player1, got {event:?}"
        );
    }

    /// Zero timeout = infinite mode: disconnect fills STAY for the disconnected player.
    #[tokio::test]
    async fn disconnect_during_infinite_timeout_fills_stay() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);

        let (cmd_tx1, _cmd_rx1) = mpsc::channel::<HostCommand>(16);
        let (cmd_tx2, _cmd_rx2) = mpsc::channel::<HostCommand>(16);

        let sessions = vec![
            SessionHandle {
                session_id: SessionId(1),
                cmd_tx: cmd_tx1,
                name: "Bot1".into(),
                author: "A".into(),
                agent_id: "bot-1".into(),
                controlled_players: vec![Player::Player1],
            },
            SessionHandle {
                session_id: SessionId(2),
                cmd_tx: cmd_tx2,
                name: "Bot2".into(),
                author: "B".into(),
                agent_id: "bot-2".into(),
                controlled_players: vec![Player::Player2],
            },
        ];

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 0;

        // P1 disconnects.
        game_tx
            .send(SessionMsg::Disconnected {
                session_id: SessionId(1),
                reason: crate::session::messages::DisconnectReason::PeerClosed,
            })
            .await
            .unwrap();

        // P2 sends a valid action.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Down,
                turn: current_turn,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let (p1, p2) = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            Duration::ZERO, // infinite
            None,
        )
        .await
        .expect("collect_actions should not fail");

        // Disconnected player gets STAY, other player's action is used.
        assert_eq!(p1, WireDirection::Stay);
        assert_eq!(p2, WireDirection::Down);
        assert!(disconnected.contains(&SessionId(1)));
    }
}
