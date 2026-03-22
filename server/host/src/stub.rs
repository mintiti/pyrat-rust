//! In-process stub bot for testing and GUI debug mode.
//!
//! Speaks the `SessionMsg` / `HostCommand` channel protocol directly —
//! no TCP, no FlatBuffers, no subprocess. From the host's perspective it
//! is indistinguishable from a real bot session.

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::session::messages::{HostCommand, OwnedInfo, SessionMsg};
use crate::session::SessionId;
use pyrat_wire::{Direction, Player};

/// The four movement directions (excludes Stay).
const MOVES: [Direction; 4] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
];

/// Spawn an in-process stub bot that plays random moves.
///
/// The returned `JoinHandle` completes when the game loop sends `GameOver`
/// or `Shutdown`, or when `game_tx` is closed.
pub fn spawn_stub_bot(
    session_id: SessionId,
    agent_id: String,
    name: String,
    game_tx: mpsc::Sender<SessionMsg>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_stub(session_id, agent_id, name, game_tx).await;
    })
}

async fn run_stub(
    session_id: SessionId,
    agent_id: String,
    name: String,
    game_tx: mpsc::Sender<SessionMsg>,
) {
    // Create per-session command channel (same pattern as real sessions).
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<HostCommand>(32);

    // Phase A: Connected
    if game_tx
        .send(SessionMsg::Connected { session_id, cmd_tx })
        .await
        .is_err()
    {
        return;
    }

    // Phase A: Identified
    if game_tx
        .send(SessionMsg::Identified {
            session_id,
            name,
            author: "stub".into(),
            options: vec![],
            agent_id,
        })
        .await
        .is_err()
    {
        return;
    }

    // Phase B: Wait for MatchConfig, then send Ready
    let controlled_players = loop {
        match cmd_rx.recv().await {
            Some(HostCommand::MatchConfig(cfg)) => {
                break cfg.controlled_players.clone();
            },
            Some(HostCommand::SetOption { .. }) => {
                // Ignore options for stub bot.
            },
            Some(HostCommand::Shutdown | HostCommand::GameOver { .. }) => {
                send_disconnected(session_id, &game_tx).await;
                return;
            },
            None => return,
            _ => {},
        }
    };

    if game_tx
        .send(SessionMsg::Ready { session_id })
        .await
        .is_err()
    {
        return;
    }

    // Phase C: Wait for StartPreprocessing, then send PreprocessingDone
    loop {
        match cmd_rx.recv().await {
            Some(HostCommand::StartPreprocessing) => break,
            Some(HostCommand::Shutdown | HostCommand::GameOver { .. }) => {
                send_disconnected(session_id, &game_tx).await;
                return;
            },
            None => return,
            _ => {},
        }
    }

    if game_tx
        .send(SessionMsg::PreprocessingDone { session_id })
        .await
        .is_err()
    {
        return;
    }

    // Playing phase: respond to TurnState with random actions.
    loop {
        match cmd_rx.recv().await {
            Some(HostCommand::TurnState(ts)) => {
                let turn = ts.turn;
                let dir = random_direction();

                // Emit fake Info so the GUI pipeline has data to display.
                let player = controlled_players
                    .first()
                    .copied()
                    .unwrap_or(Player::Player1);
                let target = ts.cheese.first().copied();
                let _ = game_tx
                    .send(SessionMsg::Info {
                        session_id,
                        info: OwnedInfo {
                            player,
                            multipv: 1,
                            target,
                            depth: 1,
                            nodes: 1,
                            score: Some(0.0),
                            pv: vec![dir],
                            message: "stub".into(),
                        },
                    })
                    .await;

                for &player in &controlled_players {
                    if game_tx
                        .send(SessionMsg::Action {
                            session_id,
                            player,
                            direction: dir,
                            turn,
                            provisional: false,
                            think_ms: 0,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            },
            Some(HostCommand::GameOver { .. } | HostCommand::Shutdown) => {
                break;
            },
            Some(_) => {},
            None => return,
        }
    }

    send_disconnected(session_id, &game_tx).await;
}

fn random_direction() -> Direction {
    MOVES[fastrand::usize(..MOVES.len())]
}

async fn send_disconnected(session_id: SessionId, game_tx: &mpsc::Sender<SessionMsg>) {
    let _ = game_tx
        .send(SessionMsg::Disconnected {
            session_id,
            reason: crate::session::messages::DisconnectReason::PeerClosed,
        })
        .await;
    debug!(session = session_id.0, "stub bot disconnected");
}
