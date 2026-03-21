use std::path::PathBuf;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing::debug;

use crate::session::messages::OwnedOptionDef;
use crate::session::{extract_bot_packet, BotPayload};
use pyrat_wire::framing::FrameReader;

use super::config::BotConfig;
use super::launch::{launch_bots, BotProcesses, LaunchError};

// ── Public types ─────────────────────────────────────

/// Information extracted from a bot's Identify message.
#[derive(Debug)]
pub struct ProbeResult {
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub options: Vec<OwnedOptionDef>,
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
    #[error("protocol error: {0}")]
    ProtocolError(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Probe implementation ─────────────────────────────

const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll `BotProcesses` until a child exits, then return its agent_id.
async fn poll_process_exit(procs: &mut BotProcesses) -> String {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
        interval.tick().await;
        if let Some(agent_id) = procs.try_exited() {
            return agent_id.to_owned();
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

    // 3. Accept one connection — wait as long as the process is alive
    let stream = tokio::select! {
        result = listener.accept() => result?.0,
        dead = poll_process_exit(&mut procs) => {
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

    // 5. Parse
    let (_msg_type, payload) = extract_bot_packet(buf).map_err(ProbeError::ProtocolError)?;

    match payload {
        BotPayload::Identify {
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
        _ => Err(ProbeError::ProtocolError(
            "expected Identify as first message".into(),
        )),
    }

    // procs drops here, killing the bot process
}
