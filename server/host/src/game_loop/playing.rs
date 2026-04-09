//! Playing phase: the core turn loop that drives a match to completion.

use std::collections::{HashMap, HashSet};

use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use pyrat::game::game_logic::GameState;
use pyrat::Direction;

use crate::session::messages::{
    HashedTurnState, HostCommand, OwnedTurnState, SessionId, SessionMsg,
};
use pyrat_wire::{GameResult, Player};

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
    last_p1: Direction,
    last_p2: Direction,
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
            last_p1: Direction::Stay,
            last_p2: Direction::Stay,
        }
    }

    /// Build the turn state for the current game position.
    pub fn build_turn_state(&self, game: &GameState) -> HashedTurnState {
        build_turn_state(game, self.last_p1, self.last_p2)
    }

    /// Record the actions taken this turn (updates last moves for next turn state).
    pub fn record_actions(&mut self, p1: Direction, p2: Direction) {
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
    p1: Direction,
    p2: Direction,
    p1_think_ms: u32,
    p2_think_ms: u32,
    /// Host-measured wall time from TurnState send to committed action receive.
    p1_wall_ms: u32,
    p2_wall_ms: u32,
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
#[tracing::instrument(level = "debug", name = "turn", skip_all, fields(turn = game.turn))]
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

    let send_time = Instant::now();

    // Collect actions.
    let actions = collect_actions(
        game_rx,
        current_turn,
        sessions,
        &state.session_players,
        &mut state.disconnected,
        config,
        event_tx,
        send_time,
    )
    .await?;

    // Warn about slow responses (>80% of timeout).
    if !config.move_timeout.is_zero() {
        let slow_threshold_ms = config.move_timeout.mul_f32(0.8).as_millis() as u32;
        if actions.p1_wall_ms > slow_threshold_ms {
            warn!(
                player = 1,
                wall_ms = actions.p1_wall_ms,
                threshold_ms = slow_threshold_ms,
                "slow response"
            );
        }
        if actions.p2_wall_ms > slow_threshold_ms {
            warn!(
                player = 2,
                wall_ms = actions.p2_wall_ms,
                threshold_ms = slow_threshold_ms,
                "slow response"
            );
        }
    }

    // Step the engine.
    let result = game.process_turn(actions.p1, actions.p2);

    state.last_p1 = actions.p1;
    state.last_p2 = actions.p2;

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

fn build_turn_state(game: &GameState, last_p1: Direction, last_p2: Direction) -> HashedTurnState {
    let p1 = &game.player1;
    let p2 = &game.player2;
    let hash = game.state_hash();
    HashedTurnState::with_hash(
        OwnedTurnState {
            turn: game.turn,
            player1_position: p1.current_pos,
            player2_position: p2.current_pos,
            player1_score: p1.score,
            player2_score: p2.score,
            player1_mud_turns: p1.mud_timer,
            player2_mud_turns: p2.mud_timer,
            cheese: game.cheese.get_all_cheese_positions(),
            player1_last_move: last_p1,
            player2_last_move: last_p2,
        },
        hash,
    )
}

/// Timing policy for evaluating committed actions.
struct ThinkPolicy {
    /// True when the move timeout is zero (infinite mode).
    infinite: bool,
    /// Maximum acceptable `think_ms` value for a committed action.
    threshold_ms: u32,
}

/// Collect one action per player for this turn.
///
/// Pre-fills STAY for disconnected players. Supports provisional actions
/// (best-so-far) and judges committed actions by `think_ms` against the
/// configured think margin, with a network grace period for packet delivery.
#[allow(clippy::too_many_arguments)]
async fn collect_actions(
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    current_turn: u16,
    sessions: &[SessionHandle],
    session_players: &HashMap<SessionId, Vec<Player>>,
    disconnected: &mut HashSet<SessionId>,
    config: &PlayingConfig,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
    send_time: Instant,
) -> Result<CollectedActions, PlayingError> {
    let stay = Direction::Stay;
    let mut p1_slot: Option<ActionSlot> = None;
    let mut p2_slot: Option<ActionSlot> = None;
    let mut p1_wall_ms: u32 = 0;
    let mut p2_wall_ms: u32 = 0;

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
        return Ok(resolve_collected(
            &p1_slot, &p2_slot, p1_wall_ms, p2_wall_ms,
        ));
    }

    // Duration::ZERO = infinite timeout (no deadline, wait for actions or disconnects).
    let move_timeout = config.move_timeout;
    let move_timeout_ms = move_timeout.as_millis() as u32;
    let think = ThinkPolicy {
        infinite: move_timeout.is_zero(),
        threshold_ms: (move_timeout_ms as f64 * (1.0 + config.think_margin as f64)) as u32,
    };

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
                        let before = responded.len();
                        handle_action(
                            &mut p1_slot,
                            &mut p2_slot,
                            &mut responded,
                            session_id,
                            player,
                            direction,
                            turn,
                            current_turn,
                            provisional,
                            think_ms,
                            &think,
                        );
                        // Track wall time when a committed action was accepted.
                        if responded.len() > before {
                            let wall_ms = send_time.elapsed().as_millis() as u32;
                            match player {
                                Player::Player1 => p1_wall_ms = wall_ms,
                                Player::Player2 => p2_wall_ms = wall_ms,
                                _ => {}
                            }
                            debug!(player = player.0, think_ms, wall_ms, "action accepted");
                        }
                    }
                    SessionMsg::Disconnected { session_id, reason } => {
                        handle_disconnect(
                            &mut p1_slot,
                            &mut p2_slot,
                            disconnected,
                            session_players,
                            event_tx,
                            session_id,
                            reason,
                        );
                    }
                    SessionMsg::Info { session_id, info } => {
                        handle_info(session_players, event_tx, session_id, info);
                    }
                    _ => {}
                }

                if both_committed(&p1_slot, &p2_slot) {
                    return Ok(resolve_collected(&p1_slot, &p2_slot, p1_wall_ms, p2_wall_ms));
                }
            }
            _ = tokio::time::sleep_until(deadline), if !think.infinite => {
                debug!(turn = current_turn, "move timeout — using provisional or STAY");
                handle_timeout(sessions, disconnected, &responded, event_tx, current_turn, stay).await;
                return Ok(resolve_collected(&p1_slot, &p2_slot, p1_wall_ms, p2_wall_ms));
            }
        }
    }
}

// ── Message handlers ─────────────────────────────────

/// Process an incoming Action message: stale-turn check, slot dispatch,
/// provisional/committed/rejected branching, responded tracking.
#[allow(clippy::too_many_arguments)]
fn handle_action(
    p1_slot: &mut Option<ActionSlot>,
    p2_slot: &mut Option<ActionSlot>,
    responded: &mut HashSet<SessionId>,
    session_id: SessionId,
    player: Player,
    direction: Direction,
    turn: u16,
    current_turn: u16,
    provisional: bool,
    think_ms: u32,
    think: &ThinkPolicy,
) {
    if turn != current_turn {
        debug!(turn, current_turn, "stale action ignored");
        return;
    }

    let slot = match player {
        Player::Player1 => p1_slot,
        Player::Player2 => p2_slot,
        _ => {
            warn!(player = player.0, "unknown player in action");
            return;
        },
    };

    if provisional {
        update_action(slot, direction, true, think_ms);
    } else if !think.infinite && (think_ms == 0 || think_ms > think.threshold_ms) {
        if think_ms == 0 {
            warn!(
                player = player.0,
                "action rejected: think_ms is 0 (must report actual thinking time), falling back to provisional or STAY"
            );
        } else {
            warn!(
                player = player.0,
                think_ms,
                threshold_ms = think.threshold_ms,
                "action rejected: think_ms exceeds threshold, falling back to provisional or STAY"
            );
        }
    } else {
        update_action(slot, direction, false, think_ms);
        responded.insert(session_id);
    }
}

/// Mark a session disconnected, fill STAY for its players, emit BotDisconnected.
fn handle_disconnect(
    p1_slot: &mut Option<ActionSlot>,
    p2_slot: &mut Option<ActionSlot>,
    disconnected: &mut HashSet<SessionId>,
    session_players: &HashMap<SessionId, Vec<Player>>,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
    session_id: SessionId,
    reason: crate::session::messages::DisconnectReason,
) {
    debug!(
        session = session_id.0,
        ?reason,
        "session disconnected during play"
    );
    disconnected.insert(session_id);
    if let Some(players) = session_players.get(&session_id) {
        for &p in players {
            let slot = match p {
                Player::Player1 => &mut *p1_slot,
                Player::Player2 => &mut *p2_slot,
                _ => continue,
            };
            update_action(slot, Direction::Stay, false, 0);
            emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
        }
    }
}

/// Resolve sender from session_players, emit BotInfo with the turn from the message.
fn handle_info(
    session_players: &HashMap<SessionId, Vec<Player>>,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
    session_id: SessionId,
    info: crate::session::messages::OwnedInfo,
) {
    if let Some(players) = session_players.get(&session_id) {
        if let Some(&sender) = players.first() {
            emit(
                event_tx,
                MatchEvent::BotInfo {
                    sender,
                    turn: info.turn,
                    state_hash: info.state_hash,
                    info,
                },
            );
        }
    }
}

/// Send Timeout command to non-responded sessions, emit BotTimeout.
async fn handle_timeout(
    sessions: &[SessionHandle],
    disconnected: &HashSet<SessionId>,
    responded: &HashSet<SessionId>,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
    current_turn: u16,
    stay: Direction,
) {
    for s in sessions {
        if !disconnected.contains(&s.session_id) && !responded.contains(&s.session_id) {
            let _ = s
                .cmd_tx
                .send(HostCommand::Timeout { default_move: stay })
                .await;
            for &p in &s.controlled_players {
                emit(
                    event_tx,
                    MatchEvent::BotTimeout {
                        player: p,
                        turn: current_turn,
                    },
                );
            }
        }
    }
}

/// Build a CollectedActions from the current slots.
fn resolve_collected(
    p1_slot: &Option<ActionSlot>,
    p2_slot: &Option<ActionSlot>,
    p1_wall_ms: u32,
    p2_wall_ms: u32,
) -> CollectedActions {
    CollectedActions {
        p1: resolve_action(p1_slot),
        p2: resolve_action(p2_slot),
        p1_think_ms: resolve_think_ms(p1_slot),
        p2_think_ms: resolve_think_ms(p2_slot),
        p1_wall_ms,
        p2_wall_ms,
    }
}

/// Per-player action state during action collection.
struct ActionSlot {
    direction: Direction,
    committed: bool,
    think_ms: u32,
}

/// Update a player's action slot. Provisional actions overwrite freely;
/// committed actions lock the slot.
fn update_action(
    slot: &mut Option<ActionSlot>,
    direction: Direction,
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
fn resolve_action(slot: &Option<ActionSlot>) -> Direction {
    slot.as_ref()
        .map(|s| s.direction)
        .unwrap_or(Direction::Stay)
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
    use pyrat::Coordinates;
    use pyrat_wire::Player;
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
                direction: Direction::Right,
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
                direction: Direction::Right,
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
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        // P1 got Right (accepted), P2 got Stay (stale → timeout default).
        assert_eq!(actions.p1, Direction::Right);
        assert_eq!(actions.p2, Direction::Stay);

        // Session 2 should have received a Timeout command.
        let cmd = cmd_rx2.try_recv().expect("session 2 should get Timeout");
        assert!(
            matches!(cmd, HostCommand::Timeout { default_move } if default_move == Direction::Stay),
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
                    turn: current_turn,
                    state_hash: 0,
                },
            })
            .await
            .unwrap();

        // Then both players send committed actions (think_ms=0 OK in infinite mode).
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: Direction::Up,
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
                direction: Direction::Down,
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
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        assert_eq!(actions.p1, Direction::Up);
        assert_eq!(actions.p2, Direction::Down);

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
                direction: Direction::Down,
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
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        // Disconnected player gets STAY, other player's action is used.
        assert_eq!(actions.p1, Direction::Stay);
        assert_eq!(actions.p2, Direction::Down);
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
                direction: Direction::Left,
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
                direction: Direction::Up,
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
                direction: Direction::Down,
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
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        // P1 gets provisional Left (committed was rejected), P2 gets committed Down.
        assert_eq!(actions.p1, Direction::Left);
        assert_eq!(actions.p2, Direction::Down);
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
                direction: Direction::Right,
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
                direction: Direction::Left,
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
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        assert_eq!(actions.p1, Direction::Right);
        // P2's committed was rejected, no provisional → Stay at timeout
        assert_eq!(actions.p2, Direction::Stay);
    }

    /// Late Info (sent for a previous turn) is forwarded with the turn
    /// from the message, not the host's current turn.
    #[tokio::test]
    async fn late_info_preserves_original_turn() {
        let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(16);
        let (sessions, _cmd_rx1, _cmd_rx2) = make_sessions();

        let session_players: HashMap<SessionId, Vec<Player>> = sessions
            .iter()
            .map(|s| (s.session_id, s.controlled_players.clone()))
            .collect();

        let current_turn: u16 = 5;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

        // Bot1 sends Info with turn=3 (late, from a previous turn).
        game_tx
            .send(SessionMsg::Info {
                session_id: SessionId(1),
                info: crate::session::messages::OwnedInfo {
                    player: Player::Player1,
                    multipv: 1,
                    target: None,
                    depth: 3,
                    nodes: 50,
                    score: Some(1.0),
                    pv: vec![],
                    message: "late".into(),
                    turn: 3,
                    state_hash: 0,
                },
            })
            .await
            .unwrap();

        // Both players send committed actions so collect_actions returns.
        game_tx
            .send(SessionMsg::Action {
                session_id: SessionId(1),
                player: Player::Player1,
                direction: Direction::Up,
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
                direction: Direction::Down,
                turn: current_turn,
                provisional: false,
                think_ms: 0,
            })
            .await
            .unwrap();

        let mut disconnected = HashSet::new();
        let config = test_config(Duration::ZERO); // infinite
        let _actions = collect_actions(
            &mut game_rx,
            current_turn,
            &sessions,
            &session_players,
            &mut disconnected,
            &config,
            Some(&event_tx),
            Instant::now(),
        )
        .await
        .expect("collect_actions should not fail");

        // BotInfo event must carry turn=3 (from the message), not turn=5 (current).
        let event = event_rx.try_recv().expect("should have BotInfo event");
        match event {
            MatchEvent::BotInfo {
                sender,
                turn,
                state_hash,
                info,
            } => {
                assert_eq!(sender, Player::Player1);
                assert_eq!(
                    turn, 3,
                    "BotInfo should carry the original turn from the Info message"
                );
                assert_eq!(
                    state_hash, 0,
                    "BotInfo should carry state_hash from the Info message"
                );
                assert_eq!(info.message, "late");
            },
            other => panic!("expected BotInfo, got {other:?}"),
        }
    }

    use crate::session::messages::HashedTurnState;

    /// Helper: baseline state for hash distinctness tests.
    fn baseline_turn_state() -> OwnedTurnState {
        OwnedTurnState {
            turn: 5,
            player1_position: Coordinates::new(1, 2),
            player2_position: Coordinates::new(3, 4),
            player1_score: 2.0,
            player2_score: 1.5,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            cheese: vec![Coordinates::new(5, 5), Coordinates::new(10, 7)],
            player1_last_move: Direction::Up,
            player2_last_move: Direction::Down,
        }
    }

    /// Changing any single field must produce a different hash.
    #[test]
    fn state_hash_distinguishes_all_fields() {
        let base = baseline_turn_state();
        let base_hash = HashedTurnState::new(base.clone()).state_hash();
        assert_ne!(base_hash, 0, "hash should not be zero");

        let cases: Vec<(&str, OwnedTurnState)> = vec![
            (
                "turn +1",
                OwnedTurnState {
                    turn: 6,
                    ..base.clone()
                },
            ),
            (
                "p1 position",
                OwnedTurnState {
                    player1_position: Coordinates::new(2, 2),
                    ..base.clone()
                },
            ),
            (
                "p2 position",
                OwnedTurnState {
                    player2_position: Coordinates::new(3, 5),
                    ..base.clone()
                },
            ),
            (
                "p1 score +0.5",
                OwnedTurnState {
                    player1_score: 2.5,
                    ..base.clone()
                },
            ),
            (
                "p2 score +0.5",
                OwnedTurnState {
                    player2_score: 2.0,
                    ..base.clone()
                },
            ),
            (
                "p1 mud +1",
                OwnedTurnState {
                    player1_mud_turns: 1,
                    ..base.clone()
                },
            ),
            (
                "p2 mud +1",
                OwnedTurnState {
                    player2_mud_turns: 1,
                    ..base.clone()
                },
            ),
            (
                "one less cheese",
                OwnedTurnState {
                    cheese: vec![Coordinates::new(5, 5)],
                    ..base.clone()
                },
            ),
            (
                "cheese offset by 1",
                OwnedTurnState {
                    cheese: vec![Coordinates::new(5, 6), Coordinates::new(10, 7)],
                    ..base.clone()
                },
            ),
            (
                "p1 last move",
                OwnedTurnState {
                    player1_last_move: Direction::Right,
                    ..base.clone()
                },
            ),
            (
                "p2 last move",
                OwnedTurnState {
                    player2_last_move: Direction::Left,
                    ..base.clone()
                },
            ),
        ];

        for (label, variant) in &cases {
            let h = HashedTurnState::new(variant.clone()).state_hash();
            assert_ne!(h, base_hash, "{label}: hash should differ from baseline");
        }
    }

    /// Identical states produce the same hash (determinism).
    #[test]
    fn identical_states_produce_same_hash() {
        let a = HashedTurnState::new(baseline_turn_state());
        let b = HashedTurnState::new(baseline_turn_state());
        assert_eq!(a.state_hash(), b.state_hash());
    }

    /// Swapping p1/p2 positions must produce a different hash (not commutative).
    #[test]
    fn swapped_players_produce_different_hash() {
        let base = baseline_turn_state();
        let swapped = OwnedTurnState {
            player1_position: base.player2_position,
            player2_position: base.player1_position,
            player1_score: base.player2_score,
            player2_score: base.player1_score,
            ..base.clone()
        };
        assert_ne!(
            HashedTurnState::new(base).state_hash(),
            HashedTurnState::new(swapped).state_hash(),
            "swapped players must hash differently"
        );
    }
}
