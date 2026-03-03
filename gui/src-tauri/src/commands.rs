use std::sync::atomic::Ordering;

use pyrat::game::builder::GameConfig;
use pyrat::game::game_logic::GameState;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{MatchErrorEvent, MatchStartedEvent};
use crate::match_runner::{run_match, PlayerSetup};
use crate::state::{AppState, MatchPhase};

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
pub fn get_game_state() -> Result<MazeState, String> {
    let config = GameConfig::classic(21, 15, 41);
    let game = config.create(Some(42)).map_err(|e| e.to_string())?;
    Ok(build_maze_state(&game))
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
) -> Result<(), String> {
    use std::time::Duration;
    use tauri_specta::Event;
    use tokio_util::sync::CancellationToken;

    // Cancel existing match and wait for cleanup before proceeding.
    let old_handle = {
        let mut phase = state.match_phase.lock().await;
        match std::mem::replace(&mut *phase, MatchPhase::Idle) {
            MatchPhase::Running { cancel, handle, .. } => {
                cancel.cancel();
                Some(handle)
            },
            MatchPhase::Idle => None,
        }
    };
    if let Some(handle) = old_handle {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

    let match_id = state.next_match_id.fetch_add(1, Ordering::Relaxed);
    let cancel = CancellationToken::new();
    let cancel_for_phase = cancel.clone();

    // Emit initial state before spawning so frontend can render immediately
    let config = GameConfig::classic(21, 15, 41);
    let game = config.create(None).map_err(|e| e.to_string())?;
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
