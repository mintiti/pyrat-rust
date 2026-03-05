use std::sync::atomic::Ordering;

use pyrat::game::builder::{GameBuilder, GameConfig, MazeParams};
use pyrat::game::game_logic::GameState;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{MatchErrorEvent, MatchStartedEvent};
use crate::match_runner::{run_match, PlayerSetup};
use crate::state::{AppState, MatchPhase};

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
        },
        player2: PlayerState {
            position: game.player2_position().into(),
            score: game.player2_score(),
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
pub async fn start_match(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    player1_cmd: String,
    player2_cmd: String,
    player1_working_dir: Option<String>,
    player2_working_dir: Option<String>,
    config: Option<MatchConfigParams>,
) -> Result<(), String> {
    use tauri_specta::Event;
    use tokio_util::sync::CancellationToken;

    let params = config.unwrap_or_default();

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
        )
        .await;

        if let Err(e) = &result {
            // Don't emit error for expected cancellation — the new match's
            // MatchStartedEvent already reset the frontend.
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

    // Set Running AFTER spawn so we have the JoinHandle.
    // Brief race window: if another start_match arrives here, it sees Idle and
    // skips cancellation. Harmless — both tasks check match_id before cleanup.
    {
        let mut phase = state.match_phase.lock().await;
        *phase = MatchPhase::Running {
            match_id,
            cancel: cancel_for_phase,
            handle,
        };
    }

    Ok(())
}
