use std::sync::atomic::Ordering;

use pyrat::game::builder::{GameBuilder, GameConfig, MazeParams};
use pyrat::game::game_logic::GameState;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::{mpsc, oneshot};

use crate::events::{Direction as SpectaDirection, MatchErrorEvent, MatchStartedEvent};
use crate::match_runner::{run_match, specta_to_wire, wire_to_specta, PlayerSetup};
use crate::state::{AnalysisCmd, AnalysisResp, AnalysisTx, AppState, MatchPhase};

/// Pair of player actions for `advance_analysis`. Both must be provided together.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AnalysisActions {
    pub player1: SpectaDirection,
    pub player2: SpectaDirection,
}

// ---------------------------------------------------------------------------
// MatchConfigParams — flat DTO for the frontend
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct MatchConfigParams {
    /// Named preset, or "custom" for manual configuration.
    pub preset: String,
    pub width: u8,
    pub height: u8,
    pub max_turns: u16,
    pub wall_density: f64,
    pub mud_density: f64,
    pub mud_range: u8,
    pub connected: bool,
    pub symmetric: bool,
    pub cheese_count: u16,
    pub cheese_symmetric: bool,
    /// "corners" or "random".
    pub player_start: String,
    /// Seed for RNG. None = OS entropy.
    pub seed: Option<u64>,
}

impl Default for MatchConfigParams {
    fn default() -> Self {
        Self {
            preset: "medium".into(),
            width: 21,
            height: 15,
            max_turns: 300,
            wall_density: 0.7,
            mud_density: 0.1,
            mud_range: 3,
            connected: true,
            symmetric: true,
            cheese_count: 41,
            cheese_symmetric: true,
            player_start: "corners".into(),
            seed: None,
        }
    }
}

impl MatchConfigParams {
    /// Convert to engine GameConfig, validating all fields.
    pub fn to_game_config(&self) -> Result<GameConfig, String> {
        if self.preset != "custom" {
            return GameConfig::preset(&self.preset);
        }

        if self.width < 2 {
            return Err("Width must be at least 2".into());
        }
        if self.height < 2 {
            return Err("Height must be at least 2".into());
        }
        if self.max_turns == 0 {
            return Err("Max turns must be at least 1".into());
        }
        if self.cheese_count == 0 {
            return Err("Cheese count must be at least 1".into());
        }
        if !(0.0..=1.0).contains(&self.wall_density) {
            return Err("Wall density must be 0–1".into());
        }
        if !(0.0..=1.0).contains(&self.mud_density) {
            return Err("Mud density must be 0–1".into());
        }
        if self.mud_density > 0.0 && self.mud_range < 2 {
            return Err("Mud range must be ≥ 2 when mud density > 0".into());
        }

        let maze_params = MazeParams {
            wall_density: self.wall_density as f32,
            connected: self.connected,
            symmetric: self.symmetric,
            mud_density: self.mud_density as f32,
            mud_range: self.mud_range,
        };

        let builder = GameBuilder::new(self.width, self.height)
            .with_max_turns(self.max_turns)
            .with_random_maze(maze_params);

        let builder = match self.player_start.as_str() {
            "random" => builder.with_random_positions(),
            _ => builder.with_corner_positions(),
        };

        let config = builder
            .with_random_cheese(self.cheese_count, self.cheese_symmetric)
            .build();

        Ok(config)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
}

impl From<pyrat::game::types::Coordinates> for Coord {
    fn from(c: pyrat::game::types::Coordinates) -> Self {
        Self { x: c.x, y: c.y }
    }
}

impl From<(u8, u8)> for Coord {
    fn from((x, y): (u8, u8)) -> Self {
        Self { x, y }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct WallEntry {
    pub from: Coord,
    pub to: Coord,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct MudEntry {
    pub from: Coord,
    pub to: Coord,
    pub cost: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct PlayerState {
    pub position: Coord,
    pub score: f32,
    pub mud_turns: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct MazeState {
    pub width: u8,
    pub height: u8,
    pub turn: u16,
    pub max_turns: u16,
    pub walls: Vec<WallEntry>,
    pub mud: Vec<MudEntry>,
    pub cheese: Vec<Coord>,
    pub player1: PlayerState,
    pub player2: PlayerState,
    pub total_cheese: u16,
}

/// Convert engine GameState to our serializable MazeState.
pub fn build_maze_state(game: &GameState) -> MazeState {
    let walls = game
        .wall_entries()
        .into_iter()
        .map(|w| WallEntry {
            from: w.pos1.into(),
            to: w.pos2.into(),
        })
        .collect();

    let mud = game
        .mud_positions()
        .iter()
        .map(|((from, to), cost)| MudEntry {
            from: from.into(),
            to: to.into(),
            cost,
        })
        .collect();

    let cheese = game
        .cheese_positions()
        .into_iter()
        .map(Coord::from)
        .collect();

    MazeState {
        width: game.width(),
        height: game.height(),
        turn: game.turns(),
        max_turns: game.max_turns(),
        walls,
        mud,
        cheese,
        player1: PlayerState {
            position: game.player1_position().into(),
            score: game.player1_score(),
            mud_turns: game.player1_mud_turns(),
        },
        player2: PlayerState {
            position: game.player2_position().into(),
            score: game.player2_score(),
            mud_turns: game.player2_mud_turns(),
        },
        total_cheese: game.total_cheese(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_game_state(config: Option<MatchConfigParams>) -> Result<MazeState, String> {
    let params = config.unwrap_or_default();
    let game_config = params.to_game_config()?;
    let game = game_config.create(params.seed).map_err(|e| e.to_string())?;
    Ok(build_maze_state(&game))
}

/// Cancel any running match: signal cooperative cancellation, wait up to 5s,
/// then abort the task if it hasn't stopped.
async fn cancel_running_match(match_phase: &tokio::sync::Mutex<MatchPhase>) {
    use std::time::Duration;

    let old_handle = {
        let mut phase = match_phase.lock().await;
        match std::mem::replace(&mut *phase, MatchPhase::Idle) {
            MatchPhase::Running { cancel, handle, .. } => {
                cancel.cancel();
                Some(handle)
            },
            MatchPhase::Idle => None,
        }
    };
    if let Some(handle) = old_handle {
        let abort = handle.abort_handle();
        if tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn stop_match(state: tauri::State<'_, AppState>) -> Result<(), String> {
    cancel_running_match(&state.match_phase).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn start_match(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    player1_cmd: String,
    player2_cmd: String,
    player1_working_dir: Option<String>,
    player2_working_dir: Option<String>,
    config: Option<MatchConfigParams>,
    step_mode: Option<bool>,
) -> Result<(), String> {
    use tauri_specta::Event;
    use tokio_util::sync::CancellationToken;

    let params = config.unwrap_or_default();
    let step = step_mode.unwrap_or(false);

    // Cancel existing match and wait for cleanup before proceeding.
    cancel_running_match(&state.match_phase).await;

    let match_id = state.next_match_id.fetch_add(1, Ordering::Relaxed);
    let cancel = CancellationToken::new();
    let cancel_for_phase = cancel.clone();

    // Emit initial state before spawning so frontend can render immediately
    let game_config = params.to_game_config()?;
    let game = game_config.create(params.seed).map_err(|e| e.to_string())?;
    let initial_state = build_maze_state(&game);
    MatchStartedEvent {
        match_id,
        maze: initial_state,
    }
    .emit(&app)
    .map_err(|e| e.to_string())?;

    // Create analysis channel if step mode
    let (cmd_tx_for_phase, cmd_rx) = if step {
        let (tx, rx) = mpsc::channel(4);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let app_handle = app.clone();
    let match_phase = state.match_phase.clone();
    let cancel_check = cancel.clone();

    let handle = tokio::spawn(async move {
        let result = run_match(
            app_handle.clone(),
            game,
            [
                PlayerSetup {
                    command: player1_cmd,
                    working_dir: player1_working_dir,
                },
                PlayerSetup {
                    command: player2_cmd,
                    working_dir: player2_working_dir,
                },
            ],
            cancel,
            match_id,
            cmd_rx,
        )
        .await;

        if let Err(e) = &result {
            if !cancel_check.is_cancelled() {
                let _ = MatchErrorEvent {
                    match_id,
                    message: e.to_string(),
                }
                .emit(&app_handle);
            }
        }

        // Only reset to Idle if this is still the current match
        let mut phase = match_phase.lock().await;
        if let MatchPhase::Running {
            match_id: current, ..
        } = &*phase
        {
            if *current == match_id {
                *phase = MatchPhase::Idle;
            }
        }
    });

    {
        let mut phase = state.match_phase.lock().await;
        *phase = MatchPhase::Running {
            match_id,
            cancel: cancel_for_phase,
            handle,
            cmd_tx: cmd_tx_for_phase,
        };
    }

    Ok(())
}

// ── Analysis commands ───────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StopAnalysisTurnResult {
    pub player1_action: SpectaDirection,
    pub player2_action: SpectaDirection,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AdvanceAnalysisResult {
    pub player1_action: SpectaDirection,
    pub player2_action: SpectaDirection,
    pub game_over: bool,
}

/// Get a clone of the analysis channel sender, if the match is in step mode.
async fn get_cmd_tx(match_phase: &tokio::sync::Mutex<MatchPhase>) -> Result<AnalysisTx, String> {
    let phase = match_phase.lock().await;
    match &*phase {
        MatchPhase::Running {
            cmd_tx: Some(tx), ..
        } => Ok(tx.clone()),
        MatchPhase::Running { cmd_tx: None, .. } => Err("match is not in step mode".into()),
        MatchPhase::Idle => Err("no match running".into()),
    }
}

/// Send an analysis command and await the response.
async fn send_analysis_cmd(tx: &AnalysisTx, cmd: AnalysisCmd) -> Result<AnalysisResp, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send((cmd, reply_tx))
        .await
        .map_err(|_| "analysis loop exited".to_string())?;
    reply_rx
        .await
        .map_err(|_| "analysis loop dropped reply".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn start_analysis_turn(
    state: tauri::State<'_, AppState>,
    duration_ms: u64,
) -> Result<(), String> {
    let tx = get_cmd_tx(&state.match_phase).await?;
    let resp = send_analysis_cmd(&tx, AnalysisCmd::StartTurn { duration_ms }).await?;
    match resp {
        AnalysisResp::TurnStarted => Ok(()),
        AnalysisResp::Error(e) => Err(e),
        _ => Err("unexpected response".into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn stop_analysis_turn(
    state: tauri::State<'_, AppState>,
) -> Result<StopAnalysisTurnResult, String> {
    let tx = get_cmd_tx(&state.match_phase).await?;
    let resp = send_analysis_cmd(&tx, AnalysisCmd::StopTurn).await?;
    match resp {
        AnalysisResp::Actions { p1, p2 } => Ok(StopAnalysisTurnResult {
            player1_action: wire_to_specta(p1),
            player2_action: wire_to_specta(p2),
        }),
        AnalysisResp::Error(e) => Err(e),
        _ => Err("unexpected response".into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn advance_analysis(
    state: tauri::State<'_, AppState>,
    actions: Option<AnalysisActions>,
) -> Result<AdvanceAnalysisResult, String> {
    let tx = get_cmd_tx(&state.match_phase).await?;
    let actions = actions.map(|a| [specta_to_wire(a.player1), specta_to_wire(a.player2)]);
    let resp = send_analysis_cmd(&tx, AnalysisCmd::Advance { actions }).await?;
    match resp {
        AnalysisResp::Advanced { p1, p2, game_over } => Ok(AdvanceAnalysisResult {
            player1_action: wire_to_specta(p1),
            player2_action: wire_to_specta(p2),
            game_over,
        }),
        AnalysisResp::Error(e) => Err(e),
        _ => Err("unexpected response".into()),
    }
}
