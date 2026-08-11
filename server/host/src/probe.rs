//! One-shot bot probing.
//!
//! [`probe_bot`] stops after `Identify` and returns the declared metadata for
//! configuration UIs. [`preflight_bot_in_slot`] drives the real tournament
//! handshake through `Identify -> Welcome -> Configure -> Ready`, including
//! the initial engine-state hash check, without paying for preprocessing or
//! playing a match.

use std::path::PathBuf;
use std::time::Duration;

use pyrat::GameState;
use tokio::net::TcpListener;
use tracing::debug;

use pyrat_protocol::{extract_bot_msg, BotMsg, HostMsg, MatchConfig, OptionDef};
use pyrat_wire::framing::FrameReader;
use pyrat_wire::{BotPacket, Player as PlayerSlot, TimingMode};

use crate::launch::{launch_bots, BotConfig, BotProcesses, LaunchError};
use crate::match_config::build_match_config;
use crate::player::{accept_players, AcceptError, EventSink, Player};

// ── Public types ─────────────────────────────────────

/// Information extracted from a bot's Identify message.
#[derive(Debug)]
pub struct ProbeResult {
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub options: Vec<OptionDef>,
}

/// The representative game and deadlines used by [`preflight_bot_in_slot`].
///
/// Constructing this from an engine [`GameState`] keeps the wire
/// [`MatchConfig`] and expected hash coupled to the same state, avoiding a
/// preflight that can accidentally validate against a hand-written config.
#[derive(Debug, Clone)]
pub struct PreflightConfig {
    match_config: MatchConfig,
    expected_state_hash: u64,
    options: Vec<(String, String)>,
    connect_timeout: Duration,
    ready_timeout: Duration,
}

impl PreflightConfig {
    /// Build a preflight from a deterministic representative game.
    ///
    /// `connect_timeout` covers process startup plus `Identify -> Welcome`;
    /// `ready_timeout` covers the subsequent `Configure -> Ready` exchange.
    #[must_use]
    pub fn new(
        game: &GameState,
        timing: TimingMode,
        move_timeout_ms: u32,
        preprocessing_timeout_ms: u32,
        connect_timeout: Duration,
        ready_timeout: Duration,
    ) -> Self {
        Self {
            match_config: build_match_config(
                game,
                timing,
                move_timeout_ms,
                preprocessing_timeout_ms,
            ),
            expected_state_hash: game.state_hash(),
            options: Vec::new(),
            connect_timeout,
            ready_timeout,
        }
    }

    /// Apply option overrides during the compatibility check.
    #[must_use]
    pub fn with_options(mut self, options: Vec<(String, String)>) -> Self {
        self.options = options;
        self
    }
}

/// What can go wrong when probing a bot.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("failed to spawn bot: {0}")]
    SpawnFailed(#[from] LaunchError),
    #[error("bot process exited before connecting (agent: {0})")]
    ProcessExited(String),
    #[error("no Identify within {0:?}")]
    IdentifyTimeout(Duration),
    #[error("no Ready within {0:?}")]
    ReadyTimeout(Duration),
    #[error("bot disconnected before Ready")]
    DisconnectedBeforeReady,
    #[error("ready hash mismatch: expected {expected:#x}, got {got:#x}")]
    ReadyHashMismatch { expected: u64, got: u64 },
    #[error("protocol error: {0}")]
    ProtocolError(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Probe implementation ─────────────────────────────

const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll `BotProcesses` until a child exits, then return its agent_id.
async fn poll_process_exit(procs: &BotProcesses) -> String {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
        interval.tick().await;
        if let Some(info) = procs.try_exited() {
            return info.agent_id;
        }
    }
}

/// Spawn a bot, read its Identify message, and return the declared metadata.
///
/// Waits indefinitely for the bot to connect as long as the process is alive.
/// If the process exits (build failure, crash), fails immediately.
///
/// The bot process is killed when this function returns (via `BotProcesses` drop).
pub async fn probe_bot(
    run_command: String,
    working_dir: String,
    agent_id: String,
) -> Result<ProbeResult, ProbeError> {
    // 1. Bind a free port
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    debug!(port, agent_id, "probe: listening");

    // 2. Spawn bot (RAII: killed on drop)
    let mut procs = launch_bots(
        &[BotConfig {
            run_command,
            working_dir: PathBuf::from(&working_dir),
            agent_id: agent_id.clone(),
        }],
        port,
    )?;

    // Drain stderr so a noisy cold `cargo`/`uv` build can't fill the OS pipe
    // buffer and block the bot before it connects — the deadlock that would
    // otherwise hang warmup during the exact "make startup calm" phase.
    procs.drain_stderr_to_tracing();

    // 3. Accept one connection — wait as long as the process is alive
    let stream = tokio::select! {
        result = listener.accept() => result?.0,
        dead = poll_process_exit(&procs) => {
            return Err(ProbeError::ProcessExited(dead));
        }
    };

    let (read_half, _write_half) = tokio::io::split(stream);
    let mut reader = FrameReader::with_default_max(read_half);

    // 4. Read one frame (Identify) — 30s safety net for hung-after-connect bots
    let buf = tokio::time::timeout(IDENTIFY_TIMEOUT, reader.read_frame())
        .await
        .map_err(|_| ProbeError::IdentifyTimeout(IDENTIFY_TIMEOUT))?
        .map_err(|e| ProbeError::ProtocolError(e.to_string()))?;

    // 5. Parse via the canonical pyrat-protocol codec.
    let packet = flatbuffers::root::<BotPacket>(buf)
        .map_err(|e| ProbeError::ProtocolError(format!("packet decode: {e}")))?;
    let msg =
        extract_bot_msg(&packet).map_err(|e| ProbeError::ProtocolError(format!("extract: {e}")))?;

    match msg {
        BotMsg::Identify {
            name,
            author,
            options,
            agent_id: wire_agent_id,
        } => {
            let resolved_id = if wire_agent_id.is_empty() {
                agent_id
            } else {
                wire_agent_id
            };
            Ok(ProbeResult {
                name,
                author,
                agent_id: resolved_id,
                options,
            })
        },
        other => Err(ProbeError::ProtocolError(format!(
            "expected Identify as first message, got {other:?}"
        ))),
    }

    // procs drops here, killing the bot process
}

/// Spawn a bot and verify the complete pre-match compatibility handshake.
///
/// This follows the same transport path as a real subprocess match:
/// Compatibility wrapper that assigns `Player1`.
pub async fn preflight_bot(
    run_command: String,
    working_dir: String,
    agent_id: String,
    config: PreflightConfig,
) -> Result<(), ProbeError> {
    preflight_bot_in_slot(
        run_command,
        working_dir,
        agent_id,
        PlayerSlot::Player1,
        config,
    )
    .await
}

/// Spawn a bot and verify the complete pre-match compatibility handshake in
/// one explicit seat.
///
/// [`accept_players`] validates `Identify`, assigns `assigned_slot`, and sends
/// `Welcome`; the probe then sends `Configure` and requires a `Ready` carrying
/// the representative engine state's exact hash. Calling this once for each
/// seat catches bots whose setup behavior incorrectly depends on their slot.
/// The bot process is killed when this function returns. Dropping the future
/// is cancellation-safe via [`BotProcesses`]' RAII cleanup.
pub async fn preflight_bot_in_slot(
    run_command: String,
    working_dir: String,
    agent_id: String,
    assigned_slot: PlayerSlot,
    config: PreflightConfig,
) -> Result<(), ProbeError> {
    let player_index = match assigned_slot {
        PlayerSlot::Player1 => 0,
        PlayerSlot::Player2 => 1,
        other => {
            return Err(ProbeError::ProtocolError(format!(
                "invalid preflight player slot: {other:?}"
            )));
        },
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    debug!(port, agent_id, "preflight: listening");

    let mut procs = launch_bots(
        &[BotConfig {
            run_command,
            working_dir: PathBuf::from(&working_dir),
            agent_id: agent_id.clone(),
        }],
        port,
    )?;
    procs.drain_stderr_to_tracing();

    let expected = [(assigned_slot, agent_id.clone())];
    let mut accepted = tokio::select! {
        result = accept_players(
            &listener,
            &expected,
            EventSink::noop(),
            config.connect_timeout,
        ) => result,
        dead = poll_process_exit(&procs) => {
            return Err(ProbeError::ProcessExited(dead));
        }
    }
    .map_err(|error| match error {
        AcceptError::Timeout => ProbeError::IdentifyTimeout(config.connect_timeout),
        other => ProbeError::ProtocolError(format!("Identify -> Welcome: {other}")),
    })?;

    let mut player = accepted[player_index].take().ok_or_else(|| {
        ProbeError::ProtocolError(format!(
            "Identify -> Welcome returned no {assigned_slot:?} handle"
        ))
    })?;

    verify_ready(&mut player, config).await
    // `player` and `procs` drop here. Closing the host side wakes the TCP
    // session task; BotProcesses then kills the subprocess tree.
}

/// Drive an already-welcomed player through the compatibility-bearing part
/// of setup. Kept separate so the protocol/error behavior is unit-testable
/// without launching a subprocess.
async fn verify_ready(player: &mut dyn Player, config: PreflightConfig) -> Result<(), ProbeError> {
    let PreflightConfig {
        match_config,
        expected_state_hash,
        options,
        connect_timeout: _,
        ready_timeout,
    } = config;

    player
        .send(HostMsg::Configure {
            options,
            match_config: Box::new(match_config),
        })
        .await
        .map_err(|error| ProbeError::ProtocolError(format!("send Configure: {error}")))?;

    let message = tokio::time::timeout(ready_timeout, player.recv())
        .await
        .map_err(|_| ProbeError::ReadyTimeout(ready_timeout))?
        .map_err(|error| ProbeError::ProtocolError(format!("receive Ready: {error}")))?
        .ok_or(ProbeError::DisconnectedBeforeReady)?;

    match message {
        BotMsg::Ready { state_hash } if state_hash == expected_state_hash => Ok(()),
        BotMsg::Ready { state_hash } => Err(ProbeError::ReadyHashMismatch {
            expected: expected_state_hash,
            got: state_hash,
        }),
        other => Err(ProbeError::ProtocolError(format!(
            "expected Ready after Configure, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use pyrat::{Coordinates, Direction, GameBuilder};

    use super::*;
    use crate::player::{PlayerError, PlayerIdentity};

    struct FakePlayer {
        identity: PlayerIdentity,
        response: Option<Result<Option<BotMsg>, PlayerError>>,
        configured: Arc<Mutex<bool>>,
    }

    impl FakePlayer {
        fn with_response(response: Result<Option<BotMsg>, PlayerError>) -> Self {
            Self::in_slot(PlayerSlot::Player1, response)
        }

        fn in_slot(slot: PlayerSlot, response: Result<Option<BotMsg>, PlayerError>) -> Self {
            Self {
                identity: PlayerIdentity {
                    name: "fake".into(),
                    author: "tests".into(),
                    agent_id: "fake/test".into(),
                    slot,
                },
                response: Some(response),
                configured: Arc::new(Mutex::new(false)),
            }
        }
    }

    #[async_trait]
    impl Player for FakePlayer {
        fn identity(&self) -> &PlayerIdentity {
            &self.identity
        }

        async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError> {
            if matches!(msg, HostMsg::Configure { .. }) {
                *self.configured.lock().expect("configured lock") = true;
                Ok(())
            } else {
                Err(PlayerError::ProtocolError(format!(
                    "expected Configure, got {msg:?}"
                )))
            }
        }

        async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
            self.response.take().expect("single recv")
        }

        fn take_provisional(
            &mut self,
            _expected_turn: u16,
            _expected_hash: u64,
        ) -> Option<Direction> {
            None
        }

        async fn close(self: Box<Self>) -> Result<(), PlayerError> {
            Ok(())
        }
    }

    fn game() -> GameState {
        GameBuilder::new(3, 3)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(Some(42))
            .expect("game")
    }

    fn config(game: &GameState) -> PreflightConfig {
        PreflightConfig::new(
            game,
            TimingMode::Wait,
            200,
            1_000,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
    }

    #[tokio::test]
    async fn verify_ready_accepts_matching_engine_hash() {
        let game = game();
        let mut player = FakePlayer::with_response(Ok(Some(BotMsg::Ready {
            state_hash: game.state_hash(),
        })));
        let configured = Arc::clone(&player.configured);

        verify_ready(&mut player, config(&game))
            .await
            .expect("matching Ready");

        assert!(*configured.lock().expect("configured lock"));
    }

    #[tokio::test]
    async fn verify_ready_rejects_stale_protocol_zero_hash() {
        let game = game();
        let expected = game.state_hash();
        assert_ne!(expected, 0, "fixture must catch the old empty Ready packet");
        let mut player = FakePlayer::with_response(Ok(Some(BotMsg::Ready { state_hash: 0 })));

        let error = verify_ready(&mut player, config(&game))
            .await
            .expect_err("zero hash must fail");

        assert!(matches!(
            error,
            ProbeError::ReadyHashMismatch { expected: got_expected, got: 0 }
                if got_expected == expected
        ));
    }

    #[tokio::test]
    async fn verify_ready_reports_disconnect_before_ready() {
        let game = game();
        let mut player = FakePlayer::with_response(Ok(None));

        let error = verify_ready(&mut player, config(&game))
            .await
            .expect_err("disconnect must fail");

        assert!(matches!(error, ProbeError::DisconnectedBeforeReady));
    }

    #[tokio::test]
    async fn verify_ready_rejects_out_of_sequence_message() {
        let game = game();
        let mut player = FakePlayer::with_response(Ok(Some(BotMsg::PreprocessingDone)));

        let error = verify_ready(&mut player, config(&game))
            .await
            .expect_err("wrong phase message must fail");

        assert!(matches!(error, ProbeError::ProtocolError(message)
            if message.contains("expected Ready after Configure")));
    }

    #[tokio::test]
    async fn paired_preflight_rejects_failure_only_in_flipped_seat() {
        let game = game();
        let mut canonical = FakePlayer::in_slot(
            PlayerSlot::Player1,
            Ok(Some(BotMsg::Ready {
                state_hash: game.state_hash(),
            })),
        );
        verify_ready(&mut canonical, config(&game))
            .await
            .expect("canonical seat should pass");

        let mut flipped = FakePlayer::in_slot(
            PlayerSlot::Player2,
            Ok(Some(BotMsg::Ready { state_hash: 0 })),
        );
        let error = verify_ready(&mut flipped, config(&game))
            .await
            .expect_err("a flipped-seat-only incompatibility must fail preflight");

        assert!(matches!(
            error,
            ProbeError::ReadyHashMismatch { got: 0, .. }
        ));
    }
}
