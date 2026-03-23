mod codec;
pub mod messages;
pub mod state;

pub use codec::{extract_bot_packet, BotPayload};
pub use messages::{DisconnectReason, HostCommand, SessionId, SessionMsg};
pub use state::SessionState;

use std::collections::HashSet;
use std::time::Duration;

use flatbuffers::FlatBufferBuilder;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, info, trace, warn};

use codec::serialize_host_command;
use pyrat_wire::framing::{FrameError, FrameReader, FrameWriter};
use pyrat_wire::{BotMessage, Player};

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

/// Truncate a string for trace logging.
fn truncate_str(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
    if s.len() <= max {
        std::borrow::Cow::Borrowed(s)
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        std::borrow::Cow::Owned(format!("{}…", &s[..end]))
    }
}

fn cmd_label(cmd: &HostCommand) -> &'static str {
    match cmd {
        HostCommand::SetOption { .. } => "SetOption",
        HostCommand::MatchConfig(_) => "MatchConfig",
        HostCommand::StartPreprocessing => "StartPreprocessing",
        HostCommand::TurnState(_) => "TurnState",
        HostCommand::Timeout { .. } => "Timeout",
        HostCommand::GameOver { .. } => "GameOver",
        HostCommand::Ping => "Ping",
        HostCommand::Stop => "Stop",
        HostCommand::Shutdown => "Shutdown",
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

    // Drain bookkeeping — set when entering wind-down.
    let mut drain_frames_remaining: u32 = 0;
    let far_future = Instant::now() + Duration::from_secs(86400);
    let mut drain_deadline = far_future;

    // Handshake deadline — only active while state == Connected.
    let handshake_deadline = Instant::now() + config.handshake_timeout;

    // Every break path below sets this explicitly. The initial value is a
    // defensive fallback if a new exit path is ever added without one.
    #[allow(unused_assignments)]
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
                            &game_tx,
                        ).await {
                            warn!(error = %e, "bot frame error");
                        }
                    }
                    Err(FrameError::Disconnected) => {
                        debug!("bot disconnected");
                        disconnect_reason = DisconnectReason::PeerClosed;
                        break;
                    }
                    Err(e) => {
                        if closing {
                            debug!(error = %e, "frame read error during shutdown");
                        } else {
                            warn!(error = %e, "frame read error");
                        }
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
                            disconnect_reason = DisconnectReason::FrameError;
                            break;
                        }
                        debug!(cmd = "Shutdown", size = bytes.len(), "→ sent");
                    }
                    Some(HostCommand::GameOver { result, player1_score, player2_score }) => {
                        state = SessionState::Done;
                        closing = true;
                        drain_frames_remaining = config.drain_max_frames;
                        drain_deadline = Instant::now() + config.drain_timeout;
                        let cmd = HostCommand::GameOver { result, player1_score, player2_score };
                        let bytes = serialize_host_command(&mut fbb, &cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            disconnect_reason = DisconnectReason::FrameError;
                            break;
                        }
                        debug!(cmd = "GameOver", size = bytes.len(), "→ sent");
                        trace!(
                            cmd = "GameOver",
                            result = ?result,
                            player1_score,
                            player2_score,
                            "→ payload"
                        );
                    }
                    Some(ref cmd @ HostCommand::MatchConfig(ref cfg)) => {
                        // Record controlled players for ownership validation.
                        controlled_players.clear();
                        for &p in &cfg.controlled_players {
                            controlled_players.insert(p);
                        }
                        let bytes = serialize_host_command(&mut fbb, cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            disconnect_reason = DisconnectReason::FrameError;
                            break;
                        }
                        debug!(cmd = "MatchConfig", size = bytes.len(), "→ sent");
                        trace!(
                            cmd = "MatchConfig",
                            width = cfg.width,
                            height = cfg.height,
                            max_turns = cfg.max_turns,
                            walls = cfg.walls.len(),
                            mud = cfg.mud.len(),
                            cheese = cfg.cheese.len(),
                            controlled_players = ?cfg.controlled_players,
                            timing = ?cfg.timing,
                            move_timeout_ms = cfg.move_timeout_ms,
                            preprocessing_timeout_ms = cfg.preprocessing_timeout_ms,
                            "→ payload"
                        );
                    }
                    Some(ref cmd @ HostCommand::TurnState(ref ts)) => {
                        let bytes = serialize_host_command(&mut fbb, cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            disconnect_reason = DisconnectReason::FrameError;
                            break;
                        }
                        debug!(cmd = "TurnState", turn = ts.turn, size = bytes.len(), "→ sent");
                        trace!(
                            cmd = "TurnState",
                            turn = ts.turn,
                            p1_pos = ?(ts.player1_position),
                            p2_pos = ?(ts.player2_position),
                            p1_score = ts.player1_score,
                            p2_score = ts.player2_score,
                            p1_mud = ts.player1_mud_turns,
                            p2_mud = ts.player2_mud_turns,
                            cheese = ts.cheese.len(),
                            p1_last = ?ts.player1_last_move,
                            p2_last = ?ts.player2_last_move,
                            "→ payload"
                        );
                    }
                    Some(cmd) => {
                        let label = cmd_label(&cmd);
                        let bytes = serialize_host_command(&mut fbb, &cmd);
                        if frame_writer.write_frame(&bytes).await.is_err() {
                            disconnect_reason = DisconnectReason::FrameError;
                            break;
                        }
                        debug!(cmd = label, size = bytes.len(), "→ sent");
                        match &cmd {
                            HostCommand::Timeout { default_move } => {
                                trace!(cmd = "Timeout", default_move = ?default_move, "→ payload");
                            }
                            HostCommand::SetOption { name, value } => {
                                trace!(cmd = "SetOption", name = %name, value = %value, "→ payload");
                            }
                            _ => {}
                        }
                    }
                    None => {
                        // Game loop dropped the sender — clean exit.
                        debug!("command channel closed");
                        disconnect_reason = DisconnectReason::ChannelClosed;
                        break;
                    }
                }
            }

            // ── Handshake timeout ───────────────────
            _ = tokio::time::sleep_until(handshake_deadline), if state == SessionState::Connected => {
                warn!("handshake timeout — no Identify received");
                disconnect_reason = DisconnectReason::HandshakeTimeout;
                break;
            }

            // ── Drain timeout ───────────────────────
            _ = tokio::time::sleep_until(drain_deadline), if closing => {
                debug!("drain timeout elapsed");
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

fn bot_msg_label(msg: BotMessage) -> &'static str {
    if msg == BotMessage::Identify {
        "Identify"
    } else if msg == BotMessage::Ready {
        "Ready"
    } else if msg == BotMessage::PreprocessingDone {
        "PreprocessingDone"
    } else if msg == BotMessage::Action {
        "Action"
    } else if msg == BotMessage::Pong {
        "Pong"
    } else if msg == BotMessage::Info {
        "Info"
    } else if msg == BotMessage::RenderCommands {
        "RenderCommands"
    } else {
        "Unknown"
    }
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
    match &payload {
        BotPayload::Action {
            player,
            direction,
            turn,
            provisional,
            think_ms,
        } => {
            let dir = direction.variant_name().unwrap_or("?");
            debug!(
                msg = "Action",
                dir,
                turn,
                provisional,
                size = buf.len(),
                "← received"
            );
            trace!(
                msg = "Action",
                player = ?player,
                dir,
                turn,
                provisional,
                think_ms,
                "← payload"
            );
        },
        BotPayload::Info(info) => {
            debug!(msg = "Info", score = ?info.score, size = buf.len(), "← received");
            trace!(
                msg = "Info",
                player = ?info.player,
                multipv = info.multipv,
                target = ?info.target,
                depth = info.depth,
                nodes = info.nodes,
                score = ?info.score,
                pv_len = info.pv.len(),
                pv_head = ?info.pv.iter().take(5).collect::<Vec<_>>(),
                message = %truncate_str(&info.message, 120),
                "← payload"
            );
        },
        BotPayload::Identify {
            name,
            author,
            agent_id,
            options,
        } => {
            debug!(msg = "Identify", size = buf.len(), "← received");
            trace!(
                msg = "Identify",
                name = %name,
                author = %author,
                agent_id = %agent_id,
                options = options.len(),
                "← payload"
            );
        },
        _ => {
            debug!(
                msg = bot_msg_label(msg_type),
                size = buf.len(),
                "← received"
            );
        },
    }

    // State validation.
    if !state.accepts(msg_type) {
        warn!(
            msg_type = msg_type.0,
            state = ?state,
            "rejected bot message in wrong state"
        );
        return Ok(());
    }

    // Apply transition.
    if let Some(next) = state.transition(msg_type) {
        debug!(
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
        } => {
            tracing::Span::current().record("agent_id", agent_id.as_str());
            info!(agent_id = %agent_id, "bot identified");
            SessionMsg::Identified {
                session_id,
                name,
                author,
                options,
                agent_id,
            }
        },
        BotPayload::Ready => SessionMsg::Ready { session_id },
        BotPayload::PreprocessingDone => SessionMsg::PreprocessingDone { session_id },
        BotPayload::Action {
            mut player,
            direction,
            turn,
            provisional,
            think_ms,
        } => {
            // Default player inference: if there's exactly one controlled player
            // that is NOT the FlatBuffers default (Player1), and the bot sent the
            // default, assume it meant the only player it controls.
            if controlled_players.len() == 1 {
                let &only = controlled_players.iter().next().unwrap();
                if only != Player::Player1 && player == Player::Player1 {
                    player = only;
                }
            }

            // Ownership validation.
            if !controlled_players.is_empty() && !controlled_players.contains(&player) {
                warn!(player = player.0, "action for non-controlled player");
                return Ok(());
            }

            SessionMsg::Action {
                session_id,
                player,
                direction,
                turn,
                provisional,
                think_ms,
            }
        },
        BotPayload::Info(info) => SessionMsg::Info { session_id, info },
        // Pong and RenderCommands are accepted but not forwarded.
        BotPayload::Pong | BotPayload::RenderCommands => return Ok(()),
    };

    let _ = game_tx.send(msg).await;
    Ok(())
}
