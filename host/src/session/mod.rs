mod codec;
pub mod messages;
pub mod state;

pub use messages::{HostCommand, SessionId, SessionMsg};
pub use state::SessionState;

use std::collections::HashSet;

use flatbuffers::FlatBufferBuilder;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::wire::framing::{FrameError, FrameReader, FrameWriter};
use crate::wire::Player;
use codec::{extract_bot_packet, serialize_host_command, BotPayload};

/// Run a single bot session to completion.
///
/// Reads BotPackets from `reader`, validates them against the lifecycle state
/// machine, and forwards owned data to `game_tx`. Receives `HostCommand`s
/// from the game loop and serializes them as HostPackets to `writer`.
///
/// Always sends `SessionMsg::Disconnected` before returning.
pub async fn run_session<R, W>(
    session_id: SessionId,
    reader: R,
    writer: W,
    game_tx: mpsc::Sender<SessionMsg>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut frame_reader = FrameReader::with_default_max(reader);
    let mut frame_writer = FrameWriter::with_default_max(writer);
    let mut fbb = FlatBufferBuilder::new();

    let mut state = SessionState::Connected;
    let mut controlled_players: HashSet<Player> = HashSet::new();
    let mut closing = false;

    // Per-session command channel — sent to the game loop in Connected.
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<HostCommand>(32);

    // Notify game loop that this session exists.
    let _ = game_tx
        .send(SessionMsg::Connected { session_id, cmd_tx })
        .await;

    loop {
        tokio::select! {
            // ── Read from bot ───────────────────────
            frame_result = frame_reader.read_frame() => {
                match frame_result {
                    Ok(buf) => {
                        if closing || state == SessionState::Done {
                            // Drain without forwarding.
                            continue;
                        }
                        if let Err(e) = handle_bot_frame(
                            buf,
                            session_id,
                            &mut state,
                            &controlled_players,
                            &game_tx,
                        ).await {
                            warn!(session = session_id.0, error = %e, "bot frame error");
                        }
                    }
                    Err(FrameError::Disconnected) => {
                        debug!(session = session_id.0, "bot disconnected");
                        break;
                    }
                    Err(e) => {
                        warn!(session = session_id.0, error = %e, "frame read error");
                        break;
                    }
                }
            }

            // ── Receive from game loop ──────────────
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(HostCommand::Shutdown) => {
                        closing = true;
                        // Send Stop on the wire, then drain.
                        let bytes = serialize_host_command(&mut fbb, &HostCommand::Stop);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(HostCommand::GameOver { result, rat_score, python_score }) => {
                        state = SessionState::Done;
                        let cmd = HostCommand::GameOver { result, rat_score, python_score };
                        let bytes = serialize_host_command(&mut fbb, &cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(ref cmd @ HostCommand::MatchConfig(ref cfg)) => {
                        // Record controlled players for ownership validation.
                        controlled_players.clear();
                        for &p in &cfg.controlled_players {
                            controlled_players.insert(p);
                        }
                        let bytes = serialize_host_command(&mut fbb, cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(cmd) => {
                        let bytes = serialize_host_command(&mut fbb, &cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        // Game loop dropped the sender — clean exit.
                        debug!(session = session_id.0, "command channel closed");
                        break;
                    }
                }
            }
        }
    }

    let _ = game_tx.send(SessionMsg::Disconnected { session_id }).await;
}

/// Process a single bot frame: parse, validate state, forward to game loop.
async fn handle_bot_frame(
    buf: &[u8],
    session_id: SessionId,
    state: &mut SessionState,
    controlled_players: &HashSet<Player>,
    game_tx: &mpsc::Sender<SessionMsg>,
) -> Result<(), String> {
    let (msg_type, payload) = extract_bot_packet(buf)?;

    // State validation.
    if !state.accepts(msg_type) {
        warn!(
            session = session_id.0,
            msg_type = msg_type.0,
            state = ?state,
            "rejected bot message in wrong state"
        );
        return Ok(());
    }

    // Apply transition.
    if let Some(next) = state.transition(msg_type) {
        debug!(
            session = session_id.0,
            from = ?state,
            to = ?next,
            "state transition"
        );
        *state = next;
    }

    // Forward to game loop.
    let msg = match payload {
        BotPayload::Identify {
            name,
            author,
            options,
        } => SessionMsg::Identified {
            session_id,
            name,
            author,
            options,
        },
        BotPayload::Ready => SessionMsg::Ready { session_id },
        BotPayload::PreprocessingDone => SessionMsg::PreprocessingDone { session_id },
        BotPayload::Action {
            mut player,
            direction,
        } => {
            // Default player inference: if there's exactly one controlled player
            // and the bot sent the FlatBuffers default (Rat/0), fill it in.
            if controlled_players.len() == 1 && player == Player::Rat {
                let &only = controlled_players.iter().next().unwrap();
                player = only;
            }

            // Ownership validation.
            if !controlled_players.is_empty() && !controlled_players.contains(&player) {
                warn!(
                    session = session_id.0,
                    player = player.0,
                    "action for non-controlled player"
                );
                return Ok(());
            }

            SessionMsg::Action {
                session_id,
                player,
                direction,
            }
        },
        BotPayload::Info(info) => SessionMsg::Info { session_id, info },
        // Pong and RenderCommands are accepted but not forwarded.
        BotPayload::Pong | BotPayload::RenderCommands => return Ok(()),
    };

    let _ = game_tx.send(msg).await;
    Ok(())
}
