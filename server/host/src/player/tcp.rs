//! TCP-backed [`Player`] implementation and the multi-slot accept layer.
//!
//! `TcpPlayer` owns its own minimal session task: it wraps a `TcpStream` in
//! `pyrat_wire::framing::{FrameReader, FrameWriter}`, decodes frames via
//! [`pyrat_protocol::codec`], and shuttles owned messages over a pair of
//! bounded `mpsc` channels.
//!
//! [`accept_players`] is the multi-slot accept layer: it dispatches incoming
//! connections by `agent_id` and runs handshakes concurrently on a `JoinSet`,
//! so a slow or silent connection cannot block valid bots.
//!
//! # Provisional handling
//!
//! Mirrors [`super::EmbeddedPlayer`]: every `BotMsg::Provisional` is consumed
//! inside `recv()`, stored in a turn-scoped slot, forwarded to `EventSink` as
//! `MatchEvent::BotProvisional`, and never returned to Match. The slot is
//! cleared on a new `Go`/`GoState` (whole-turn boundary) or by a successful
//! [`Player::take_provisional`].

use std::time::Duration;

use async_trait::async_trait;
use pyrat::Direction;
use pyrat_protocol::{extract_bot_msg, serialize_host_msg, BotMsg, HostMsg};
use pyrat_wire::framing::{FrameError, FrameReader, FrameWriter};
use pyrat_wire::{BotPacket, Player as PlayerSlot};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

use super::{EventSink, Player, PlayerError, PlayerIdentity, ProvisionalSlot};
use crate::match_host::MatchEvent;

/// Default depth of the bounded outbound and inbound mpsc channels between
/// `TcpPlayer` and its session task. Provides natural backpressure when one
/// side outpaces the other.
const DEFAULT_CHANNEL_DEPTH: usize = 64;

/// Default per-connection deadline for the `Identify -> Welcome` handshake.
/// A silent or slow peer times out after this without affecting other
/// in-flight handshakes.
const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long [`TcpPlayer::close`] waits for the session task to exit before
/// aborting it. Bounded per the [`Player::close`] contract.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

// ── Errors ────────────────────────────────────────────────────────────

/// Reasons [`accept_players`] can fail to produce its slot-indexed array.
///
/// Unknown / duplicate agent_ids do **not** propagate here. Those connections
/// are sent `HostMsg::ProtocolError` and dropped silently; `accept_players`
/// keeps waiting for the expected agents.
#[derive(Debug, thiserror::Error)]
pub enum AcceptError {
    /// I/O error while accepting a connection from the listener. The listener
    /// itself is bound by the caller, so there's no `Bind` variant.
    #[error("accept failed: {0}")]
    AcceptIo(#[from] std::io::Error),
    /// Overall accept timeout fired before all expected agents connected.
    #[error("accept timed out before all expected agents connected")]
    Timeout,
    /// `expected` is empty, longer than 2, or names the same agent_id twice.
    /// Hivemind (one agent claiming both slots) is deferred to a follow-up
    /// brief.
    #[error("hivemind not supported: {0}")]
    HivemindNotSupported(String),
    /// `expected` was empty or invalid in a way unrelated to hivemind.
    #[error("invalid expected agents: {0}")]
    InvalidExpected(String),
}

// ── Handshake ─────────────────────────────────────────────────────────

/// Output of a per-connection handshake task: the agent that identified plus
/// the framed reader/writer ready for the session loop. Written to the
/// `JoinSet` in [`accept_players`].
struct Handshaked {
    agent_id: String,
    name: String,
    author: String,
    reader: FrameReader<tokio::net::tcp::OwnedReadHalf>,
    writer: FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
}

/// Run a single connection's `Identify -> ` handshake. The Welcome reply is
/// sent by [`accept_players`] after slot lookup succeeds — it owns slot
/// assignment, so it's the one entitled to send `Welcome`.
async fn handshake_one(stream: TcpStream, deadline: Duration) -> Result<Handshaked, PlayerError> {
    let _ = stream.set_nodelay(true);
    let (read_half, write_half) = stream.into_split();
    let mut reader = FrameReader::with_default_max(read_half);
    let writer = FrameWriter::with_default_max(write_half);

    let frame_result = timeout(deadline, reader.read_frame())
        .await
        .map_err(|_| PlayerError::Timeout)?;
    let bytes = frame_result
        .map_err(|e| PlayerError::TransportError(format!("handshake read: {e}")))?
        .to_vec();
    let packet = flatbuffers::root::<BotPacket>(&bytes)
        .map_err(|e| PlayerError::ProtocolError(format!("handshake packet decode: {e}")))?;
    let msg = extract_bot_msg(&packet)
        .map_err(|e| PlayerError::ProtocolError(format!("handshake extract: {e}")))?;
    match msg {
        BotMsg::Identify {
            name,
            author,
            agent_id,
            ..
        } => Ok(Handshaked {
            agent_id,
            name,
            author,
            reader,
            writer,
        }),
        other => Err(PlayerError::ProtocolError(format!(
            "expected Identify as first message, got {other:?}"
        ))),
    }
}

/// Send `HostMsg::ProtocolError` to a peer the host is rejecting (unknown or
/// duplicate agent_id), then drop the writer so the socket closes. Best-effort
/// — if the peer is already gone, the error is swallowed.
async fn reject_peer<W: AsyncWrite + Unpin>(mut writer: FrameWriter<W>, reason: &str) {
    let bytes = serialize_host_msg(&HostMsg::ProtocolError {
        reason: reason.to_owned(),
    });
    let _ = writer.write_frame(&bytes).await;
    drop(writer);
}

// ── accept_players ────────────────────────────────────────────────────

/// Multi-slot, agent_id-dispatched, concurrent-handshake accept (Foundation F3).
///
/// Accepts up to two TCP connections from `listener`, expects each to send
/// `BotMsg::Identify`, dispatches by `agent_id` to the slot named in
/// `expected`, replies with `HostMsg::Welcome { player_slot }`, and returns
/// the resulting `TcpPlayer`s indexed by the engine's `Player` enum: position
/// 0 is `Player::Player1`'s player (or `None`), position 1 is `Player::Player2`'s.
///
/// `expected.len()` is 1 or 2. Length-1 supports GUI matches where the other
/// slot is filled with an [`super::EmbeddedPlayer`] at the call site.
///
/// The handshake stops at `Identify -> Welcome`. `Configure` and `Ready`
/// belong to `Match::setup()` and are not sent here.
///
/// Concurrency: each accepted connection's handshake runs on its own task in
/// a `JoinSet`. The main `select!` advances on accept events AND handshake
/// completions, so a silent/slow connection times out alone without blocking
/// valid bots. Unknown or duplicate agent_ids receive `ProtocolError` and are
/// dropped; `accept_players` keeps waiting for the expected ones.
pub async fn accept_players(
    listener: &TcpListener,
    expected: &[(PlayerSlot, String)],
    event_sink: EventSink,
    overall_timeout: Duration,
) -> Result<[Option<TcpPlayer>; 2], AcceptError> {
    if expected.is_empty() || expected.len() > 2 {
        return Err(AcceptError::InvalidExpected(format!(
            "expected.len() must be 1 or 2, got {}",
            expected.len()
        )));
    }
    if expected.len() == 2 && expected[0].1 == expected[1].1 {
        return Err(AcceptError::HivemindNotSupported(format!(
            "agent_id {:?} appears twice",
            expected[0].1
        )));
    }

    // Per-connection handshake deadline: a fraction of the overall timeout,
    // capped at 5s so a long overall timeout doesn't make individual silent
    // peers hang forever.
    let per_conn = overall_timeout
        .checked_div(2)
        .unwrap_or(DEFAULT_HANDSHAKE_TIMEOUT)
        .min(DEFAULT_HANDSHAKE_TIMEOUT);

    let mut handshakes: JoinSet<Result<Handshaked, PlayerError>> = JoinSet::new();
    let mut out: [Option<TcpPlayer>; 2] = [None, None];
    let mut remaining: usize = expected.len();

    let result = timeout(overall_timeout, async {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    let (stream, _peer) = accept?;
                    handshakes.spawn(handshake_one(stream, per_conn));
                }
                Some(joined) = handshakes.join_next(), if !handshakes.is_empty() => {
                    let handshaked = match joined {
                        Ok(Ok(h)) => h,
                        Ok(Err(e)) => {
                            tracing::debug!(error = %e, "handshake failed; dropping peer");
                            continue;
                        }
                        Err(je) => {
                            tracing::warn!(error = %je, "handshake task panicked");
                            continue;
                        }
                    };

                    // Look up the agent_id against `expected`.
                    let Some(slot_index) = expected.iter().position(|(_, id)| id == &handshaked.agent_id) else {
                        reject_peer(handshaked.writer, "unknown agent_id").await;
                        continue;
                    };
                    let assigned_slot = expected[slot_index].0;
                    let array_index = match assigned_slot {
                        PlayerSlot::Player1 => 0,
                        PlayerSlot::Player2 => 1,
                        _ => unreachable!("PlayerSlot has only Player1 and Player2"),
                    };
                    if out[array_index].is_some() {
                        reject_peer(handshaked.writer, "agent_id already claimed").await;
                        continue;
                    }

                    // Send Welcome and build the TcpPlayer.
                    let welcome_bytes = serialize_host_msg(&HostMsg::Welcome {
                        player_slot: assigned_slot,
                    });
                    let mut writer = handshaked.writer;
                    if let Err(e) = writer.write_frame(&welcome_bytes).await {
                        tracing::debug!(error = %e, "failed to write Welcome; dropping peer");
                        continue;
                    }

                    tracing::info!(
                        agent_id = %handshaked.agent_id,
                        slot = ?assigned_slot,
                        "accept_players: peer welcomed"
                    );
                    let identity = PlayerIdentity {
                        name: handshaked.name,
                        author: handshaked.author,
                        agent_id: handshaked.agent_id,
                        slot: assigned_slot,
                    };
                    let player = TcpPlayer::spawn(identity, handshaked.reader, writer, event_sink.clone());
                    out[array_index] = Some(player);

                    remaining -= 1;
                    if remaining == 0 {
                        return Ok::<_, AcceptError>(());
                    }
                }
            }
        }
    })
    .await;

    // Abort any in-flight handshake tasks regardless of outcome — no leaked
    // sockets or background work past the function return.
    handshakes.abort_all();

    match result {
        Ok(Ok(())) => Ok(out),
        Ok(Err(e)) => Err(e),
        Err(_elapsed) => Err(AcceptError::Timeout),
    }
}

// ── TcpPlayer ─────────────────────────────────────────────────────────

/// TCP-backed [`Player`]. Owns a per-connection session task that shuttles
/// frames between the `TcpStream` and a pair of bounded `mpsc` channels.
pub struct TcpPlayer {
    identity: PlayerIdentity,
    /// Outbound: TcpPlayer pushes `HostMsg`, session task pulls and writes.
    host_tx: mpsc::Sender<HostMsg>,
    /// Inbound: session task pushes `BotMsg`, recv() pulls.
    bot_rx: mpsc::Receiver<BotMsg>,
    session: Option<JoinHandle<Result<(), PlayerError>>>,
    /// Sink for observer-facing events (Provisional forwarding lives in
    /// `recv`; Info / RenderCommands forwarding happens here too).
    event_sink: EventSink,
    /// Latest provisional direction (Foundation F2).
    latest_provisional: Option<ProvisionalSlot>,
}

impl TcpPlayer {
    /// Spawn a session task and return a TcpPlayer wired to its channels.
    /// Used by [`accept_players`] after handshake completion.
    fn spawn<R, W>(
        identity: PlayerIdentity,
        reader: FrameReader<R>,
        writer: FrameWriter<W>,
        event_sink: EventSink,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (host_tx, host_rx) = mpsc::channel(DEFAULT_CHANNEL_DEPTH);
        let (bot_tx, bot_rx) = mpsc::channel(DEFAULT_CHANNEL_DEPTH);
        let session = tokio::spawn(session_task(reader, writer, host_rx, bot_tx));
        Self {
            identity,
            host_tx,
            bot_rx,
            session: Some(session),
            event_sink,
            latest_provisional: None,
        }
    }

    /// Reap the session task handle, surfacing its exit reason.
    async fn reap_session(&mut self) -> Result<(), PlayerError> {
        let Some(handle) = self.session.take() else {
            return Ok(());
        };
        match handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(je) => Err(PlayerError::TransportError(format!(
                "session task panicked: {je}"
            ))),
        }
    }

    /// Validate that an incoming bot message's `player` tag matches the slot
    /// assigned to this connection. Each TcpPlayer owns exactly one slot
    /// (hivemind is rejected at accept time), so anything else is a protocol
    /// violation.
    fn check_slot(&self, msg_player: PlayerSlot) -> Result<(), PlayerError> {
        if msg_player == self.identity.slot {
            Ok(())
        } else {
            Err(PlayerError::ProtocolError(format!(
                "message tagged for {msg_player:?}, but this connection owns {:?}",
                self.identity.slot
            )))
        }
    }
}

#[async_trait]
impl Player for TcpPlayer {
    fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError> {
        // Whole-turn boundary: a new Go/GoState invalidates any stored
        // provisional from the previous turn (Foundation F2).
        if matches!(msg, HostMsg::Go { .. } | HostMsg::GoState { .. }) {
            self.latest_provisional = None;
        }
        self.host_tx
            .send(msg)
            .await
            .map_err(|_| PlayerError::TransportError("session task closed".into()))
    }

    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
        loop {
            let Some(msg) = self.bot_rx.recv().await else {
                // Session task closed bot_tx → it has exited.
                self.reap_session().await?;
                return Ok(None);
            };
            match msg {
                BotMsg::Provisional {
                    direction,
                    player,
                    turn,
                    state_hash,
                } => {
                    self.check_slot(player)?;
                    self.latest_provisional = Some(ProvisionalSlot {
                        direction,
                        turn,
                        state_hash,
                    });
                    self.event_sink.emit(MatchEvent::BotProvisional {
                        sender: player,
                        turn,
                        state_hash,
                        direction,
                    });
                    continue;
                },
                BotMsg::Info(info) => {
                    // Sideband; `info.player` is the analysis subject, not
                    // a sender claim. Bots may analyze either player, so no
                    // slot check here. See protocol.md:202-212.
                    let turn = info.turn;
                    let state_hash = info.state_hash;
                    self.event_sink.emit(MatchEvent::BotInfo {
                        sender: self.identity.slot,
                        turn,
                        state_hash,
                        info,
                    });
                    continue;
                },
                BotMsg::RenderCommands { .. } => {
                    // Sideband; `player` field is the analysis subject, not
                    // a sender claim. No slot check. Today we drop it
                    // (no `MatchEvent::BotRenderCommands` variant yet);
                    // wiring that is a separate concern.
                    continue;
                },
                BotMsg::Action { player, .. } => {
                    self.check_slot(player)?;
                    return Ok(Some(msg));
                },
                BotMsg::Identify { .. } => {
                    return Err(PlayerError::ProtocolError(
                        "Identify after handshake".into(),
                    ));
                },
                other => return Ok(Some(other)),
            }
        }
    }

    fn take_provisional(&mut self, expected_turn: u16, expected_hash: u64) -> Option<Direction> {
        ProvisionalSlot::match_take(&mut self.latest_provisional, expected_turn, expected_hash)
    }

    async fn close(self: Box<Self>) -> Result<(), PlayerError> {
        let Self {
            identity: _,
            host_tx,
            bot_rx,
            session,
            event_sink: _,
            latest_provisional: _,
        } = *self;

        // Best-effort polite Stop. Drop the receiver eagerly so the session
        // task wakes up if it's parked on bot_tx.send (full inbound channel).
        drop(bot_rx);
        let _ = host_tx.send(HostMsg::Stop).await;
        // Dropping host_tx signals "no more host messages" to the session task,
        // which then drains anything queued and exits.
        drop(host_tx);

        let Some(mut handle) = session else {
            return Ok(());
        };
        tokio::select! {
            joined = &mut handle => match joined {
                Ok(_) => Ok(()),
                Err(je) => Err(PlayerError::TransportError(format!(
                    "session task panicked: {je}"
                ))),
            },
            () = tokio::time::sleep(CLOSE_GRACE) => {
                handle.abort();
                Ok(())
            }
        }
    }
}

// ── Session task ──────────────────────────────────────────────────────

/// Per-connection session loop.
///
/// Reads frames from `reader`, decodes via `pyrat_protocol::codec`, and
/// pushes `BotMsg` onto `bot_tx`. In parallel pulls `HostMsg` from `host_rx`,
/// serializes, and writes through `writer`.
///
/// Exits when either:
/// - `host_rx` is closed (TcpPlayer dropped or `close` called) → drains any
///   queued outbound and returns `Ok(())`.
/// - The peer closes the socket cleanly → returns `Ok(())` (caller surfaces
///   as `recv() -> Ok(None)` once `bot_tx` is dropped).
/// - Any I/O or codec error → returns the corresponding `PlayerError`.
async fn session_task<R, W>(
    mut reader: FrameReader<R>,
    mut writer: FrameWriter<W>,
    mut host_rx: mpsc::Receiver<HostMsg>,
    bot_tx: mpsc::Sender<BotMsg>,
) -> Result<(), PlayerError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            biased;
            // Outbound: host has a message to write. Cancellation here would
            // drop a pulled message; we never cancel this branch (the read
            // branch is selected against directly without a wrapper that
            // could discard).
            host_msg = host_rx.recv() => {
                let Some(msg) = host_msg else {
                    // Host side closed — drain any in-flight inbound is not
                    // our concern (we don't drain inbound on close per spec).
                    return Ok(());
                };
                let bytes = serialize_host_msg(&msg);
                if let Err(e) = writer.write_frame(&bytes).await {
                    return Err(map_frame_error(e, "outbound write"));
                }
            }
            // Inbound: read next frame from peer, decode, push to bot_tx.
            // FrameReader::read_frame is documented cancel-safe.
            frame = reader.read_frame() => {
                let bytes = match frame {
                    Ok(b) => b.to_vec(),
                    Err(FrameError::Disconnected) => return Ok(()),
                    Err(e) => return Err(map_frame_error(e, "inbound read")),
                };
                let packet = root_as_host_or_bot(&bytes)?;
                if bot_tx.send(packet).await.is_err() {
                    // recv side dropped; nothing more to do.
                    return Ok(());
                }
            }
        }
    }
}

/// Decode a frame as a `BotPacket`. Wraps the raw flatbuffers root + extract
/// pair into one fallible call so the session loop reads cleanly.
fn root_as_host_or_bot(bytes: &[u8]) -> Result<BotMsg, PlayerError> {
    let packet = flatbuffers::root::<BotPacket>(bytes)
        .map_err(|e| PlayerError::ProtocolError(format!("packet decode: {e}")))?;
    extract_bot_msg(&packet).map_err(|e| PlayerError::ProtocolError(format!("extract: {e}")))
}

/// Map a [`FrameError`] into a [`PlayerError`]. Frame errors are either I/O
/// (transport) or oversize/malformed (protocol).
fn map_frame_error(err: FrameError, ctx: &str) -> PlayerError {
    match err {
        FrameError::PayloadTooLarge { .. } => PlayerError::ProtocolError(format!("{ctx}: {err}")),
        _ => PlayerError::TransportError(format!("{ctx}: {err}")),
    }
}
