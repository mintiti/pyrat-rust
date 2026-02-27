use std::sync::atomic::Ordering;

use pyrat::game::builder::GameConfig;
use pyrat::game::game_logic::GameState;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{MatchErrorEvent, MatchStartedEvent};
use crate::match_runner::run_match;
use crate::state::{AppState, MatchPhase};

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
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
            from: Coord {
                x: w.pos1.x,
                y: w.pos1.y,
            },
            to: Coord {
                x: w.pos2.x,
                y: w.pos2.y,
            },
        })
        .collect();

    let mud = game
        .mud_positions()
        .iter()
        .map(|((from, to), cost)| MudEntry {
            from: Coord {
                x: from.x,
                y: from.y,
            },
            to: Coord { x: to.x, y: to.y },
            cost,
        })
        .collect();

    let cheese = game
        .cheese_positions()
        .into_iter()
        .map(|c| Coord { x: c.x, y: c.y })
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
            position: Coord {
                x: game.player1_position().x,
                y: game.player1_position().y,
            },
            score: game.player1_score(),
        },
        player2: PlayerState {
            position: Coord {
                x: game.player2_position().x,
                y: game.player2_position().y,
            },
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
) -> Result<(), String> {
    use tauri_specta::Event;
    use tokio_util::sync::CancellationToken;

    // Cancel existing match (cooperative, not abort)
    {
        let phase = state.match_phase.lock().await;
        if let MatchPhase::Running { cancel, .. } = &*phase {
            cancel.cancel();
        }
    }
    // Lock dropped — old task will clean itself up

    let match_id = state.next_match_id.fetch_add(1, Ordering::Relaxed);
    let cancel = CancellationToken::new();

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

    // Set Running BEFORE spawn to avoid race where a fast-failing task
    // tries to reset phase before it's been set to Running.
    {
        let mut phase = state.match_phase.lock().await;
        *phase = MatchPhase::Running {
            match_id,
            cancel: cancel.clone(),
        };
    }

    let app_handle = app.clone();
    let match_phase = state.match_phase.clone();

    tokio::spawn(async move {
        let result = run_match(
            app_handle.clone(),
            game,
            player1_cmd,
            player2_cmd,
            cancel,
            match_id,
        )
        .await;

        if let Err(e) = &result {
            let _ = MatchErrorEvent {
                match_id,
                message: e.to_string(),
            }
            .emit(&app_handle);
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

    Ok(())
}
