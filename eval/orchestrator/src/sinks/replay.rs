//! Forensic replay sink. Buffers events per match and flushes a JSON file
//! when the match completes.
//!
//! Per-match state lives in `parking_lot::Mutex<HashMap<MatchId, …>>`. One
//! `Arc<ReplaySink>` serves all concurrent matches: a single shared
//! `Vec<ReplayEvent>` would interleave events from different matches.
//! Mutex held only across `HashMap` ops; the JSON serialise + file write
//! happens on the buffer that was removed under the lock, *outside* the
//! lock.
//!
//! Lifecycle:
//! - `on_match_started`: insert a fresh buffer keyed by `descriptor.match_id()`.
//! - `on_match_event`: append `ReplayEvent::from(event)`. The first
//!   `MatchEvent::MatchStarted { config }` populates `match_config` (the
//!   wire-level `MatchConfig` is only available through the host event;
//!   we don't have it at `on_match_started` time).
//! - `on_match_finished`: synthesise terminal `ReplayEvent::MatchOver`
//!   from `outcome.result` (the host's `MatchOver` is suppressed
//!   upstream), flush the JSON, drop the buffer.
//! - `on_match_failed` / `on_match_abandoned`: drop the buffer without
//!   writing. **No partial replays.**
//!
//! Missing-buffer behaviour: if no entry exists for `id` (Optional
//! `on_match_started` failed earlier and the match proceeded anyway), all
//! later callbacks no-op silently. The earlier failure was already logged
//! at `warn` by the composite; flooding the log with per-event warnings
//! would be noise.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};
use crate::replay_event::{ReplayEvent, ReplayMatchConfig, ReplayMatchResult};
use crate::sink::{MatchSink, SinkError};

/// JSON shape of one replay file. Wraps the per-match metadata around the
/// flat event list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFile {
    /// Free-form version string (e.g. `pyrat-orchestrator/0.1.0`).
    pub engine_version: String,
    /// `MatchConfig` extracted from the host's `MatchEvent::MatchStarted`.
    /// `None` if the match failed before the host emitted `MatchStarted`
    /// (early protocol fault), which never reaches a clean
    /// `on_match_finished` flush, so production files always have it.
    pub match_config: Option<ReplayMatchConfig>,
    /// Seed pinned by the descriptor at submission.
    pub seed: u64,
    /// Every host event observed, in arrival order. The terminal entry is
    /// always `ReplayEvent::MatchOver` (synthesised by the sink; the
    /// host's `MatchOver` is suppressed upstream).
    pub events: Vec<ReplayEvent>,
    /// Final `MatchResult`, in DTO form.
    pub result: ReplayMatchResult,
}

/// Persistence strategy for replay files. Dyn-friendly so callers can hand
/// in an in-memory writer for tests, a directory writer in production, a
/// rotating one for long-running services, etc.
pub trait ReplayWriter: Send + Sync {
    /// Persist the JSON payload for `id`. Errors propagate as
    /// [`SinkError`].
    fn write(&self, id: MatchId, payload: &str) -> std::io::Result<()>;
}

/// Writes one file per match into `dir`, named `match-{id}.json`. Pretty
/// JSON for human inspection.
pub struct DirectoryWriter {
    dir: PathBuf,
}

impl DirectoryWriter {
    /// Ensures `dir` exists once, up front, so the per-match write path
    /// is one syscall instead of two.
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl ReplayWriter for DirectoryWriter {
    fn write(&self, id: MatchId, payload: &str) -> std::io::Result<()> {
        let path = self.dir.join(format!("match-{}.json", id.0));
        std::fs::write(path, payload)
    }
}

/// In-memory writer for tests. Records every flush keyed by `MatchId`.
pub struct MemoryWriter {
    files: Mutex<HashMap<MatchId, String>>,
}

impl MemoryWriter {
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    pub fn count(&self) -> usize {
        self.files.lock().len()
    }

    pub fn get(&self, id: MatchId) -> Option<String> {
        self.files.lock().get(&id).cloned()
    }

    pub fn ids(&self) -> Vec<MatchId> {
        let mut v: Vec<_> = self.files.lock().keys().copied().collect();
        v.sort();
        v
    }
}

impl Default for MemoryWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWriter for MemoryWriter {
    fn write(&self, id: MatchId, payload: &str) -> std::io::Result<()> {
        self.files.lock().insert(id, payload.to_owned());
        Ok(())
    }
}

struct ReplayBuffer {
    events: Vec<ReplayEvent>,
    match_config: Option<ReplayMatchConfig>,
    seed: u64,
}

/// Per-match buffered replay sink. Optional in role: losing a replay file
/// is a telemetry concern, never a durable-record concern.
pub struct ReplaySink {
    writer: Arc<dyn ReplayWriter>,
    state: Mutex<HashMap<MatchId, ReplayBuffer>>,
    engine_version: String,
}

impl ReplaySink {
    pub fn new(writer: Arc<dyn ReplayWriter>) -> Self {
        Self {
            writer,
            state: Mutex::new(HashMap::new()),
            engine_version: format!("pyrat-orchestrator/{}", env!("CARGO_PKG_VERSION")),
        }
    }

    /// Override the version string written into every replay file. Useful
    /// for embedding application identity (e.g. `"pyrat-eval/0.1.0"`).
    pub fn with_engine_version(mut self, version: impl Into<String>) -> Self {
        self.engine_version = version.into();
        self
    }

    /// True iff a buffer is currently held for this match. Test hook.
    pub fn has_buffer(&self, id: MatchId) -> bool {
        self.state.lock().contains_key(&id)
    }

    /// Number of buffers currently held. Test hook.
    pub fn buffer_count(&self) -> usize {
        self.state.lock().len()
    }
}

#[async_trait]
impl<D: Descriptor> MatchSink<D> for ReplaySink {
    async fn on_match_started(
        &self,
        descriptor: &D,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        let id = descriptor.match_id();
        let buffer = ReplayBuffer {
            events: Vec::new(),
            match_config: None,
            seed: descriptor.seed(),
        };
        self.state.lock().insert(id, buffer);
        Ok(())
    }

    async fn on_match_event(&self, id: MatchId, event: &MatchEvent) -> Result<(), SinkError> {
        let mut state = self.state.lock();
        let Some(buf) = state.get_mut(&id) else {
            return Ok(());
        };
        if let MatchEvent::MatchStarted { config } = event {
            // Only the first `MatchStarted` populates `match_config`.
            // Subsequent ones (shouldn't happen, but defensive) are
            // ignored to keep the seed-derived header stable.
            if buf.match_config.is_none() {
                buf.match_config = Some(config.into());
            }
        }
        buf.events.push(ReplayEvent::from(event));
        Ok(())
    }

    async fn on_match_finished(&self, outcome: &MatchOutcome<D>) -> Result<(), SinkError> {
        let id = outcome.descriptor.match_id();
        let buf = match self.state.lock().remove(&id) {
            Some(b) => b,
            None => return Ok(()),
        };
        // Synthesise terminal MatchOver since the host's was suppressed.
        let result_dto: ReplayMatchResult = (&outcome.result).into();
        let mut events = buf.events;
        events.push(ReplayEvent::MatchOver {
            result: result_dto.clone(),
        });
        let file = ReplayFile {
            engine_version: self.engine_version.clone(),
            match_config: buf.match_config,
            seed: buf.seed,
            events,
            result: result_dto,
        };
        let payload = serde_json::to_string_pretty(&file).map_err(|e| SinkError {
            source: anyhow::anyhow!("serialise replay {id}: {e}"),
        })?;
        if let Err(e) = self.writer.write(id, &payload) {
            // Optional sink error; never propagated as durable. The
            // composite layer will log + count if this is wired Optional.
            return Err(SinkError {
                source: anyhow::anyhow!("write replay {id}: {e}"),
            });
        }
        Ok(())
    }

    async fn on_match_failed(&self, failure: &MatchFailure<D>) -> Result<(), SinkError> {
        // Drop the buffer without writing. No partial replays on failure.
        let id = failure.descriptor.match_id();
        if self.state.lock().remove(&id).is_none() {
            // Defensive: nothing to drop.
            warn!(match_id = %id, "replay sink: on_match_failed for unknown id");
        }
        Ok(())
    }

    async fn on_match_abandoned(&self, id: MatchId) -> Result<(), SinkError> {
        // Same outcome as `on_match_failed`: drop the buffer, no write.
        // Hook is separate so the cause stays honest (another sink errored,
        // not this one).
        self.state.lock().remove(&id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::SystemTime;

    use pyrat::{Coordinates, Direction};
    use pyrat_host::match_host::MatchResult;
    use pyrat_host::wire::{GameResult, Player as PlayerSlot};

    use super::*;
    use crate::descriptor::AdHocDescriptor;
    use crate::outcome::{FailureReason, MatchFailure, MatchOutcome};

    fn ad_hoc(id: u64, seed: u64) -> AdHocDescriptor {
        AdHocDescriptor {
            match_id: MatchId(id),
            seed,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn identity(slot: PlayerSlot) -> PlayerIdentity {
        PlayerIdentity {
            name: "n".into(),
            author: "a".into(),
            agent_id: "id".into(),
            slot,
        }
    }

    fn outcome(desc: AdHocDescriptor) -> MatchOutcome<AdHocDescriptor> {
        MatchOutcome {
            descriptor: desc,
            started_at: SystemTime::UNIX_EPOCH,
            finished_at: SystemTime::UNIX_EPOCH,
            result: MatchResult {
                result: GameResult::Draw,
                player1_score: 0.0,
                player2_score: 0.0,
                turns_played: 0,
            },
            players: [identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
        }
    }

    fn failure(desc: AdHocDescriptor) -> MatchFailure<AdHocDescriptor> {
        MatchFailure {
            descriptor: desc,
            started_at: None,
            failed_at: SystemTime::UNIX_EPOCH,
            reason: FailureReason::Cancelled,
            players: None,
            durable_record: false,
        }
    }

    #[tokio::test]
    async fn finished_writes_one_file_and_drops_buffer() {
        let writer = Arc::new(MemoryWriter::new());
        let sink = ReplaySink::new(writer.clone());
        let desc = ad_hoc(1, 42);

        sink.on_match_started(
            &desc,
            &[identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
        )
        .await
        .unwrap();
        assert!(sink.has_buffer(MatchId(1)));

        sink.on_match_finished(&outcome(desc)).await.unwrap();

        assert!(!sink.has_buffer(MatchId(1)));
        assert_eq!(writer.count(), 1);
        let payload = writer.get(MatchId(1)).unwrap();
        let parsed: ReplayFile = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed.seed, 42);
        // Last event must be the synthesised MatchOver.
        assert!(matches!(
            parsed.events.last(),
            Some(ReplayEvent::MatchOver { .. })
        ));
    }

    #[tokio::test]
    async fn failed_drops_buffer_without_writing() {
        let writer = Arc::new(MemoryWriter::new());
        let sink = ReplaySink::new(writer.clone());
        let desc = ad_hoc(2, 7);

        sink.on_match_started(
            &desc,
            &[identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
        )
        .await
        .unwrap();
        sink.on_match_failed(&failure(desc)).await.unwrap();
        assert!(!sink.has_buffer(MatchId(2)));
        assert_eq!(writer.count(), 0, "no file should be written on failure");
    }

    #[tokio::test]
    async fn abandoned_drops_buffer_without_writing() {
        let writer = Arc::new(MemoryWriter::new());
        let sink_inner = ReplaySink::new(writer.clone());
        let sink: &dyn MatchSink<AdHocDescriptor> = &sink_inner;
        let desc = ad_hoc(3, 8);

        sink.on_match_started(
            &desc,
            &[identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
        )
        .await
        .unwrap();
        sink.on_match_abandoned(MatchId(3)).await.unwrap();
        assert!(!sink_inner.has_buffer(MatchId(3)));
        assert_eq!(writer.count(), 0);
    }

    #[tokio::test]
    async fn missing_buffer_callbacks_are_silent_noops() {
        let writer = Arc::new(MemoryWriter::new());
        let sink_inner = ReplaySink::new(writer.clone());
        let sink: &dyn MatchSink<AdHocDescriptor> = &sink_inner;
        // No on_match_started.
        sink.on_match_event(MatchId(99), &MatchEvent::PreprocessingStarted)
            .await
            .unwrap();
        sink.on_match_finished(&outcome(ad_hoc(99, 0)))
            .await
            .unwrap();
        sink.on_match_abandoned(MatchId(99)).await.unwrap();
        assert_eq!(writer.count(), 0);
        assert_eq!(sink_inner.buffer_count(), 0);
    }

    /// Concurrent matches must keep their event streams separate. Pins the
    /// per-match keyed-buffer invariant: a single shared `Vec<ReplayEvent>`
    /// would interleave.
    #[tokio::test]
    async fn concurrent_matches_have_separate_buffers() {
        let writer = Arc::new(MemoryWriter::new());
        let sink_inner = ReplaySink::new(writer.clone());
        // Bind to the trait-object form once: `on_match_event` doesn't
        // take a `&D`, so otherwise the compiler can't infer which `D` to
        // dispatch to (the impl is generic over every Descriptor).
        let sink: &dyn MatchSink<AdHocDescriptor> = &sink_inner;

        // Start three matches with distinguishable seeds.
        for (id, seed) in [(10u64, 1u64), (11, 2), (12, 3)] {
            let desc = ad_hoc(id, seed);
            sink.on_match_started(
                &desc,
                &[identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
            )
            .await
            .unwrap();
        }

        // Interleave events: only id=10 sees a BotTimeout for player1; only
        // id=11 sees BotTimeout for player2; id=12 sees no BotTimeout.
        sink.on_match_event(
            MatchId(10),
            &MatchEvent::BotTimeout {
                player: PlayerSlot::Player1,
                turn: 1,
            },
        )
        .await
        .unwrap();
        sink.on_match_event(
            MatchId(11),
            &MatchEvent::BotTimeout {
                player: PlayerSlot::Player2,
                turn: 2,
            },
        )
        .await
        .unwrap();
        // Cross-traffic that should NOT show up in any buffer for that id.
        sink.on_match_event(MatchId(10), &MatchEvent::PreprocessingStarted)
            .await
            .unwrap();

        // Finish all three.
        for id in [10u64, 11, 12] {
            let seed = id - 9;
            let desc = ad_hoc(id, seed);
            sink.on_match_finished(&outcome(desc)).await.unwrap();
        }

        let payload10: ReplayFile =
            serde_json::from_str(&writer.get(MatchId(10)).unwrap()).unwrap();
        let payload11: ReplayFile =
            serde_json::from_str(&writer.get(MatchId(11)).unwrap()).unwrap();
        let payload12: ReplayFile =
            serde_json::from_str(&writer.get(MatchId(12)).unwrap()).unwrap();

        assert_eq!(payload10.seed, 1);
        assert_eq!(payload11.seed, 2);
        assert_eq!(payload12.seed, 3);

        // id=10 saw BotTimeout(P1) and PreprocessingStarted (plus synthesised MatchOver).
        let bot_timeout_count = payload10
            .events
            .iter()
            .filter(|e| matches!(e, ReplayEvent::BotTimeout { player, .. } if *player == PlayerSlot::Player1.0))
            .count();
        assert_eq!(
            bot_timeout_count, 1,
            "id=10 should see exactly one P1 BotTimeout"
        );
        assert!(payload10
            .events
            .iter()
            .any(|e| matches!(e, ReplayEvent::PreprocessingStarted)));
        // id=10 should not see id=11's BotTimeout.
        let p2_timeout_in_10 = payload10
            .events
            .iter()
            .filter(|e| matches!(e, ReplayEvent::BotTimeout { player, .. } if *player == PlayerSlot::Player2.0))
            .count();
        assert_eq!(p2_timeout_in_10, 0, "id=10 must NOT see id=11's BotTimeout");

        // id=11 saw exactly one BotTimeout(P2).
        let p2_timeout_in_11 = payload11
            .events
            .iter()
            .filter(|e| matches!(e, ReplayEvent::BotTimeout { player, .. } if *player == PlayerSlot::Player2.0))
            .count();
        assert_eq!(p2_timeout_in_11, 1);

        // id=12 saw only the synthesised MatchOver.
        assert_eq!(payload12.events.len(), 1);
        assert!(matches!(payload12.events[0], ReplayEvent::MatchOver { .. }));
    }

    /// `MatchStarted { config }` populates the file's `match_config` once.
    /// A second `MatchStarted` (defensive; shouldn't happen) doesn't
    /// overwrite. Pins the seed-derived header against late host events.
    #[tokio::test]
    async fn match_config_captured_from_first_match_started() {
        let writer = Arc::new(MemoryWriter::new());
        let sink_inner = ReplaySink::new(writer.clone());
        let sink: &dyn MatchSink<AdHocDescriptor> = &sink_inner;
        let desc = ad_hoc(20, 99);

        sink.on_match_started(
            &desc,
            &[identity(PlayerSlot::Player1), identity(PlayerSlot::Player2)],
        )
        .await
        .unwrap();

        let cfg = pyrat_protocol::MatchConfig {
            width: 5,
            height: 5,
            max_turns: 100,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            timing: pyrat_host::wire::TimingMode::Wait,
            move_timeout_ms: 500,
            preprocessing_timeout_ms: 1000,
        };
        sink.on_match_event(MatchId(20), &MatchEvent::MatchStarted { config: cfg })
            .await
            .unwrap();
        // Append a later event so the file isn't all-MatchStarted.
        sink.on_match_event(
            MatchId(20),
            &MatchEvent::BotProvisional {
                sender: PlayerSlot::Player1,
                turn: 1,
                state_hash: 0xCAFE,
                direction: Direction::Up,
            },
        )
        .await
        .unwrap();
        sink.on_match_finished(&outcome(desc)).await.unwrap();

        let parsed: ReplayFile = serde_json::from_str(&writer.get(MatchId(20)).unwrap()).unwrap();
        assert!(parsed.match_config.is_some());
        assert_eq!(parsed.match_config.as_ref().unwrap().width, 5);
        assert_eq!(parsed.seed, 99);
    }
}
