mod codec;
pub mod messages;
pub mod state;

pub use messages::{DisconnectReason, HostCommand, SessionId, SessionMsg};
pub use state::SessionState;

use std::collections::HashSet;
use std::time::Duration;

use flatbuffers::FlatBufferBuilder;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::wire::framing::{FrameError, FrameReader, FrameWriter};
use crate::wire::Player;
use codec::{extract_bot_packet, serialize_host_command, BotPayload};

/// Tunable limits for a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// How long a bot may stay in `Connected` before sending `Identify`.
    pub handshake_timeout: Duration,
    /// Maximum frames to drain after entering wind-down (Shutdown / GameOver).
    pub drain_max_frames: u32,
    /// Wall-clock cap on the post-shutdown drain phase.
    pub drain_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(10),
            drain_max_frames: 64,
            drain_timeout: Duration::from_secs(2),
        }
    }
}

/// Run a single bot session to completion.
///
/// Reads BotPackets from `reader`, validates them against the lifecycle state
/// machine, and forwards owned data to `game_tx`. Receives `HostCommand`s
/// from the game loop and serializes them as HostPackets to `writer`.
///
/// Sends `SessionMsg::Disconnected` before returning, unless the game loop
/// receiver is already gone.
pub async fn run_session<R, W>(
    session_id: SessionId,
    reader: R,
    writer: W,
    game_tx: mpsc::Sender<SessionMsg>,
    config: SessionConfig,
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
    let mut current_turn: u16 = 0;

    // Drain bookkeeping — set when entering wind-down.
    let mut drain_frames_remaining: u32 = 0;
    let far_future = Instant::now() + Duration::from_secs(86400);
    let mut drain_deadline = far_future;

    // Handshake deadline — only active while state == Connected.
    let handshake_deadline = Instant::now() + config.handshake_timeout;

    let mut disconnect_reason = DisconnectReason::PeerClosed;

    // Per-session command channel — sent to the game loop in Connected.
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<HostCommand>(32);

    // Notify game loop that this session exists.
    if game_tx
        .send(SessionMsg::Connected { session_id, cmd_tx })
        .await
        .is_err()
    {
        // Receiver is gone — nobody is listening. Skip Disconnected too.
        return;
    }

    loop {
        tokio::select! {
            // ── Read from bot ───────────────────────
            frame_result = frame_reader.read_frame() => {
                match frame_result {
                    Ok(buf) => {
                        if closing || state == SessionState::Done {
                            // Drain without forwarding.
                            drain_frames_remaining = drain_frames_remaining.saturating_sub(1);
                            if drain_frames_remaining == 0 {
                                disconnect_reason = DisconnectReason::DrainComplete;
                                break;
                            }
                            continue;
                        }
                        if let Err(e) = handle_bot_frame(
                            buf,
                            session_id,
                            &mut state,
                            &controlled_players,
                            current_turn,
                            &game_tx,
                        ).await {
                            warn!(session = session_id.0, error = %e, "bot frame error");
                        }
                    }
                    Err(FrameError::Disconnected) => {
                        debug!(session = session_id.0, "bot disconnected");
                        disconnect_reason = DisconnectReason::PeerClosed;
                        break;
                    }
                    Err(e) => {
                        warn!(session = session_id.0, error = %e, "frame read error");
                        disconnect_reason = DisconnectReason::FrameError;
                        break;
                    }
                }
            }

            // ── Receive from game loop ──────────────
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(HostCommand::Shutdown) => {
                        closing = true;
                        drain_frames_remaining = config.drain_max_frames;
                        drain_deadline = Instant::now() + config.drain_timeout;
                        // Send Stop on the wire, then drain.
                        let bytes = serialize_host_command(&mut fbb, &HostCommand::Shutdown);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(HostCommand::GameOver { result, rat_score, python_score }) => {
                        state = SessionState::Done;
                        closing = true;
                        drain_frames_remaining = config.drain_max_frames;
                        drain_deadline = Instant::now() + config.drain_timeout;
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
                    Some(HostCommand::TurnState(ref ts)) => {
                        current_turn = ts.turn;
                        let cmd = HostCommand::TurnState(ts.clone());
                        let bytes = serialize_host_command(&mut fbb, &cmd);
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
                        disconnect_reason = DisconnectReason::ChannelClosed;
                        break;
                    }
                }
            }

            // ── Handshake timeout ───────────────────
            _ = tokio::time::sleep_until(handshake_deadline), if state == SessionState::Connected => {
                warn!(session = session_id.0, "handshake timeout — no Identify received");
                disconnect_reason = DisconnectReason::HandshakeTimeout;
                break;
            }

            // ── Drain timeout ───────────────────────
            _ = tokio::time::sleep_until(drain_deadline), if closing => {
                debug!(session = session_id.0, "drain timeout elapsed");
                disconnect_reason = DisconnectReason::DrainComplete;
                break;
            }
        }
    }

    let _ = game_tx
        .send(SessionMsg::Disconnected {
            session_id,
            reason: disconnect_reason,
        })
        .await;
}

/// Process a single bot frame: parse, validate state, forward to game loop.
async fn handle_bot_frame(
    buf: &[u8],
    session_id: SessionId,
    state: &mut SessionState,
    controlled_players: &HashSet<Player>,
    current_turn: u16,
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
            agent_id,
        } => SessionMsg::Identified {
            session_id,
            name,
            author,
            options,
            agent_id,
        },
        BotPayload::Ready => SessionMsg::Ready { session_id },
        BotPayload::PreprocessingDone => SessionMsg::PreprocessingDone { session_id },
        BotPayload::Action {
            mut player,
            direction,
        } => {
            // Default player inference: if there's exactly one controlled player
            // that is NOT the FlatBuffers default (Rat), and the bot sent the
            // default, assume it meant the only player it controls.
            if controlled_players.len() == 1 {
                let &only = controlled_players.iter().next().unwrap();
                if only != Player::Rat && player == Player::Rat {
                    player = only;
                }
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
                turn: current_turn,
            }
        },
        BotPayload::Info(info) => SessionMsg::Info { session_id, info },
        // Pong and RenderCommands are accepted but not forwarded.
        BotPayload::Pong | BotPayload::RenderCommands => return Ok(()),
    };

    let _ = game_tx.send(msg).await;
    Ok(())
}
