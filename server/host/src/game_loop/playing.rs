//! Playing phase: the core turn loop that drives a match to completion.

use std::collections::{HashMap, HashSet};

use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use pyrat::game::game_logic::GameState;
use pyrat::{Coordinates, Direction as EngineDirection};

use crate::session::messages::{HostCommand, OwnedTurnState, SessionId, SessionMsg};
use pyrat_wire::{Direction as WireDirection, GameResult, Player};

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

    /// Build the turn state for the current game position.
    pub fn build_turn_state(&self, game: &GameState) -> OwnedTurnState {
        build_turn_state(game, self.last_p1, self.last_p2)
    }

    /// Record the actions taken this turn (updates last moves for next turn state).
    pub fn record_actions(&mut self, p1: WireDirection, p2: WireDirection) {
        self.last_p1 = p1;
        self.last_p2 = p2;
    }

    /// Map of session ID to controlled players.
    pub fn session_players(&self) -> &HashMap<SessionId, Vec<Player>> {
        &self.session_players
    }

    /// Set of disconnected session IDs.
    pub fn disconnected(&self) -> &HashSet<SessionId> {
        &self.disconnected
    }

    /// Mutable access to disconnected set.
    pub fn disconnected_mut(&mut self) -> &mut HashSet<SessionId> {
        &mut self.disconnected
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

/// Actions collected for a single turn, including timing metadata.
struct CollectedActions {
    p1: WireDirection,
    p2: WireDirection,
    p1_think_ms: u32,
    p2_think_ms: u32,
}

// ── Direction conversion ─────────────────────────────

/// Convert a wire Direction (u8 newtype) to an engine Direction enum.
///
/// Same discriminant values: Up=0, Right=1, Down=2, Left=3, Stay=4.
pub fn wire_to_engine(d: WireDirection) -> EngineDirection {
    EngineDirection::try_from(d.0).unwrap_or(EngineDirection::Stay)
}

pub fn engine_to_wire(d: EngineDirection) -> WireDirection {
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
    let actions = collect_actions(
        game_rx,
        current_turn,
        sessions,
        &state.session_players,
        &mut state.disconnected,
        config,
        event_tx,
    )
    .await?;

    // Step the engine.
    let p1_move = wire_to_engine(actions.p1);
    let p2_move = wire_to_engine(actions.p2);
    let result = game.process_turn(p1_move, p2_move);

    state.last_p1 = engine_to_wire(p1_move);
    state.last_p2 = engine_to_wire(p2_move);

    // Emit TurnPlayed event.
    emit(
        event_tx,
        MatchEvent::TurnPlayed {
            state: build_turn_state(game, state.last_p1, state.last_p2),
            p1_action: actions.p1,
            p2_action: actions.p2,
            p1_think_ms: actions.p1_think_ms,
            p2_think_ms: actions.p2_think_ms,
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
/// Pre-fills STAY for disconnected players. Supports provisional actions
/// (best-so-far) and judges committed actions by `think_ms` against the
/// configured think margin, with a network grace period for packet delivery.
async fn collect_actions(
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    current_turn: u16,
    sessions: &[SessionHandle],
    session_players: &HashMap<SessionId, Vec<Player>>,
    disconnected: &mut HashSet<SessionId>,
    config: &PlayingConfig,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
) -> Result<CollectedActions, PlayingError> {
    let stay = WireDirection::Stay;
    let mut p1_slot: Option<ActionSlot> = None;
    let mut p2_slot: Option<ActionSlot> = None;

    // Pre-fill committed Stay for disconnected players.
    for sid in disconnected.iter() {
        if let Some(players) = session_players.get(sid) {
            for &p in players {
                let slot = match p {
                    Player::Player1 => &mut p1_slot,
                    Player::Player2 => &mut p2_slot,
                    _ => continue,
                };
                update_action(slot, stay, false, 0);
            }
        }
    }

    // Track which sessions have committed an accepted action this turn.
    let mut responded: HashSet<SessionId> = HashSet::new();

    if both_committed(&p1_slot, &p2_slot) {
        return Ok(CollectedActions {
            p1: resolve_action(&p1_slot),
            p2: resolve_action(&p2_slot),
            p1_think_ms: resolve_think_ms(&p1_slot),
            p2_think_ms: resolve_think_ms(&p2_slot),
        });
    }

    // Duration::ZERO = infinite timeout (no deadline, wait for actions or disconnects).
    let move_timeout = config.move_timeout;
    let infinite = move_timeout.is_zero();

    // Think threshold: move_timeout_ms × (1 + think_margin)
    let move_timeout_ms = move_timeout.as_millis() as u32;
    let think_threshold_ms = (move_timeout_ms as f64 * (1.0 + config.think_margin as f64)) as u32;

    // Hard deadline = think deadline + network grace
    let deadline =
        Instant::now() + move_timeout.mul_f32(1.0 + config.think_margin) + config.network_grace;

    loop {
        tokio::select! {
            msg = game_rx.recv() => {
                let Some(msg) = msg else {
                    return Err(PlayingError::AllDisconnected);
                };
                match msg {
                    SessionMsg::Action {
                        session_id,
                        player,
                        direction,
                        turn,
                        provisional,
                        think_ms,
                    } => {
                        if turn != current_turn {
                            debug!(turn, current_turn, "stale action ignored");
                            continue;
                        }

                        let slot = match player {
                            Player::Player1 => &mut p1_slot,
                            Player::Player2 => &mut p2_slot,
                            _ => {
                                warn!(player = player.0, "unknown player in action");
                                continue;
                            },
                        };

                        if provisional {
                            // Provisional: overwrite freely, don't count as responded.
                            update_action(slot, direction, true, think_ms);
                        } else if !infinite && (think_ms == 0 || think_ms > think_threshold_ms) {
                            // Committed but rejected: think_ms missing or over threshold.
                            // Don't lock slot — latest provisional remains as fallback.
                            debug!(
                                player = player.0,
                                think_ms,
                                think_threshold_ms,
                                "committed action rejected — think_ms out of range"
                            );
                        } else {
                            // Committed and accepted.
                            update_action(slot, direction, false, think_ms);
                            responded.insert(session_id);
                        }
                    }
                    SessionMsg::Disconnected { session_id, reason } => {
                        debug!(session = session_id.0, ?reason, "session disconnected during play");
                        disconnected.insert(session_id);
                        if let Some(players) = session_players.get(&session_id) {
                            for &p in players {
                                let slot = match p {
                                    Player::Player1 => &mut p1_slot,
                                    Player::Player2 => &mut p2_slot,
                                    _ => continue,
                                };
                                update_action(slot, stay, false, 0);
                                emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
                            }
                        }
                    }
                    SessionMsg::Info { session_id, info } => {
                        if let Some(players) = session_players.get(&session_id) {
                            if let Some(&sender) = players.first() {
                                emit(event_tx, MatchEvent::BotInfo {
                                    sender,
                                    turn: current_turn,
                                    info,
                                });
                            }
                        }
                    }
                    _ => {}
                }

                if both_committed(&p1_slot, &p2_slot) {
                    return Ok(CollectedActions {
                        p1: resolve_action(&p1_slot),
                        p2: resolve_action(&p2_slot),
                        p1_think_ms: resolve_think_ms(&p1_slot),
                        p2_think_ms: resolve_think_ms(&p2_slot),
                    });
                }
            }
            _ = tokio::time::sleep_until(deadline), if !infinite => {
                // Hard deadline: use whatever we have (provisional or Stay).
                debug!(turn = current_turn, "move timeout — using provisional or STAY");

                // Emit BotTimeout + send Timeout command for sessions that didn't respond.
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

                return Ok(CollectedActions {
                    p1: resolve_action(&p1_slot),
                    p2: resolve_action(&p2_slot),
                    p1_think_ms: resolve_think_ms(&p1_slot),
                    p2_think_ms: resolve_think_ms(&p2_slot),
                });
            }
        }
    }
}

/// Per-player action state during action collection.
struct ActionSlot {
    direction: WireDirection,
    committed: bool,
    think_ms: u32,
}

/// Update a player's action slot. Provisional actions overwrite freely;
/// committed actions lock the slot.
fn update_action(
    slot: &mut Option<ActionSlot>,
    direction: WireDirection,
    provisional: bool,
    think_ms: u32,
) {
    match slot {
        Some(existing) if existing.committed => {
            // Already committed — ignore further actions.
        },
        _ => {
            *slot = Some(ActionSlot {
                direction,
                committed: !provisional,
                think_ms,
            });
        },
    }
}

/// True when both slots have a committed action.
fn both_committed(p1: &Option<ActionSlot>, p2: &Option<ActionSlot>) -> bool {
    matches!(
        (p1, p2),
        (
            Some(ActionSlot {
                committed: true,
                ..
            }),
            Some(ActionSlot {
                committed: true,
                ..
            })
        )
    )
}

/// Resolve a slot to a direction: committed > provisional > Stay.
fn resolve_action(slot: &Option<ActionSlot>) -> WireDirection {
    slot.as_ref()
        .map(|s| s.direction)
        .unwrap_or(WireDirection::Stay)
}

/// Extract think_ms from a slot (0 if absent).
fn resolve_think_ms(slot: &Option<ActionSlot>) -> u32 {
    slot.as_ref().map(|s| s.think_ms).unwrap_or(0)
}

pub fn determine_result(game: &GameState) -> MatchResult {
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
    use pyrat_wire::{Direction as WireDirection, Player};
    use std::collections::{HashMap, HashSet};
    use std::time::Duration;
    use tokio::sync::mpsc;

    fn test_config(move_timeout: Duration) -> PlayingConfig {
        PlayingConfig {
            move_timeout,
            think_margin: 0.10,
            network_grace: Duration::from_millis(50),
        }
    }

    fn make_sessions() -> (
        Vec<SessionHandle>,
        mpsc::Receiver<HostCommand>,
        mpsc::Receiver<HostCommand>,
    ) {
        let (cmd_tx1, cmd_rx1) = mpsc::channel::<HostCommand>(16);
        let (cmd_tx2, cmd_rx2) = mpsc::channel::<HostCommand>(16);
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
        (sessions, cmd_rx1, cmd_rx2)
    }

    /// Stale action (wrong turn number) is silently dropped; the player times out.
    #[tokio::test]
    async fn stale_action_is_ignored() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, mut cmd_rx1, mut cmd_rx2) = make_sessions();

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
                provisional: false,
                think_ms: 50,
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
                provisional: false,
                think_ms: 50,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let config = test_config(Duration::from_millis(100));
        let actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            None,
        )
        .await
        .expect("collect_actions should not fail");

        // P1 got Right (accepted), P2 got Stay (stale → timeout default).
        assert_eq!(actions.p1, WireDirection::Right);
        assert_eq!(actions.p2, WireDirection::Stay);

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
    /// In infinite mode, think_ms=0 is accepted (no timing enforcement).
    #[tokio::test]
    async fn infinite_timeout_collects_actions_and_relays_info() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, _cmd_rx1, _cmd_rx2) = make_sessions();

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
                    player: Player::Player1,
                    multipv: 1,
                    target: None,
                    depth: 5,
                    nodes: 100,
                    score: Some(0.5),
                    pv: vec![],
                    message: "thinking".into(),
                },
            })
            .await
            .unwrap();

        // Then both players send committed actions (think_ms=0 OK in infinite mode).
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Up,
                turn: current_turn,
                provisional: false,
                think_ms: 0,
            })
            .await
            .unwrap();
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Down,
                turn: current_turn,
                provisional: false,
                think_ms: 0,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let config = test_config(Duration::ZERO); // infinite
        let actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            Some(&event_tx),
        )
        .await
        .expect("collect_actions should not fail");

        assert_eq!(actions.p1, WireDirection::Up);
        assert_eq!(actions.p2, WireDirection::Down);

        // Info should have been relayed as BotInfo event.
        let event = event_rx.try_recv().expect("should have BotInfo event");
        assert!(
            matches!(
                event,
                MatchEvent::BotInfo {
                    sender: Player::Player1,
                    turn: 1,
                    ..
                }
            ),
            "expected BotInfo from Player1, got {event:?}"
        );
    }

    /// Zero timeout = infinite mode: disconnect fills STAY for the disconnected player.
    #[tokio::test]
    async fn disconnect_during_infinite_timeout_fills_stay() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, _cmd_rx1, _cmd_rx2) = make_sessions();

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
                provisional: false,
                think_ms: 0,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let config = test_config(Duration::ZERO); // infinite
        let actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            None,
        )
        .await
        .expect("collect_actions should not fail");

        // Disconnected player gets STAY, other player's action is used.
        assert_eq!(actions.p1, WireDirection::Stay);
        assert_eq!(actions.p2, WireDirection::Down);
        assert!(disconnected.contains(&SessionId(1)));
    }

    /// Provisional action is used as fallback when committed action is rejected.
    #[tokio::test]
    async fn provisional_action_used_as_fallback() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, _cmd_rx1, _cmd_rx2) = make_sessions();

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 1;

        // P1 sends provisional Left, then committed with think_ms=0 (rejected).
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Left,
                turn: current_turn,
                provisional: true,
                think_ms: 0,
            })
            .await
            .unwrap();
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Up,
                turn: current_turn,
                provisional: false,
                think_ms: 0, // rejected — missing think_ms
            })
            .await
            .unwrap();

        // P2 sends a valid committed action.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Down,
                turn: current_turn,
                provisional: false,
                think_ms: 50,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let config = test_config(Duration::from_millis(100));
        let actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            None,
        )
        .await
        .expect("collect_actions should not fail");

        // P1 gets provisional Left (committed was rejected), P2 gets committed Down.
        assert_eq!(actions.p1, WireDirection::Left);
        assert_eq!(actions.p2, WireDirection::Down);
    }

    /// Committed action within think margin is accepted.
    #[tokio::test]
    async fn committed_action_within_margin_accepted() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, _cmd_rx1, _cmd_rx2) = make_sessions();

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 1;
        // Config: 1000ms timeout, 10% margin → threshold = 1100ms
        let config = PlayingConfig {
            move_timeout: Duration::from_millis(1000),
            think_margin: 0.10,
            network_grace: Duration::from_millis(50),
        };

        // P1: think_ms=1050 (within 1100 threshold) → accepted
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: WireDirection::Right,
                turn: current_turn,
                provisional: false,
                think_ms: 1050,
            })
            .await
            .unwrap();

        // P2: think_ms=1200 (over 1100 threshold) → rejected, falls back to Stay
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(2),
                player: Player::Player2,
                direction: WireDirection::Left,
                turn: current_turn,
                provisional: false,
                think_ms: 1200,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            None,
        )
        .await
        .expect("collect_actions should not fail");

        assert_eq!(actions.p1, WireDirection::Right);
        // P2's committed was rejected, no provisional → Stay at timeout
        assert_eq!(actions.p2, WireDirection::Stay);
    }
}
