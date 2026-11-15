use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use pyrat_eval::orchestrator::{
    AttemptDetail, ExecutionPhase, MatchId, TimingFailureReason, TimingGameResult,
};
use pyrat_eval::{
    compare_eta_models_walk_forward, DispatchPolicy, EtaEstimate, EtaModelComparisonPoint,
    EtaState, PlannedSlot, TournamentTimingTrace,
};
use serde::Deserialize;

const CORPUS_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusManifest {
    schema_version: u32,
    title: String,
    cases: Vec<CaseEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseEntry {
    id: String,
    label: String,
    provenance: String,
    description: String,
    trace: PathBuf,
    /// Matched runs with the same family and concurrency are compared as a
    /// policy pair. Absent for the historical single-case ETA corpus.
    #[serde(default)]
    family: Option<String>,
    /// Randomized execution position within one matched block, retained as
    /// provenance rather than interpreted as a metric.
    #[serde(default)]
    run_order: Option<u32>,
    /// Replicated matched blocks that should be summarized together. The
    /// family remains seed-local so policy outcomes are never crossed between
    /// different schedule instances.
    #[serde(default)]
    replicate_group: Option<String>,
    /// Optional final standings exported from the same completed tournament.
    #[serde(default)]
    results: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct ResultsSummary {
    standings: Vec<RatingStanding>,
}

#[derive(Debug, Clone, Deserialize)]
struct RatingStanding {
    player_id: String,
    elo: f64,
    elo_stderr: f64,
}

#[derive(Debug)]
struct CaseAnalysis {
    entry: CaseEntry,
    slots: usize,
    max_parallel: usize,
    event_count: usize,
    duration_seconds: f64,
    timeline: Vec<TimelinePoint>,
    m0: ModelMetrics,
    m1: ModelMetrics,
    m1p: ModelMetrics,
    common_support: CommonSupportMetrics,
    schedule: ScheduleMetrics,
    attempts: Vec<AttemptBar>,
    phase_diagnostics: PhaseDiagnostics,
    outcomes: HashMap<PlannedSlot, AttemptDetail>,
    ratings: BTreeMap<String, RatingStanding>,
}

#[derive(Debug, Clone)]
struct TimelinePoint {
    elapsed_seconds: f64,
    actual_seconds: f64,
    m0_seconds: Option<f64>,
    m1_seconds: Option<f64>,
    m1p_seconds: Option<f64>,
    m0_error_seconds: Option<f64>,
    m1_error_seconds: Option<f64>,
    m1p_error_seconds: Option<f64>,
}

#[derive(Debug, Default)]
struct ModelMetrics {
    ready_points: usize,
    first_ready_seconds: Option<f64>,
    mae_seconds: Option<f64>,
    bias_seconds: Option<f64>,
    jitter_seconds: Option<f64>,
    early_mae_seconds: Option<f64>,
    middle_mae_seconds: Option<f64>,
    tail_mae_seconds: Option<f64>,
}

#[derive(Debug, Default)]
struct CommonSupportMetrics {
    points: usize,
    first_seconds: Option<f64>,
    m0_mae_seconds: Option<f64>,
    m1_mae_seconds: Option<f64>,
    m1p_mae_seconds: Option<f64>,
}

#[derive(Debug, Default)]
struct ScheduleMetrics {
    policy: DispatchPolicy,
    utilization: Option<f64>,
    underfilled_tail_seconds: Option<f64>,
    bot_overlap_seconds: BTreeMap<String, f64>,
    failures: usize,
    timeouts: usize,
    issue_order: Vec<String>,
    coverage: Vec<(f64, usize)>,
    total_matchups: usize,
}

#[derive(Debug)]
struct AttemptBar {
    pair: String,
    repetition_index: u32,
    attempt_index: u32,
    started_seconds: f64,
    finished_seconds: f64,
    launch_seconds: Option<f64>,
    setup_seconds: Option<f64>,
    play_seconds: Option<f64>,
    finish_seconds: Option<f64>,
    lane: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct PhaseMeans {
    total_seconds: Option<f64>,
    launch_seconds: Option<f64>,
    setup_seconds: Option<f64>,
    play_seconds: Option<f64>,
    finish_seconds: Option<f64>,
}

#[derive(Debug, Default)]
struct PhaseDiagnostics {
    first_quarter: PhaseMeans,
    last_quarter: PhaseMeans,
    same_pair_late_minus_early: PhaseMeans,
    pair_mean_min_seconds: Option<f64>,
    pair_mean_max_seconds: Option<f64>,
    first_completion_wave_mean_seconds: Option<f64>,
    all_attempt_mean_seconds: Option<f64>,
}

#[derive(Debug)]
struct AttemptBuild {
    match_id: MatchId,
    slot: PlannedSlot,
    attempt_index: u32,
    players: [String; 2],
    queued_at: Option<u64>,
    acquired_at: Option<u64>,
    launch_started_at: Option<u64>,
    handshake_completed_at: Option<u64>,
    playing_started_at: Option<u64>,
    terminal_produced_at: Option<u64>,
    persisted_at: Option<u64>,
    released_at: Option<u64>,
    detail: Option<AttemptDetail>,
}

pub(crate) fn write_report(
    manifest_path: &Path,
    output_path: &Path,
    simulation_seed: u64,
) -> Result<()> {
    let raw = fs::read(manifest_path)
        .with_context(|| format!("read ETA corpus manifest {}", manifest_path.display()))?;
    let manifest: CorpusManifest = serde_json::from_slice(&raw)
        .with_context(|| format!("decode ETA corpus manifest {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;

    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut analyses = Vec::with_capacity(manifest.cases.len());
    for entry in manifest.cases.iter().cloned() {
        let trace_path = root.join(&entry.trace);
        let trace = read_timing_trace(&trace_path)?;
        let ratings = entry
            .results
            .as_ref()
            .map(|path| read_results(&root.join(path)))
            .transpose()?
            .unwrap_or_default();
        let mut analysis = analyze_case(entry, &trace, simulation_seed)
            .with_context(|| format!("analyze ETA timing trace {}", trace_path.display()))?;
        analysis.ratings = ratings;
        analyses.push(analysis);
    }

    let html = render_html(&manifest.title, &analyses, simulation_seed);
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create report directory {}", parent.display()))?;
    }
    fs::write(output_path, html)
        .with_context(|| format!("write ETA report {}", output_path.display()))?;
    Ok(())
}

fn read_timing_trace(path: &Path) -> Result<TournamentTimingTrace> {
    let file =
        File::open(path).with_context(|| format!("read ETA timing trace {}", path.display()))?;
    let reader = BufReader::new(file);
    let reader: Box<dyn Read> = if path.extension().is_some_and(|extension| extension == "gz") {
        Box::new(GzDecoder::new(reader))
    } else {
        Box::new(reader)
    };
    serde_json::from_reader(reader)
        .with_context(|| format!("decode ETA timing trace {}", path.display()))
}

fn read_results(path: &Path) -> Result<BTreeMap<String, RatingStanding>> {
    let encoded =
        fs::read(path).with_context(|| format!("read tournament results {}", path.display()))?;
    let results: ResultsSummary = serde_json::from_slice(&encoded)
        .with_context(|| format!("decode tournament results {}", path.display()))?;
    let mut standings = BTreeMap::new();
    for standing in results.standings {
        let player_id = standing.player_id.clone();
        if standings.insert(player_id.clone(), standing).is_some() {
            bail!(
                "tournament results {} contain duplicate player {:?}",
                path.display(),
                player_id
            );
        }
    }
    Ok(standings)
}

fn validate_manifest(manifest: &CorpusManifest) -> Result<()> {
    if manifest.schema_version == 0 || manifest.schema_version > CORPUS_SCHEMA_VERSION {
        bail!(
            "unsupported corpus schema {}; latest supported is {}",
            manifest.schema_version,
            CORPUS_SCHEMA_VERSION
        );
    }
    if manifest.cases.is_empty() {
        bail!("ETA corpus contains no cases");
    }
    let mut ids = HashSet::with_capacity(manifest.cases.len());
    for case in &manifest.cases {
        if !valid_id(&case.id) {
            bail!(
                "case id {:?} must contain only lowercase ASCII letters, digits, and hyphens",
                case.id
            );
        }
        if !ids.insert(case.id.as_str()) {
            bail!("duplicate ETA corpus case id {:?}", case.id);
        }
        if case.trace.is_absolute() {
            bail!("case {:?} trace path must be manifest-relative", case.id);
        }
        if case.results.as_ref().is_some_and(|path| path.is_absolute()) {
            bail!("case {:?} results path must be manifest-relative", case.id);
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn analyze_case(
    entry: CaseEntry,
    trace: &TournamentTimingTrace,
    simulation_seed: u64,
) -> Result<CaseAnalysis> {
    let comparison = compare_eta_models_walk_forward(trace, simulation_seed)?;
    let first_at = comparison
        .first()
        .context("complete trace contains no observations")?
        .at
        .micros;
    let completion_at = comparison
        .iter()
        .find(|point| point.m0.estimate.state == EtaState::Complete)
        .context("comparison did not contain a complete point")?
        .at
        .micros;
    let duration_seconds = completion_at.saturating_sub(first_at) as f64 / 1_000_000.0;

    // One last observation per elapsed wall-clock second prevents high-volume
    // turn traces from receiving more statistical weight merely because they
    // emitted more phase events. Completion is retained explicitly.
    let mut per_second = BTreeMap::new();
    for point in &comparison {
        if point.at.micros <= completion_at {
            let bucket = point.at.micros.saturating_sub(first_at) / 1_000_000;
            per_second.insert(bucket, point);
        }
    }
    let completion = comparison
        .iter()
        .find(|point| point.at.micros == completion_at)
        .context("completion observation disappeared")?;
    per_second.insert(
        completion_at.saturating_sub(first_at) / 1_000_000,
        completion,
    );

    let timeline: Vec<_> = per_second
        .values()
        .map(|point| timeline_point(point, first_at))
        .collect();
    let m0 = model_metrics(&timeline, duration_seconds, Model::M0);
    let m1 = model_metrics(&timeline, duration_seconds, Model::M1);
    let m1p = model_metrics(&timeline, duration_seconds, Model::M1P);
    let common_support = common_support_metrics(&timeline);
    let (schedule, attempts, outcomes) =
        analyze_schedule(trace, first_at, completion_at, duration_seconds);
    let phase_diagnostics = phase_diagnostics(&attempts, trace.max_parallel);

    Ok(CaseAnalysis {
        entry,
        slots: trace.slots.len(),
        max_parallel: trace.max_parallel,
        event_count: trace.events.len(),
        duration_seconds,
        timeline,
        m0,
        m1,
        m1p,
        common_support,
        schedule,
        attempts,
        phase_diagnostics,
        outcomes,
        ratings: BTreeMap::new(),
    })
}

fn analyze_schedule(
    trace: &TournamentTimingTrace,
    first_at: u64,
    completion_at: u64,
    duration_seconds: f64,
) -> (
    ScheduleMetrics,
    Vec<AttemptBar>,
    HashMap<PlannedSlot, AttemptDetail>,
) {
    let normalized = trace.clone().normalized();
    let mut builds: HashMap<MatchId, AttemptBuild> = HashMap::new();
    let mut issue_order = Vec::new();
    let mut outcomes = HashMap::new();

    for event in normalized
        .events
        .iter()
        .filter(|event| event.observed_at.micros <= completion_at)
    {
        let build = builds
            .entry(event.descriptor.match_id)
            .or_insert_with(|| AttemptBuild {
                match_id: event.descriptor.match_id,
                slot: event.descriptor.slot_key(),
                attempt_index: event.descriptor.attempt_index,
                players: [
                    event.descriptor.player1_id.clone(),
                    event.descriptor.player2_id.clone(),
                ],
                queued_at: None,
                acquired_at: None,
                launch_started_at: None,
                handshake_completed_at: None,
                playing_started_at: None,
                terminal_produced_at: None,
                persisted_at: None,
                released_at: None,
                detail: None,
            });
        match &event.phase {
            ExecutionPhase::Queued { players } => {
                build.players = players.clone();
                build.queued_at = Some(event.observed_at.micros);
                issue_order.push(format!(
                    "{} · r{}a{}",
                    pair_label(&build.slot),
                    build.slot.repetition_index,
                    build.attempt_index
                ));
            },
            ExecutionPhase::PermitAcquired => {
                build.acquired_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::LaunchStarted => {
                build.launch_started_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::HandshakeCompleted { .. } => {
                build.handshake_completed_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::PlayingStarted => {
                build.playing_started_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::TerminalProduced { .. } => {
                build.terminal_produced_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::TerminalDetail { detail } => {
                build.detail = Some(*detail);
                outcomes.insert(build.slot.clone(), *detail);
            },
            ExecutionPhase::TerminalPersisted { .. } => {
                build.persisted_at = Some(event.observed_at.micros);
            },
            ExecutionPhase::PermitReleased => {
                build.released_at = Some(event.observed_at.micros);
            },
            _ => {},
        }
    }

    let mut complete_builds: Vec<_> = builds
        .into_values()
        .filter(|build| {
            build.acquired_at.is_some()
                && (build.released_at.is_some() || build.persisted_at.is_some())
        })
        .collect();
    complete_builds.sort_by_key(|build| {
        (
            build.acquired_at.unwrap_or(u64::MAX),
            build.queued_at.unwrap_or(u64::MAX),
            build.match_id,
        )
    });

    let mut lane_ends = vec![first_at; trace.max_parallel.max(1)];
    let mut attempts = Vec::with_capacity(complete_builds.len());
    let mut intervals_by_bot: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
    let mut service_micros = 0u128;
    let mut failures = 0usize;
    let mut timeouts = 0usize;
    for build in &complete_builds {
        let started = build.acquired_at.unwrap_or(first_at);
        // Durable completion can precede permit release by a small cleanup
        // interval. Cap every bar at the ETA completion boundary so the last
        // completed attempts remain in utilization/overlap without extending
        // the report beyond the user-visible tournament horizon.
        let finished = build
            .released_at
            .unwrap_or(completion_at)
            .min(completion_at)
            .max(started);
        service_micros = service_micros.saturating_add(u128::from(finished - started));
        for player in &build.players {
            intervals_by_bot
                .entry(player.clone())
                .or_default()
                .push((started, finished));
        }
        if let Some(AttemptDetail::Failed { reason }) = build.detail {
            failures += 1;
            if matches!(reason, TimingFailureReason::Timeout { .. }) {
                timeouts += 1;
            }
        }
        let lane = lane_ends
            .iter()
            .position(|&available_at| available_at <= started)
            .unwrap_or_else(|| {
                lane_ends.push(first_at);
                lane_ends.len() - 1
            });
        lane_ends[lane] = finished;
        attempts.push(AttemptBar {
            pair: pair_label(&build.slot),
            repetition_index: build.slot.repetition_index,
            attempt_index: build.attempt_index,
            started_seconds: started.saturating_sub(first_at) as f64 / 1_000_000.0,
            finished_seconds: finished.saturating_sub(first_at) as f64 / 1_000_000.0,
            launch_seconds: duration_seconds_between(
                build.launch_started_at,
                build.handshake_completed_at,
            ),
            setup_seconds: duration_seconds_between(
                build.handshake_completed_at,
                build.playing_started_at,
            ),
            play_seconds: duration_seconds_between(
                build.playing_started_at,
                build.terminal_produced_at,
            ),
            finish_seconds: duration_seconds_between(build.terminal_produced_at, Some(finished)),
            lane,
        });
    }

    let utilization = (duration_seconds > 0.0 && trace.max_parallel > 0).then(|| {
        service_micros as f64 / (duration_seconds * 1_000_000.0 * trace.max_parallel as f64)
    });
    let bot_overlap_seconds = intervals_by_bot
        .into_iter()
        .map(|(player, intervals)| (player, overlap_micros(&intervals) as f64 / 1_000_000.0))
        .collect();

    let events_before_completion: Vec<_> = normalized
        .events
        .iter()
        .filter(|event| event.observed_at.micros <= completion_at)
        .collect();
    let last_full_index = events_before_completion
        .iter()
        .rposition(|event| event.active_concurrency >= trace.max_parallel);
    let tail_start = last_full_index
        .and_then(|index| {
            events_before_completion[index + 1..]
                .iter()
                .find(|event| event.active_concurrency < trace.max_parallel)
        })
        .map_or(completion_at, |event| event.observed_at.micros);
    let underfilled_tail_seconds =
        Some(completion_at.saturating_sub(tail_start) as f64 / 1_000_000.0);

    let total_matchups = trace
        .slots
        .iter()
        .map(pair_label)
        .collect::<BTreeSet<_>>()
        .len();
    let mut completions: Vec<_> = complete_builds
        .iter()
        .filter_map(|build| {
            build.persisted_at.or(build.released_at).map(|at| {
                (
                    at.min(completion_at),
                    build.match_id,
                    pair_label(&build.slot),
                )
            })
        })
        .collect();
    completions.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    let mut covered = BTreeSet::new();
    let mut coverage = Vec::new();
    for (completed_at, _, pair) in completions {
        if covered.insert(pair) {
            coverage.push((
                completed_at.saturating_sub(first_at) as f64 / 1_000_000.0,
                covered.len(),
            ));
        }
    }

    (
        ScheduleMetrics {
            policy: trace.dispatch_policy,
            utilization,
            underfilled_tail_seconds,
            bot_overlap_seconds,
            failures,
            timeouts,
            issue_order,
            coverage,
            total_matchups,
        },
        attempts,
        outcomes,
    )
}

fn overlap_micros(intervals: &[(u64, u64)]) -> u64 {
    let mut changes: BTreeMap<u64, i32> = BTreeMap::new();
    for &(start, end) in intervals {
        *changes.entry(start).or_default() += 1;
        *changes.entry(end).or_default() -= 1;
    }
    let mut active = 0i32;
    let mut previous = None;
    let mut overlap = 0u64;
    for (at, delta) in changes {
        if let Some(previous) = previous {
            if active > 1 {
                overlap = overlap.saturating_add(at.saturating_sub(previous));
            }
        }
        active += delta;
        previous = Some(at);
    }
    overlap
}

fn duration_seconds_between(start: Option<u64>, end: Option<u64>) -> Option<f64> {
    start
        .zip(end)
        .map(|(start, end)| end.saturating_sub(start) as f64 / 1_000_000.0)
}

fn phase_diagnostics(attempts: &[AttemptBar], max_parallel: usize) -> PhaseDiagnostics {
    if attempts.is_empty() {
        return PhaseDiagnostics::default();
    }
    let quarter = (attempts.len() / 4).max(1);
    let first: Vec<_> = attempts.iter().take(quarter).collect();
    let last: Vec<_> = attempts.iter().rev().take(quarter).collect();

    let mut by_pair: BTreeMap<&str, Vec<&AttemptBar>> = BTreeMap::new();
    for attempt in attempts {
        by_pair.entry(&attempt.pair).or_default().push(attempt);
    }
    let mut same_pair_deltas = Vec::new();
    for pair_attempts in by_pair.values() {
        let pair_quarter = (pair_attempts.len() / 4).max(1);
        let pair_first: Vec<_> = pair_attempts.iter().take(pair_quarter).copied().collect();
        let pair_last: Vec<_> = pair_attempts
            .iter()
            .rev()
            .take(pair_quarter)
            .copied()
            .collect();
        same_pair_deltas.push(phase_difference(
            phase_means(&pair_last),
            phase_means(&pair_first),
        ));
    }
    let pair_means: Vec<_> = by_pair
        .values()
        .filter_map(|pair_attempts| phase_means(pair_attempts).total_seconds)
        .collect();
    let mut completion_order: Vec<_> = attempts.iter().collect();
    completion_order.sort_by(|left, right| {
        left.finished_seconds
            .total_cmp(&right.finished_seconds)
            .then(left.started_seconds.total_cmp(&right.started_seconds))
    });
    let first_completion_wave: Vec<_> = completion_order
        .into_iter()
        .take(max_parallel.max(1))
        .collect();
    let all_attempts: Vec<_> = attempts.iter().collect();

    PhaseDiagnostics {
        first_quarter: phase_means(&first),
        last_quarter: phase_means(&last),
        same_pair_late_minus_early: median_phase_means(&same_pair_deltas),
        pair_mean_min_seconds: pair_means.iter().copied().reduce(f64::min),
        pair_mean_max_seconds: pair_means.iter().copied().reduce(f64::max),
        first_completion_wave_mean_seconds: phase_means(&first_completion_wave).total_seconds,
        all_attempt_mean_seconds: phase_means(&all_attempts).total_seconds,
    }
}

fn phase_means(attempts: &[&AttemptBar]) -> PhaseMeans {
    let total: Vec<_> = attempts
        .iter()
        .map(|attempt| attempt.finished_seconds - attempt.started_seconds)
        .collect();
    let launch: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.launch_seconds)
        .collect();
    let setup: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.setup_seconds)
        .collect();
    let play: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.play_seconds)
        .collect();
    let finish: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.finish_seconds)
        .collect();
    PhaseMeans {
        total_seconds: mean(&total),
        launch_seconds: mean(&launch),
        setup_seconds: mean(&setup),
        play_seconds: mean(&play),
        finish_seconds: mean(&finish),
    }
}

fn phase_difference(late: PhaseMeans, early: PhaseMeans) -> PhaseMeans {
    PhaseMeans {
        total_seconds: option_difference(late.total_seconds, early.total_seconds),
        launch_seconds: option_difference(late.launch_seconds, early.launch_seconds),
        setup_seconds: option_difference(late.setup_seconds, early.setup_seconds),
        play_seconds: option_difference(late.play_seconds, early.play_seconds),
        finish_seconds: option_difference(late.finish_seconds, early.finish_seconds),
    }
}

fn median_phase_means(values: &[PhaseMeans]) -> PhaseMeans {
    let metric = |read: fn(&PhaseMeans) -> Option<f64>| {
        median(&values.iter().filter_map(read).collect::<Vec<_>>())
    };
    PhaseMeans {
        total_seconds: metric(|value| value.total_seconds),
        launch_seconds: metric(|value| value.launch_seconds),
        setup_seconds: metric(|value| value.setup_seconds),
        play_seconds: metric(|value| value.play_seconds),
        finish_seconds: metric(|value| value.finish_seconds),
    }
}

fn pair_label(slot: &PlannedSlot) -> String {
    format!("{} × {}", slot.player1_id, slot.player2_id)
}

fn timeline_point(point: &EtaModelComparisonPoint, first_at: u64) -> TimelinePoint {
    let m0_seconds = ready_seconds(&point.m0.estimate);
    let m1_seconds = ready_seconds(&point.m1.estimate);
    let m1p_seconds = ready_seconds(&point.m1p.estimate);
    TimelinePoint {
        elapsed_seconds: point.at.micros.saturating_sub(first_at) as f64 / 1_000_000.0,
        actual_seconds: point.actual_remaining_seconds as f64,
        m0_seconds,
        m1_seconds,
        m1p_seconds,
        m0_error_seconds: m0_seconds
            .zip(point.m0.signed_error_seconds)
            .map(|(_, value)| value as f64),
        m1_error_seconds: m1_seconds
            .zip(point.m1.signed_error_seconds)
            .map(|(_, value)| value as f64),
        m1p_error_seconds: m1p_seconds
            .zip(point.m1p.signed_error_seconds)
            .map(|(_, value)| value as f64),
    }
}

fn ready_seconds(estimate: &EtaEstimate) -> Option<f64> {
    match estimate.state {
        EtaState::Ready => estimate.seconds.map(|value| value as f64),
        EtaState::Learning | EtaState::Waiting | EtaState::Complete => None,
    }
}

#[derive(Clone, Copy)]
enum Model {
    M0,
    M1,
    M1P,
}

fn model_metrics(timeline: &[TimelinePoint], duration_seconds: f64, model: Model) -> ModelMetrics {
    let observations: Vec<_> = timeline
        .iter()
        .filter_map(|point| {
            let (prediction, error) = match model {
                Model::M0 => (point.m0_seconds, point.m0_error_seconds),
                Model::M1 => (point.m1_seconds, point.m1_error_seconds),
                Model::M1P => (point.m1p_seconds, point.m1p_error_seconds),
            };
            prediction.zip(error).map(|(_, error)| (point, error))
        })
        .collect();
    if observations.is_empty() {
        return ModelMetrics::default();
    }

    let errors: Vec<_> = observations.iter().map(|(_, error)| *error).collect();
    let stage_mae = |start: f64, end: f64| {
        let values: Vec<_> = observations
            .iter()
            .filter_map(|(point, error)| {
                let fraction = if duration_seconds > 0.0 {
                    point.elapsed_seconds / duration_seconds
                } else {
                    1.0
                };
                (fraction >= start && fraction < end).then_some(error.abs())
            })
            .collect();
        mean(&values)
    };
    let error_changes: Vec<_> = errors
        .windows(2)
        .map(|window| (window[1] - window[0]).abs())
        .collect();

    ModelMetrics {
        ready_points: observations.len(),
        first_ready_seconds: observations.first().map(|(point, _)| point.elapsed_seconds),
        mae_seconds: mean_abs(&errors),
        bias_seconds: mean(&errors),
        jitter_seconds: mean(&error_changes),
        early_mae_seconds: stage_mae(0.0, 1.0 / 3.0),
        middle_mae_seconds: stage_mae(1.0 / 3.0, 2.0 / 3.0),
        tail_mae_seconds: stage_mae(2.0 / 3.0, f64::INFINITY),
    }
}

fn common_support_metrics(timeline: &[TimelinePoint]) -> CommonSupportMetrics {
    let observations: Vec<_> = timeline
        .iter()
        .filter_map(|point| {
            Some((
                point,
                point.m0_error_seconds?,
                point.m1_error_seconds?,
                point.m1p_error_seconds?,
            ))
        })
        .collect();
    let m0_errors: Vec<_> = observations.iter().map(|(_, error, _, _)| *error).collect();
    let m1_errors: Vec<_> = observations.iter().map(|(_, _, error, _)| *error).collect();
    let m1p_errors: Vec<_> = observations.iter().map(|(_, _, _, error)| *error).collect();
    CommonSupportMetrics {
        points: observations.len(),
        first_seconds: observations
            .first()
            .map(|(point, _, _, _)| point.elapsed_seconds),
        m0_mae_seconds: mean_abs(&m0_errors),
        m1_mae_seconds: mean_abs(&m1_errors),
        m1p_mae_seconds: mean_abs(&m1p_errors),
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn mean_abs(values: &[f64]) -> Option<f64> {
    (!values.is_empty())
        .then(|| values.iter().map(|value| value.abs()).sum::<f64>() / values.len() as f64)
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    Some(if ordered.len().is_multiple_of(2) {
        (ordered[middle - 1] + ordered[middle]) / 2.0
    } else {
        ordered[middle]
    })
}

fn render_html(title: &str, cases: &[CaseAnalysis], simulation_seed: u64) -> String {
    let mut html = String::with_capacity(32_000 + cases.len() * 12_000);
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>
:root {{ color-scheme: light dark; --bg:#f7f7f4; --surface:#ffffff; --text:#181817; --muted:#65645f; --border:#d9d8d2; --grid:#e7e6e1; --actual:#262626; --m0:#2f6fbd; --m1:#c66a1c; --m1p:#7450a8; --gain:#237a4b; --loss:#b5473f; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#151514; --surface:#1d1d1b; --text:#efeee9; --muted:#aaa9a2; --border:#3a3935; --grid:#2f2f2c; --actual:#f0efe9; --m0:#75a9ea; --m1:#efa15f; --m1p:#c5a0ee; --gain:#6bc596; --loss:#ed8179; }} }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--text); font:15px/1.5 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }}
main {{ width:min(1180px,calc(100% - 32px)); margin:0 auto; padding:44px 0 64px; }}
h1,h2,h3,p {{ margin-top:0; }} h1 {{ margin-bottom:8px; font-size:clamp(26px,4vw,40px); line-height:1.12; letter-spacing:-.025em; }} h2 {{ margin-bottom:6px; font-size:22px; }} h3 {{ margin-bottom:4px; font-size:17px; }}
.lede {{ max-width:820px; color:var(--muted); font-size:16px; }}
.guard {{ display:inline-block; margin:10px 0 28px; padding:7px 10px; border:1px solid var(--border); border-radius:6px; font-weight:600; }}
.table-wrap {{ overflow-x:auto; border:1px solid var(--border); border-radius:8px; background:var(--surface); }}
table {{ width:100%; border-collapse:collapse; font-variant-numeric:tabular-nums; }} th,td {{ padding:10px 12px; text-align:right; border-bottom:1px solid var(--border); white-space:nowrap; }} th:first-child,td:first-child {{ text-align:left; }} thead th {{ color:var(--muted); font-size:12px; text-transform:uppercase; letter-spacing:.04em; }} tbody tr:last-child td {{ border-bottom:0; }}
.case {{ margin-top:34px; padding-top:30px; border-top:1px solid var(--border); }} .case-head {{ display:flex; justify-content:space-between; gap:18px; align-items:flex-start; }} .provenance {{ color:var(--muted); white-space:nowrap; }} .description {{ max-width:820px; color:var(--muted); }}
.facts {{ display:flex; flex-wrap:wrap; gap:8px 18px; margin:14px 0 18px; color:var(--muted); font-variant-numeric:tabular-nums; }} .facts strong {{ color:var(--text); }}
.legend {{ display:flex; flex-wrap:wrap; gap:14px; margin:0 0 7px; color:var(--muted); font-size:13px; }} .legend span::before {{ content:""; display:inline-block; width:18px; height:3px; margin:0 6px 3px 0; background:currentColor; }} .legend .actual {{ color:var(--actual); }} .legend .m0 {{ color:var(--m0); }} .legend .m1 {{ color:var(--m1); }} .legend .m1p {{ color:var(--m1p); }}
.chart {{ width:100%; height:auto; display:block; background:var(--surface); border:1px solid var(--border); border-radius:8px; margin-bottom:14px; }} .chart text {{ fill:var(--muted); font-size:12px; }} .chart .axis-title {{ fill:var(--text); font-weight:600; }} .chart .grid {{ stroke:var(--grid); stroke-width:1; }} .chart .axis {{ stroke:var(--border); stroke-width:1; }} .chart .actual-line {{ fill:none; stroke:var(--actual); stroke-width:2.2; stroke-dasharray:7 5; }} .chart .m0-line {{ fill:none; stroke:var(--m0); stroke-width:2.4; }} .chart .m1-line {{ fill:none; stroke:var(--m1); stroke-width:2.4; }} .chart .m1p-line {{ fill:none; stroke:var(--m1p); stroke-width:2.6; }} .chart .actual-point {{ fill:var(--actual); }} .chart .m0-point {{ fill:var(--m0); }} .chart .m1-point {{ fill:var(--m1); }} .chart .m1p-point {{ fill:var(--m1p); }} .chart .actual-point,.chart .m0-point,.chart .m1-point,.chart .m1p-point {{ stroke:var(--surface); stroke-width:1.2; }} .chart .zero {{ stroke:var(--actual); stroke-width:1.4; }}
.metrics {{ margin-top:12px; }} .delta-gain {{ color:var(--gain); }} .delta-loss {{ color:var(--loss); }}
.comparison {{ margin-top:28px; }} .comparison h3 {{ margin-top:22px; }} .issue-order {{ color:var(--muted); font-size:13px; overflow-wrap:anywhere; }}
.schedule {{ width:100%; height:auto; display:block; background:var(--surface); border:1px solid var(--border); border-radius:8px; margin:12px 0 14px; }} .schedule text {{ fill:var(--muted); font-size:11px; }} .schedule .grid {{ stroke:var(--grid); stroke-width:1; }} .schedule .attempt-label {{ fill:#fff; font-size:10px; font-weight:650; }}
.case-reading {{ margin:12px 0 0; font-weight:600; }}
.boundary {{ margin-top:38px; padding:22px; border:1px solid var(--border); border-radius:8px; background:var(--surface); }} .boundary p:last-child {{ margin-bottom:0; }} code {{ font-family:ui-monospace,SFMono-Regular,Consolas,monospace; }}
@media (max-width:680px) {{ main {{ width:min(100% - 20px,1180px); padding-top:28px; }} .case-head {{ display:block; }} .provenance {{ white-space:normal; }} th,td {{ padding:8px 9px; }} }}
</style>
</head>
<body>
<main>
<header>
<h1>{}</h1>
<p class="lede">Walk-forward point-estimate evidence from {} replayable timing traces. Every candidate sees only the prefix available at that instant; eventual completion is attached afterward for scoring. Replay seed: <code>{}</code>.</p>
<p class="guard">Evidence only — no candidate is selected.</p>
</header>
<section aria-labelledby="summary-title">
<h2 id="summary-title">Across the first corpus</h2>
<div class="table-wrap"><table>
<thead><tr><th>Case</th><th>Policy</th><th>Duration</th><th>Utilization</th><th>Underfilled tail</th><th>Bot overlap</th><th>M0 first</th><th>M1 first</th><th>M1P first</th><th>M0 MAE</th><th>M1 MAE</th><th>M1P MAE</th><th>M1P − M1</th><th>M0 tail</th><th>M1 tail</th><th>M1P tail</th></tr></thead><tbody>
"#,
        escape_html(title),
        escape_html(title),
        cases.len(),
        simulation_seed
    )
    .expect("write HTML header");

    for case in cases {
        let delta = option_difference(case.m1p.mae_seconds, case.m1.mae_seconds);
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&case.entry.label),
            policy_label(case.schedule.policy),
            format_seconds(Some(case.duration_seconds)),
            format_percent(case.schedule.utilization),
            format_seconds(case.schedule.underfilled_tail_seconds),
            format_seconds(Some(total_bot_overlap(&case.schedule))),
            format_seconds(case.m0.first_ready_seconds),
            format_seconds(case.m1.first_ready_seconds),
            format_seconds(case.m1p.first_ready_seconds),
            format_seconds(case.m0.mae_seconds),
            format_seconds(case.m1.mae_seconds),
            format_seconds(case.m1p.mae_seconds),
            delta_class(delta),
            format_signed_seconds(delta),
            format_seconds(case.m0.tail_mae_seconds),
            format_seconds(case.m1.tail_mae_seconds),
            format_seconds(case.m1p.tail_mae_seconds),
        )
        .expect("write summary row");
    }
    html.push_str("</tbody></table></div></section>\n");

    write_replicate_summary(&mut html, cases);
    write_model_diagnostics(&mut html, cases);
    write_policy_comparisons(&mut html, cases);

    for case in cases {
        write_case(&mut html, case);
    }

    let next_boundary = if cases
        .iter()
        .any(|case| case.entry.replicate_group.is_some())
    {
        "The corpus now includes three production-shaped eight-maze matched seed blocks plus one 16-maze-per-pair diagnostic block at GUI-default timing and concurrency. The remaining evidence boundary is the real partially pooled M2 with censored active evidence, calibrated ranges, deterministic or repeated outcome/rating stability, and final GUI-wrapper dogfood. One larger diagnostic block does not add an independent dispatch replication."
    } else {
        "The next evidence boundary is a wider held-out corpus across mixed speeds, schedule shapes, failures, and concurrency."
    };
    write!(
        html,
        r#"<section class="boundary" aria-labelledby="next-title">
<h2 id="next-title">What M1P changes</h2>
<p>M1P keeps M1’s scheduler fold and global duration for pending work. Only active service changes: each match uses the mean remaining time previously observed from its exact phase/turn boundary, falling back to <code>global mean − age</code> when that boundary has no completed evidence. It deliberately does not condition on time already spent inside an open phase; the partially pooled <code>setup + turns × turn-speed + finish</code> candidate retains the M2 name, and censored survival remains M3.</p>
<p>{next_boundary} Interval coverage and sharpness remain absent because M0, M1, and M1P still emit point estimates.</p>
</section>
</main>
</body>
</html>
"#,
    )
    .expect("write evidence boundary");
    html
}

#[derive(Debug)]
struct MatchedPolicyPair<'a> {
    family: String,
    concurrency: usize,
    pair_major: &'a CaseAnalysis,
    balanced: &'a CaseAnalysis,
}

fn matched_policy_pairs(cases: &[CaseAnalysis]) -> Vec<MatchedPolicyPair<'_>> {
    let mut groups: BTreeMap<(String, usize), Vec<&CaseAnalysis>> = BTreeMap::new();
    for case in cases {
        if let Some(family) = &case.entry.family {
            groups
                .entry((family.clone(), case.max_parallel))
                .or_default()
                .push(case);
        }
    }
    groups
        .into_iter()
        .filter_map(|((family, concurrency), cases)| {
            let pair_major = cases
                .iter()
                .copied()
                .find(|case| case.schedule.policy == DispatchPolicy::PairMajorV1)?;
            let balanced = cases
                .iter()
                .copied()
                .find(|case| case.schedule.policy == DispatchPolicy::BotBalancedCircleV1)?;
            Some(MatchedPolicyPair {
                family,
                concurrency,
                pair_major,
                balanced,
            })
        })
        .collect()
}

fn write_replicate_summary(html: &mut String, cases: &[CaseAnalysis]) {
    let matched = matched_policy_pairs(cases);
    let mut groups: BTreeMap<String, Vec<&MatchedPolicyPair<'_>>> = BTreeMap::new();
    for pair in &matched {
        let Some(pair_group) = pair.pair_major.entry.replicate_group.as_ref() else {
            continue;
        };
        if pair.balanced.entry.replicate_group.as_ref() == Some(pair_group) {
            groups.entry(pair_group.clone()).or_default().push(pair);
        }
    }
    if groups.is_empty() {
        return;
    }

    html.push_str(
        r#"<section class="comparison" aria-labelledby="replicate-title">
<h2 id="replicate-title">Replicated production-shaped evidence</h2>
<p class="lede">Each block is one seed-local pair of whole tournaments. Block count—not the games nested inside each tournament—is the dispatch replication count.</p>
<p class="lede">These live captures use the tournament CLI over the same planner, <code>EvalSession</code>, orchestrator, and host core as the GUI. They do not include GUI warmup, replay writing, Tauri event delivery, or frontend rendering.</p>
<p class="lede">Elo and rank columns describe observed run-to-run stability. Bot randomness is not seeded, so they are not causal estimates of dispatch effects.</p>
<div class="table-wrap"><table><thead><tr><th>Experiment</th><th>Blocks</th><th>Games / run</th><th>Balanced faster</th><th>Duration Δ mean [range]</th><th>Overlap Δ mean</th><th>M0 MAE pair → balanced</th><th>M1P MAE pair → balanced</th><th>Exact terminal detail</th><th>Winner stability</th><th>Max |Elo Δ|</th><th>Exact rank positions</th></tr></thead><tbody>"#,
    );

    let mut readings = Vec::new();
    for (group, pairs) in groups {
        let duration_deltas: Vec<_> = pairs
            .iter()
            .map(|pair| pair.balanced.duration_seconds - pair.pair_major.duration_seconds)
            .collect();
        let overlap_deltas: Vec<_> = pairs
            .iter()
            .map(|pair| {
                total_bot_overlap(&pair.balanced.schedule)
                    - total_bot_overlap(&pair.pair_major.schedule)
            })
            .collect();
        let pair_m0: Vec<_> = pairs
            .iter()
            .filter_map(|pair| pair.pair_major.m0.mae_seconds)
            .collect();
        let balanced_m0: Vec<_> = pairs
            .iter()
            .filter_map(|pair| pair.balanced.m0.mae_seconds)
            .collect();
        let pair_m1p: Vec<_> = pairs
            .iter()
            .filter_map(|pair| pair.pair_major.m1p.mae_seconds)
            .collect();
        let balanced_m1p: Vec<_> = pairs
            .iter()
            .filter_map(|pair| pair.balanced.m1p.mae_seconds)
            .collect();
        let mut outcomes = OutcomeComparison::default();
        let mut ratings = RatingComparison::default();
        for pair in &pairs {
            outcomes.add(&compare_outcomes(pair.pair_major, pair.balanced));
            ratings.add(&compare_ratings(pair.pair_major, pair.balanced));
        }
        let duration_mean = mean(&duration_deltas);
        let balanced_faster = duration_deltas.iter().filter(|&&delta| delta < 0.0).count();
        let pair_m0_mean = mean(&pair_m0);
        let balanced_m0_mean = mean(&balanced_m0);
        let pair_m1p_mean = mean(&pair_m1p);
        let balanced_m1p_mean = mean(&balanced_m1p);
        readings.push(format!(
            "<p class=\"case-reading\"><strong>{}:</strong> balanced finished faster in {}/{} blocks; mean duration Δ {}. Mean measured bot-overlap Δ was {}. M0 MAE moved {}, and M1P MAE moved {}. Winners matched {}/{} slots and exact terminal detail matched {}/{}; the largest Elo point shift was {}. With unseeded bot randomness, outcome and rating movement is a stability warning, not causal attribution.</p>",
            escape_html(&group),
            balanced_faster,
            pairs.len(),
            format_signed_seconds(duration_mean),
            format_signed_seconds(mean(&overlap_deltas)),
            format_metric_transition(pair_m0_mean, balanced_m0_mean),
            format_metric_transition(pair_m1p_mean, balanced_m1p_mean),
            outcomes.winner,
            outcomes.winner_compared,
            outcomes.exact,
            outcomes.compared,
            format_rating_shift(&ratings),
        ));
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td class=\"{}\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}/{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&group),
            pairs.len(),
            pairs[0].pair_major.slots,
            balanced_faster,
            pairs.len(),
            delta_class(duration_mean),
            format_mean_range(&duration_deltas),
            format_signed_seconds(mean(&overlap_deltas)),
            format_metric_transition(pair_m0_mean, balanced_m0_mean),
            format_metric_transition(pair_m1p_mean, balanced_m1p_mean),
            outcomes.exact,
            outcomes.compared,
            outcomes.winner,
            outcomes.winner_compared,
            format_rating_shift(&ratings),
            format_count_ratio(ratings.exact_rank_positions, ratings.compared),
        )
        .expect("write replicated comparison row");
    }
    html.push_str("</tbody></table></div>");
    for reading in readings {
        html.push_str(&reading);
    }
    html.push_str("</section>\n");
}

fn write_model_diagnostics(html: &mut String, cases: &[CaseAnalysis]) {
    let diagnostic_cases: Vec<_> = cases
        .iter()
        .filter(|case| case.entry.replicate_group.is_some())
        .collect();
    if diagnostic_cases.is_empty() {
        return;
    }

    html.push_str(
        r#"<section class="comparison" aria-labelledby="diagnostic-title">
<h2 id="diagnostic-title">Cold start, job mix, and common scoring support</h2>
<p class="lede">Global first-to-last-quarter movement mixes machine drift with whichever bot pairs the dispatcher places there. The same-pair median compares each pair’s own late games with its own early games; this is the stronger warmup check. Launch/handshake includes shell startup, <code>cargo run</code> or <code>uv run</code>, and the TCP connection. Setup covers configure and preprocessing; play covers the live turn loop.</p>
<p class="lede">The headline MAEs score each model over its own ready window. M1 and M1P become ready after one completed attempt, while M0 waits for the baseline sample threshold, so their extra early predictions are both useful and harder. “Common MAE” scores only wall-clock seconds where all three emitted a prediction.</p>
<p class="lede">M1 and M1P also learn service time only from completed attempts. During parallel execution, early completions preferentially expose shorter jobs while longer active jobs remain right-censored; M1 can then clamp an older active job to zero residual when its age exceeds that optimistic completed mean. M1P repairs only active residuals with completed phase/turn profiles. Pending jobs still inherit the same global completed-only mean.</p>
<div class="table-wrap"><table><thead><tr><th>Case</th><th>Policy</th><th>Games</th><th>First c completions / all</th><th>Pair mean range</th><th>Slow / fast</th><th>Total Q1 → Q4</th><th>Launch Q1 → Q4</th><th>Setup Q1 → Q4</th><th>Play Q1 → Q4</th><th>Same-pair total Δ</th><th>Same-pair launch Δ</th><th>Common from</th><th>Common points</th><th>Common MAE M0 / M1 / M1P</th></tr></thead><tbody>"#,
    );
    for case in diagnostic_cases {
        let diagnostics = &case.phase_diagnostics;
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&case.entry.label),
            policy_label(case.schedule.policy),
            case.slots,
            format_completion_bias(&case.phase_diagnostics),
            format_metric_transition(
                diagnostics.pair_mean_min_seconds,
                diagnostics.pair_mean_max_seconds,
            ),
            format_ratio(
                diagnostics.pair_mean_max_seconds,
                diagnostics.pair_mean_min_seconds,
            ),
            format_metric_transition(
                diagnostics.first_quarter.total_seconds,
                diagnostics.last_quarter.total_seconds,
            ),
            format_metric_transition(
                diagnostics.first_quarter.launch_seconds,
                diagnostics.last_quarter.launch_seconds,
            ),
            format_metric_transition(
                diagnostics.first_quarter.setup_seconds,
                diagnostics.last_quarter.setup_seconds,
            ),
            format_metric_transition(
                diagnostics.first_quarter.play_seconds,
                diagnostics.last_quarter.play_seconds,
            ),
            format_signed_seconds(diagnostics.same_pair_late_minus_early.total_seconds),
            format_signed_seconds(diagnostics.same_pair_late_minus_early.launch_seconds),
            format_seconds(case.common_support.first_seconds),
            case.common_support.points,
            format_common_mae(&case.common_support),
        )
        .expect("write model diagnostic row");
    }
    html.push_str("</tbody></table></div></section>\n");
}

fn write_policy_comparisons(html: &mut String, cases: &[CaseAnalysis]) {
    let matched = matched_policy_pairs(cases);
    if matched.is_empty() {
        return;
    }

    html.push_str(
        r#"<section class="comparison" aria-labelledby="policy-title">
<h2 id="policy-title">Matched dispatch comparisons</h2>
<p class="lede">Each row holds schedule content and tournament seed fixed. Runtime changes belong to dispatch; ETA errors remain model measurements within each realized run.</p>
<p class="lede">The whole matched run is the experimental unit. Games inside a row share one schedule, seed, machine, and execution window; they are not independent dispatch replications.</p>
<p class="lede">Exact terminal detail requires the same result, half-point scores, and turn count (or the same failure class). Winner stability compares only slots that finished in both runs.</p>
<div class="table-wrap"><table><thead><tr><th>Family</th><th>Workers</th><th>Pair-major</th><th>Balanced</th><th>Duration Δ</th><th>Utilization Δ</th><th>Tail Δ</th><th>Overlap Δ</th><th>Exact terminal detail</th><th>Winner stability</th><th>Max |Elo Δ|</th><th>Exact rank positions</th><th>Run order</th></tr></thead><tbody>"#,
    );
    for pair in matched {
        let MatchedPolicyPair {
            family,
            concurrency,
            pair_major,
            balanced,
        } = pair;
        let outcome = compare_outcomes(pair_major, balanced);
        let ratings = compare_ratings(pair_major, balanced);
        let duration_delta = balanced.duration_seconds - pair_major.duration_seconds;
        let utilization_delta = option_difference(
            balanced.schedule.utilization,
            pair_major.schedule.utilization,
        );
        let tail_delta = option_difference(
            balanced.schedule.underfilled_tail_seconds,
            pair_major.schedule.underfilled_tail_seconds,
        );
        let overlap_delta =
            total_bot_overlap(&balanced.schedule) - total_bot_overlap(&pair_major.schedule);
        let run_order = format!(
            "pair-major {} · balanced {}",
            pair_major
                .entry
                .run_order
                .map_or_else(|| "—".into(), |value| value.to_string()),
            balanced
                .entry
                .run_order
                .map_or_else(|| "—".into(), |value| value.to_string())
        );
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&family),
            concurrency,
            format_seconds(Some(pair_major.duration_seconds)),
            format_seconds(Some(balanced.duration_seconds)),
            delta_class(Some(duration_delta)),
            format_signed_seconds(Some(duration_delta)),
            format_signed_percent(utilization_delta),
            format_signed_seconds(tail_delta),
            format_signed_seconds(Some(overlap_delta)),
            outcome.exact,
            outcome.compared,
            outcome.winner,
            outcome.winner_compared,
            format_rating_shift(&ratings),
            format_count_ratio(ratings.exact_rank_positions, ratings.compared),
            escape_html(&run_order),
        )
        .expect("write policy comparison row");
    }
    html.push_str("</tbody></table></div></section>\n");
}

#[derive(Debug, Default)]
struct OutcomeComparison {
    compared: usize,
    exact: usize,
    winner: usize,
    winner_compared: usize,
}

impl OutcomeComparison {
    fn add(&mut self, other: &Self) {
        self.compared += other.compared;
        self.exact += other.exact;
        self.winner += other.winner;
        self.winner_compared += other.winner_compared;
    }
}

fn compare_outcomes(left: &CaseAnalysis, right: &CaseAnalysis) -> OutcomeComparison {
    let mut comparison = OutcomeComparison::default();
    for (slot, left_detail) in &left.outcomes {
        let Some(right_detail) = right.outcomes.get(slot) else {
            continue;
        };
        comparison.compared += 1;
        comparison.exact += usize::from(left_detail == right_detail);
        if let (Some(left_winner), Some(right_winner)) =
            (outcome_winner(left_detail), outcome_winner(right_detail))
        {
            comparison.winner_compared += 1;
            comparison.winner += usize::from(left_winner == right_winner);
        }
    }
    comparison
}

fn outcome_winner(detail: &AttemptDetail) -> Option<TimingGameResult> {
    match detail {
        AttemptDetail::Finished { result, .. } => Some(*result),
        AttemptDetail::Failed { .. } => None,
    }
}

#[derive(Debug, Default)]
struct RatingComparison {
    compared: usize,
    exact_rank_positions: usize,
    max_abs_delta: Option<f64>,
    combined_error_ratio_for_max: Option<f64>,
}

impl RatingComparison {
    fn add(&mut self, other: &Self) {
        self.compared += other.compared;
        self.exact_rank_positions += other.exact_rank_positions;
        if let Some(other_delta) = other.max_abs_delta {
            if self
                .max_abs_delta
                .is_none_or(|current| other_delta > current)
            {
                self.max_abs_delta = Some(other_delta);
                self.combined_error_ratio_for_max = other.combined_error_ratio_for_max;
            }
        }
    }
}

fn compare_ratings(left: &CaseAnalysis, right: &CaseAnalysis) -> RatingComparison {
    let ranks = |ratings: &BTreeMap<String, RatingStanding>| {
        let mut ordered: Vec<_> = ratings.values().collect();
        ordered.sort_by(|left, right| {
            right
                .elo
                .total_cmp(&left.elo)
                .then_with(|| left.player_id.cmp(&right.player_id))
        });
        ordered
            .into_iter()
            .enumerate()
            .map(|(index, standing)| (standing.player_id.clone(), index))
            .collect::<HashMap<_, _>>()
    };
    let left_ranks = ranks(&left.ratings);
    let right_ranks = ranks(&right.ratings);
    let mut comparison = RatingComparison::default();
    for (player, left_standing) in &left.ratings {
        let Some(right_standing) = right.ratings.get(player) else {
            continue;
        };
        comparison.compared += 1;
        comparison.exact_rank_positions +=
            usize::from(left_ranks.get(player) == right_ranks.get(player));
        let absolute_delta = (right_standing.elo - left_standing.elo).abs();
        if comparison
            .max_abs_delta
            .is_none_or(|current| absolute_delta > current)
        {
            let combined_error = left_standing.elo_stderr.hypot(right_standing.elo_stderr);
            comparison.max_abs_delta = Some(absolute_delta);
            comparison.combined_error_ratio_for_max =
                (combined_error > 0.0).then_some(absolute_delta / combined_error);
        }
    }
    comparison
}

fn total_bot_overlap(schedule: &ScheduleMetrics) -> f64 {
    schedule.bot_overlap_seconds.values().sum()
}

fn policy_label(policy: DispatchPolicy) -> &'static str {
    match policy {
        DispatchPolicy::PairMajorV1 => "pair-major v1",
        DispatchPolicy::BotBalancedCircleV1 => "bot-balanced circle v1",
    }
}

fn write_case(html: &mut String, case: &CaseAnalysis) {
    let m1p_vs_m1 = option_difference(case.m1p.mae_seconds, case.m1.mae_seconds);
    let m1p_vs_m0 = option_difference(case.m1p.mae_seconds, case.m0.mae_seconds);
    write!(
        html,
        r#"<section class="case" id="{}" aria-labelledby="{}-title">
<div class="case-head"><div><h2 id="{}-title">{}</h2><p class="description">{}</p></div><div class="provenance">{}</div></div>
<div class="facts"><span><strong>{}</strong> slots</span><span><strong>{}</strong> workers</span><span><strong>{}</strong> events</span><span><strong>{}</strong> elapsed</span><span><strong>{}</strong> policy</span><span><strong>{}</strong> utilization</span><span><strong>{}</strong> underfilled tail</span><span><strong>{}/{}</strong> matchups observed</span><span><strong>{}</strong> bot-overlap time</span><span><strong>{}</strong> failures / <strong>{}</strong> timeouts</span></div>
<h3>Realized schedule</h3>
<p class="issue-order"><strong>Issue order:</strong> {}</p>
{}
{}
<div class="legend" aria-label="Series legend"><span class="actual">Actual remaining</span><span class="m0">M0 terminal throughput</span><span class="m1">M1 scheduler residual</span><span class="m1p">M1P phase/turn progress probe</span></div>
{}
{}
<div class="table-wrap metrics"><table>
<thead><tr><th>Model</th><th>Ready samples</th><th>First useful</th><th>Early MAE</th><th>Middle MAE</th><th>Tail MAE</th><th>Bias</th><th>Error jitter</th></tr></thead>
<tbody>{}{}{}</tbody></table></div>
<p class="case-reading">{}</p>
</section>
"#,
        case.entry.id,
        case.entry.id,
        case.entry.id,
        escape_html(&case.entry.label),
        escape_html(&case.entry.description),
        escape_html(&case.entry.provenance),
        case.slots,
        case.max_parallel,
        case.event_count,
        format_seconds(Some(case.duration_seconds)),
        policy_label(case.schedule.policy),
        format_percent(case.schedule.utilization),
        format_seconds(case.schedule.underfilled_tail_seconds),
        case.schedule.coverage.last().map_or(0, |(_, count)| *count),
        case.schedule.total_matchups,
        format_seconds(Some(total_bot_overlap(&case.schedule))),
        case.schedule.failures,
        case.schedule.timeouts,
        issue_order_preview(&case.schedule.issue_order),
        schedule_svg(case),
        coverage_svg(case),
        timeline_svg(case),
        error_svg(case),
        metrics_row("M0", &case.m0),
        metrics_row("M1", &case.m1),
        metrics_row("M1P", &case.m1p),
        case_reading(m1p_vs_m1, m1p_vs_m0),
    )
    .expect("write case section");
}

fn issue_order_preview(order: &[String]) -> String {
    let shown = order
        .iter()
        .take(18)
        .cloned()
        .collect::<Vec<_>>()
        .join(" → ");
    let suffix = if order.len() > 18 {
        format!(" → … +{} attempts", order.len() - 18)
    } else {
        String::new()
    };
    escape_html(&format!("{shown}{suffix}"))
}

fn schedule_svg(case: &CaseAnalysis) -> String {
    let width = 960.0;
    let left = 72.0;
    let right = 20.0;
    let top = 28.0;
    let lane_height = 34.0;
    let lanes = case
        .attempts
        .iter()
        .map(|attempt| attempt.lane + 1)
        .max()
        .unwrap_or(case.max_parallel.max(1));
    let height = top + lanes as f64 * lane_height + 42.0;
    let plot_width = width - left - right;
    let duration = case.duration_seconds.max(1.0);
    let x = |seconds: f64| left + seconds / duration * plot_width;
    let mut svg = String::with_capacity(4_000 + case.attempts.len() * 180);
    write!(
        svg,
        "<svg class=\"schedule\" viewBox=\"0 0 {width:.0} {height:.0}\" role=\"img\" aria-labelledby=\"{}-schedule-title {}-schedule-desc\"><title id=\"{}-schedule-title\">{} realized worker schedule</title><desc id=\"{}-schedule-desc\">Permit occupancy by matchup and virtual worker lane in realized issue order.</desc>",
        case.entry.id,
        case.entry.id,
        case.entry.id,
        escape_html(&case.entry.label),
        case.entry.id,
    )
    .expect("write schedule SVG header");
    for index in 0..=4 {
        let seconds = duration * index as f64 / 4.0;
        let at = x(seconds);
        write!(
            svg,
            "<line class=\"grid\" x1=\"{at:.2}\" y1=\"{top:.2}\" x2=\"{at:.2}\" y2=\"{:.2}\"/><text x=\"{at:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{seconds:.0}s</text>",
            height - 34.0,
            height - 12.0,
        )
        .expect("write schedule grid");
    }
    for lane in 0..lanes {
        let y = top + lane as f64 * lane_height;
        write!(
            svg,
            "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\">worker {}</text>",
            left - 9.0,
            y + 20.0,
            lane + 1,
        )
        .expect("write worker label");
    }
    for attempt in &case.attempts {
        let start = x(attempt.started_seconds);
        let end = x(attempt.finished_seconds);
        let rect_width = (end - start).max(1.5);
        let y = top + attempt.lane as f64 * lane_height + 3.0;
        let label = compact_pair(&attempt.pair);
        let title = format!(
            "{} · repetition {} · attempt {} · {:.2}s–{:.2}s",
            attempt.pair,
            attempt.repetition_index,
            attempt.attempt_index,
            attempt.started_seconds,
            attempt.finished_seconds
        );
        write!(
            svg,
            "<g><title>{}</title><rect x=\"{start:.2}\" y=\"{y:.2}\" width=\"{rect_width:.2}\" height=\"26\" rx=\"3\" fill=\"{}\"/>",
            escape_html(&title),
            matchup_color(&attempt.pair),
        )
        .expect("write attempt rectangle");
        if rect_width >= 48.0 {
            write!(
                svg,
                "<text class=\"attempt-label\" x=\"{:.2}\" y=\"{:.2}\">{}</text>",
                start + 5.0,
                y + 17.0,
                escape_html(&label),
            )
            .expect("write attempt label");
        }
        svg.push_str("</g>");
    }
    svg.push_str("</svg>");
    svg
}

fn coverage_svg(case: &CaseAnalysis) -> String {
    let frame = PlotFrame::new(
        case.duration_seconds.max(1.0),
        0.0,
        case.schedule.total_matchups.max(1) as f64,
        190.0,
    );
    let mut path = format!("M {:.2} {:.2} ", frame.x(0.0), frame.y(0.0));
    let mut previous = 0usize;
    let mut points = Vec::new();
    for &(elapsed, count) in &case.schedule.coverage {
        write!(
            path,
            "L {:.2} {:.2} L {:.2} {:.2} ",
            frame.x(elapsed),
            frame.y(previous as f64),
            frame.x(elapsed),
            frame.y(count as f64),
        )
        .expect("write coverage path");
        points.push((frame.x(elapsed), frame.y(count as f64)));
        previous = count;
    }
    write!(
        path,
        "L {:.2} {:.2}",
        frame.x(case.duration_seconds.max(1.0)),
        frame.y(previous as f64),
    )
    .expect("finish coverage path");
    chart_svg(
        &format!("{}-coverage", case.entry.id),
        &format!("{} matchup evidence coverage", case.entry.label),
        "Distinct matchups with at least one completed permit-occupancy observation over elapsed time.",
        &frame,
        &[("m0-line", path)],
        &[("m0-point", points)],
        "Observed matchups",
        false,
    )
}

fn compact_pair(pair: &str) -> String {
    pair.split(" × ")
        .map(|player| player.rsplit('/').next().unwrap_or(player))
        .collect::<Vec<_>>()
        .join("×")
}

fn matchup_color(pair: &str) -> &'static str {
    const COLORS: [&str; 8] = [
        "#2f6fbd", "#c66a1c", "#7450a8", "#237a4b", "#b5473f", "#247c87", "#8a6d1d", "#a04f7a",
    ];
    let hash = pair.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    COLORS[hash % COLORS.len()]
}

fn metrics_row(label: &str, metrics: &ModelMetrics) -> String {
    format!(
        "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
        label,
        metrics.ready_points,
        format_seconds(metrics.first_ready_seconds),
        format_seconds(metrics.early_mae_seconds),
        format_seconds(metrics.middle_mae_seconds),
        format_seconds(metrics.tail_mae_seconds),
        format_signed_seconds(metrics.bias_seconds),
        format_seconds(metrics.jitter_seconds),
    )
}

fn case_reading(m1p_vs_m1: Option<f64>, m1p_vs_m0: Option<f64>) -> String {
    let against_m1 = match m1p_vs_m1 {
        Some(value) if value < -0.05 => format!(
            "M1P’s mean absolute error is {:.1}s lower than M1’s",
            value.abs()
        ),
        Some(value) if value > 0.05 => {
            format!(
                "M1P’s mean absolute error is {:.1}s higher than M1’s",
                value
            )
        },
        Some(_) => "M1P and M1 have effectively equal mean absolute error".into(),
        None => return "This trace does not contain comparable ready M1/M1P estimates.".into(),
    };
    let against_m0 = match m1p_vs_m0 {
        Some(value) if value < -0.05 => {
            format!("; it is also {:.1}s lower than M0.", value.abs())
        },
        Some(value) if value > 0.05 => format!("; it is {:.1}s higher than M0.", value),
        Some(_) => "; it is effectively equal to M0.".into(),
        None => ".".into(),
    };
    format!("On this trace, {against_m1}{against_m0}")
}

fn timeline_svg(case: &CaseAnalysis) -> String {
    let max_y = case
        .timeline
        .iter()
        .flat_map(|point| {
            [
                Some(point.actual_seconds),
                point.m0_seconds,
                point.m1_seconds,
                point.m1p_seconds,
            ]
        })
        .flatten()
        .fold(1.0_f64, f64::max)
        * 1.08;
    let frame = PlotFrame::new(case.duration_seconds.max(1.0), 0.0, max_y.max(1.0), 320.0);
    let actual = line_path(&case.timeline, &frame, |point| Some(point.actual_seconds));
    let m0 = line_path(&case.timeline, &frame, |point| point.m0_seconds);
    let m1 = line_path(&case.timeline, &frame, |point| point.m1_seconds);
    let m1p = line_path(&case.timeline, &frame, |point| point.m1p_seconds);
    let actual_points = chart_points(&case.timeline, &frame, |point| Some(point.actual_seconds));
    let m0_points = chart_points(&case.timeline, &frame, |point| point.m0_seconds);
    let m1_points = chart_points(&case.timeline, &frame, |point| point.m1_seconds);
    let m1p_points = chart_points(&case.timeline, &frame, |point| point.m1p_seconds);
    let title = format!("{} remaining-time predictions", case.entry.label);
    let description =
        "Actual remaining seconds compared with M0, M1, and M1P at each walk-forward checkpoint.";
    chart_svg(
        &case.entry.id,
        &title,
        description,
        &frame,
        &[
            ("actual-line", actual),
            ("m0-line", m0),
            ("m1-line", m1),
            ("m1p-line", m1p),
        ],
        &[
            ("actual-point", actual_points),
            ("m0-point", m0_points),
            ("m1-point", m1_points),
            ("m1p-point", m1p_points),
        ],
        "Remaining time (seconds)",
        false,
    )
}

fn error_svg(case: &CaseAnalysis) -> String {
    let extent = case
        .timeline
        .iter()
        .flat_map(|point| {
            [
                point.m0_error_seconds,
                point.m1_error_seconds,
                point.m1p_error_seconds,
            ]
        })
        .flatten()
        .fold(0.0_f64, |largest, value| largest.max(value.abs()))
        .max(1.0)
        * 1.12;
    let frame = PlotFrame::new(case.duration_seconds.max(1.0), -extent, extent, 250.0);
    let m0 = line_path(&case.timeline, &frame, |point| point.m0_error_seconds);
    let m1 = line_path(&case.timeline, &frame, |point| point.m1_error_seconds);
    let m1p = line_path(&case.timeline, &frame, |point| point.m1p_error_seconds);
    let m0_points = chart_points(&case.timeline, &frame, |point| point.m0_error_seconds);
    let m1_points = chart_points(&case.timeline, &frame, |point| point.m1_error_seconds);
    let m1p_points = chart_points(&case.timeline, &frame, |point| point.m1p_error_seconds);
    let title = format!("{} signed prediction error", case.entry.label);
    let description =
        "Predicted minus actual remaining seconds; values below zero are underestimates.";
    chart_svg(
        &format!("{}-error", case.entry.id),
        &title,
        description,
        &frame,
        &[("m0-line", m0), ("m1-line", m1), ("m1p-line", m1p)],
        &[
            ("m0-point", m0_points),
            ("m1-point", m1_points),
            ("m1p-point", m1p_points),
        ],
        "Signed error (seconds)",
        true,
    )
}

struct PlotFrame {
    width: f64,
    height: f64,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl PlotFrame {
    fn new(max_x: f64, min_y: f64, max_y: f64, height: f64) -> Self {
        Self {
            width: 960.0,
            height,
            left: 72.0,
            right: 20.0,
            top: 22.0,
            bottom: 52.0,
            max_x,
            min_y,
            max_y,
        }
    }

    fn x(&self, value: f64) -> f64 {
        self.left + value / self.max_x * (self.width - self.left - self.right)
    }

    fn y(&self, value: f64) -> f64 {
        self.top
            + (self.max_y - value) / (self.max_y - self.min_y)
                * (self.height - self.top - self.bottom)
    }
}

fn line_path(
    timeline: &[TimelinePoint],
    frame: &PlotFrame,
    value: impl Fn(&TimelinePoint) -> Option<f64>,
) -> String {
    let mut path = String::new();
    let mut open = false;
    for point in timeline {
        if let Some(y) = value(point) {
            write!(
                path,
                "{} {:.2} {:.2} ",
                if open { 'L' } else { 'M' },
                frame.x(point.elapsed_seconds),
                frame.y(y)
            )
            .expect("write SVG path");
            open = true;
        } else {
            open = false;
        }
    }
    path
}

fn chart_points(
    timeline: &[TimelinePoint],
    frame: &PlotFrame,
    value: impl Fn(&TimelinePoint) -> Option<f64>,
) -> Vec<(f64, f64)> {
    timeline
        .iter()
        .filter_map(|point| value(point).map(|y| (frame.x(point.elapsed_seconds), frame.y(y))))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn chart_svg(
    id: &str,
    title: &str,
    description: &str,
    frame: &PlotFrame,
    paths: &[(&str, String)],
    point_sets: &[(&str, Vec<(f64, f64)>)],
    y_title: &str,
    zero_line: bool,
) -> String {
    let mut svg = String::with_capacity(4_000);
    write!(
        svg,
        "<svg class=\"chart\" viewBox=\"0 0 {:.0} {:.0}\" role=\"img\" aria-labelledby=\"{}-svg-title {}-svg-desc\"><title id=\"{}-svg-title\">{}</title><desc id=\"{}-svg-desc\">{}</desc>",
        frame.width,
        frame.height,
        id,
        id,
        id,
        escape_html(title),
        id,
        escape_html(description),
    )
    .expect("write SVG header");

    for index in 0..=4 {
        let fraction = index as f64 / 4.0;
        let y_value = frame.min_y + (frame.max_y - frame.min_y) * (1.0 - fraction);
        let y = frame.top + fraction * (frame.height - frame.top - frame.bottom);
        write!(
            svg,
            "<line class=\"grid\" x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\"/><text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\">{:.0}</text>",
            frame.left,
            frame.width - frame.right,
            frame.left - 9.0,
            y + 4.0,
            y_value,
        )
        .expect("write y axis");
    }
    for index in 0..=4 {
        let value = frame.max_x * index as f64 / 4.0;
        let x = frame.x(value);
        write!(
            svg,
            "<line class=\"grid\" x1=\"{x:.2}\" y1=\"{:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\"/><text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{value:.0}</text>",
            frame.top,
            frame.height - frame.bottom,
            frame.height - frame.bottom + 22.0,
        )
        .expect("write x axis");
    }
    if zero_line && frame.min_y < 0.0 && frame.max_y > 0.0 {
        let zero = frame.y(0.0);
        write!(
            svg,
            "<line class=\"zero\" x1=\"{:.2}\" y1=\"{zero:.2}\" x2=\"{:.2}\" y2=\"{zero:.2}\"/>",
            frame.left,
            frame.width - frame.right,
        )
        .expect("write zero line");
    }
    for (class, path) in paths {
        if !path.is_empty() {
            write!(svg, "<path class=\"{class}\" d=\"{path}\"/>").expect("write series path");
        }
    }
    for (class, points) in point_sets {
        for (x, y) in points {
            write!(
                svg,
                "<circle class=\"{class}\" cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"3.1\"/>"
            )
            .expect("write series point");
        }
    }
    write!(
        svg,
        "<text class=\"axis-title\" x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\">Elapsed time (seconds)</text><text class=\"axis-title\" transform=\"translate(18 {:.2}) rotate(-90)\" text-anchor=\"middle\">{}</text></svg>",
        (frame.left + frame.width - frame.right) / 2.0,
        frame.height - 10.0,
        (frame.top + frame.height - frame.bottom) / 2.0,
        escape_html(y_title),
    )
    .expect("write SVG labels");
    svg
}

fn option_difference(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    left.zip(right).map(|(left, right)| left - right)
}

fn delta_class(delta: Option<f64>) -> &'static str {
    match delta {
        Some(value) if value < -0.05 => "delta-gain",
        Some(value) if value > 0.05 => "delta-loss",
        _ => "",
    }
}

fn format_seconds(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.1}s"))
}

fn format_signed_seconds(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:+.1}s"))
}

fn format_mean_range(values: &[f64]) -> String {
    if values.is_empty() {
        return "—".into();
    }
    let average = mean(values).expect("non-empty values have a mean");
    let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    format!("{average:+.1}s [{minimum:+.1}s, {maximum:+.1}s]")
}

fn format_metric_transition(left: Option<f64>, right: Option<f64>) -> String {
    left.zip(right).map_or_else(
        || "—".into(),
        |(left, right)| format!("{left:.1}s → {right:.1}s"),
    )
}

fn format_common_mae(metrics: &CommonSupportMetrics) -> String {
    metrics
        .m0_mae_seconds
        .zip(metrics.m1_mae_seconds)
        .zip(metrics.m1p_mae_seconds)
        .map_or_else(
            || "—".into(),
            |((m0, m1), m1p)| format!("{m0:.1}s / {m1:.1}s / {m1p:.1}s"),
        )
}

fn format_completion_bias(diagnostics: &PhaseDiagnostics) -> String {
    diagnostics
        .first_completion_wave_mean_seconds
        .zip(diagnostics.all_attempt_mean_seconds)
        .map_or_else(
            || "—".into(),
            |(first, all)| {
                let percentage = if all <= f64::EPSILON {
                    "—".into()
                } else {
                    format!("{:.0}%", first / all * 100.0)
                };
                format!("{first:.1}s / {all:.1}s ({percentage})")
            },
        )
}

fn format_ratio(numerator: Option<f64>, denominator: Option<f64>) -> String {
    numerator.zip(denominator).map_or_else(
        || "—".into(),
        |(numerator, denominator)| {
            if denominator <= f64::EPSILON {
                "—".into()
            } else {
                format!("{:.1}×", numerator / denominator)
            }
        },
    )
}

fn format_rating_shift(comparison: &RatingComparison) -> String {
    comparison.max_abs_delta.map_or_else(
        || "—".into(),
        |delta| {
            comparison.combined_error_ratio_for_max.map_or_else(
                || format!("{delta:.1}"),
                |ratio| format!("{delta:.1} ({ratio:.1}× combined ±err)"),
            )
        },
    )
}

fn format_count_ratio(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        "—".into()
    } else {
        format!("{numerator}/{denominator}")
    }
}

fn format_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{:.1}%", value * 100.0))
}

fn format_signed_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{:+.1} pp", value * 100.0))
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    fn entry(id: &str, label: &str) -> CaseEntry {
        CaseEntry {
            id: id.into(),
            label: label.into(),
            provenance: "synthetic".into(),
            description: "Pinned edge case".into(),
            trace: PathBuf::from("unused.json"),
            family: None,
            run_order: None,
            replicate_group: None,
            results: None,
        }
    }

    #[test]
    fn report_renders_temporal_and_error_evidence_without_selecting_a_model() {
        let analyses: Vec<_> = synthetic::cases()
            .into_iter()
            .enumerate()
            .map(|(index, (name, trace))| {
                analyze_case(entry(&format!("case-{index}"), name), &trace, 0)
                    .expect("analyze synthetic case")
            })
            .collect();
        let html = render_html("ETA evidence", &analyses, 0);
        assert!(html.contains("Actual remaining"));
        assert!(html.contains("Signed error (seconds)"));
        assert!(html.contains("no candidate is selected"));
        assert!(html.contains("What M1P changes"));
        assert!(html.contains("M1P phase/turn progress probe"));
        assert!(!html.contains("NaN"));
        assert!(!html.contains("inf"));
    }

    #[test]
    fn replicated_summary_uses_whole_blocks_and_includes_rating_stability() {
        let cases = synthetic::cases();
        let pair_trace = &cases
            .iter()
            .find(|(name, _)| *name == synthetic::MIXED_PAIR_MAJOR_C2_FILE)
            .expect("pair-major fixture")
            .1;
        let balanced_trace = &cases
            .iter()
            .find(|(name, _)| *name == synthetic::MIXED_BALANCED_C2_FILE)
            .expect("balanced fixture")
            .1;
        let mut pair_entry = entry("pair", "Pair");
        pair_entry.family = Some("seed one".into());
        pair_entry.replicate_group = Some("replicated fixture".into());
        let mut balanced_entry = entry("balanced", "Balanced");
        balanced_entry.family = Some("seed one".into());
        balanced_entry.replicate_group = Some("replicated fixture".into());
        let mut pair = analyze_case(pair_entry, pair_trace, 0).expect("analyze pair-major");
        let mut balanced =
            analyze_case(balanced_entry, balanced_trace, 0).expect("analyze balanced");
        pair.ratings.insert(
            "a".into(),
            RatingStanding {
                player_id: "a".into(),
                elo: 1000.0,
                elo_stderr: 0.0,
            },
        );
        pair.ratings.insert(
            "b".into(),
            RatingStanding {
                player_id: "b".into(),
                elo: 900.0,
                elo_stderr: 50.0,
            },
        );
        balanced.ratings.insert(
            "a".into(),
            RatingStanding {
                player_id: "a".into(),
                elo: 1000.0,
                elo_stderr: 0.0,
            },
        );
        balanced.ratings.insert(
            "b".into(),
            RatingStanding {
                player_id: "b".into(),
                elo: 950.0,
                elo_stderr: 50.0,
            },
        );

        let html = render_html("replicated", &[pair, balanced], 0);
        assert!(html.contains("Replicated production-shaped evidence"));
        assert!(html.contains("replicated fixture"));
        assert!(html.contains("50.0 (0.7× combined ±err)"));
        assert!(html.contains("Block count—not the games"));
        assert!(html.contains("Cold start, job mix, and common scoring support"));
        assert!(html.contains("Common MAE M0 / M1 / M1P"));
    }

    #[test]
    fn common_support_does_not_reward_a_model_for_abstaining_earlier() {
        let point = |elapsed_seconds: f64,
                     m0_error_seconds: Option<f64>,
                     m1_error_seconds: Option<f64>,
                     m1p_error_seconds: Option<f64>| {
            TimelinePoint {
                elapsed_seconds,
                actual_seconds: 20.0 - elapsed_seconds,
                m0_seconds: m0_error_seconds.map(|error| 20.0 - elapsed_seconds + error),
                m1_seconds: m1_error_seconds.map(|error| 20.0 - elapsed_seconds + error),
                m1p_seconds: m1p_error_seconds.map(|error| 20.0 - elapsed_seconds + error),
                m0_error_seconds,
                m1_error_seconds,
                m1p_error_seconds,
            }
        };
        let timeline = vec![
            point(0.0, None, Some(10.0), Some(8.0)),
            point(1.0, Some(4.0), Some(6.0), Some(2.0)),
            point(2.0, Some(-2.0), Some(4.0), Some(0.0)),
        ];

        let metrics = common_support_metrics(&timeline);
        assert_eq!(metrics.points, 2);
        assert_eq!(metrics.first_seconds, Some(1.0));
        assert_eq!(metrics.m0_mae_seconds, Some(3.0));
        assert_eq!(metrics.m1_mae_seconds, Some(5.0));
        assert_eq!(metrics.m1p_mae_seconds, Some(1.0));
    }

    #[test]
    fn phase_diagnostics_separate_pair_mix_from_within_pair_warmup() {
        let attempt = |pair: &str, start: f64, duration: f64, launch: f64| AttemptBar {
            pair: pair.into(),
            repetition_index: start as u32,
            attempt_index: 0,
            started_seconds: start,
            finished_seconds: start + duration,
            launch_seconds: Some(launch),
            setup_seconds: Some(1.0),
            play_seconds: Some(duration - launch - 1.0),
            finish_seconds: Some(0.0),
            lane: 0,
        };
        let attempts = vec![
            attempt("fast", 0.0, 2.0, 0.2),
            attempt("fast", 2.0, 2.0, 0.2),
            attempt("fast", 4.0, 2.0, 0.2),
            attempt("fast", 6.0, 2.0, 0.2),
            attempt("slow", 8.0, 10.0, 0.5),
            attempt("slow", 18.0, 10.0, 0.5),
            attempt("slow", 28.0, 10.0, 0.5),
            attempt("slow", 38.0, 10.0, 0.5),
        ];

        let diagnostics = phase_diagnostics(&attempts, 2);
        assert_eq!(diagnostics.first_quarter.total_seconds, Some(2.0));
        assert_eq!(diagnostics.last_quarter.total_seconds, Some(10.0));
        assert_eq!(
            diagnostics.same_pair_late_minus_early.total_seconds,
            Some(0.0)
        );
        assert_eq!(
            diagnostics.same_pair_late_minus_early.launch_seconds,
            Some(0.0)
        );
    }

    #[test]
    fn starter_corpus_records_m1p_as_a_partial_straggler_repair_not_a_selection() {
        let (_, trace) = synthetic::cases()
            .into_iter()
            .find(|(name, _)| *name == synthetic::ACTIVE_STRAGGLER_FILE)
            .expect("active straggler fixture");
        let analysis = analyze_case(entry("active-straggler", "Active straggler"), &trace, 0)
            .expect("analyze active straggler");

        assert!(analysis.m1p.mae_seconds < analysis.m1.mae_seconds);
        assert!(analysis.m1p.mae_seconds > analysis.m0.mae_seconds);
    }

    #[test]
    fn schedule_metrics_include_attempts_that_release_after_durable_completion() {
        let (_, trace) = synthetic::cases()
            .into_iter()
            .find(|(name, _)| *name == synthetic::MIXED_PAIR_MAJOR_C1_FILE)
            .expect("single-worker mixed fixture");
        let analysis =
            analyze_case(entry("mixed-c1", "Mixed c1"), &trace, 0).expect("analyze mixed fixture");

        assert_eq!(analysis.attempts.len(), analysis.slots);
        assert_eq!(analysis.schedule.utilization, Some(1.0));
        assert_eq!(analysis.schedule.underfilled_tail_seconds, Some(0.0));
    }

    #[test]
    fn simultaneous_attempts_render_deterministically_and_coverage_is_chronological() {
        let (_, trace) = synthetic::cases()
            .into_iter()
            .find(|(name, _)| *name == synthetic::MIXED_PAIR_MAJOR_C2_FILE)
            .expect("two-worker mixed fixture");
        let first = analyze_case(entry("mixed-c2", "Mixed c2"), &trace, 0).expect("first analysis");
        let second =
            analyze_case(entry("mixed-c2", "Mixed c2"), &trace, 0).expect("second analysis");

        assert!(first
            .schedule
            .coverage
            .windows(2)
            .all(|window| window[0].0 <= window[1].0));
        assert_eq!(
            render_html("deterministic", &[first], 0),
            render_html("deterministic", &[second], 0)
        );
    }

    #[test]
    fn durable_retry_reports_the_failed_timeout_and_successful_retry() {
        let (_, trace) = synthetic::cases()
            .into_iter()
            .find(|(name, _)| *name == synthetic::DURABLE_RETRY_FILE)
            .expect("durable retry fixture");
        let analysis = analyze_case(entry("durable-retry", "Durable retry"), &trace, 0)
            .expect("analyze durable retry");

        assert_eq!(analysis.attempts.len(), 2);
        assert_eq!(analysis.schedule.failures, 1);
        assert_eq!(analysis.schedule.timeouts, 1);
        assert_eq!(analysis.outcomes.len(), 1, "retry replaces slot outcome");
    }

    #[test]
    fn manifest_rejects_duplicate_or_unsafe_case_ids() {
        let duplicate = CorpusManifest {
            schema_version: CORPUS_SCHEMA_VERSION,
            title: "x".into(),
            cases: vec![entry("same", "one"), entry("same", "two")],
        };
        assert!(validate_manifest(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));

        let unsafe_id = CorpusManifest {
            schema_version: CORPUS_SCHEMA_VERSION,
            title: "x".into(),
            cases: vec![entry("Not safe", "one")],
        };
        assert!(validate_manifest(&unsafe_id)
            .unwrap_err()
            .to_string()
            .contains("lowercase ASCII"));
    }

    #[test]
    fn report_reader_decodes_gzip_timing_traces() {
        let (_, trace) = synthetic::cases()
            .into_iter()
            .next()
            .expect("synthetic trace");
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("trace.json.gz");
        let file = File::create(&path).expect("create compressed trace");
        let mut encoder = GzEncoder::new(file, Compression::default());
        serde_json::to_writer(&mut encoder, &trace).expect("encode trace");
        encoder.finish().expect("finish compressed trace");

        assert_eq!(read_timing_trace(&path).expect("read trace"), trace);
    }
}
