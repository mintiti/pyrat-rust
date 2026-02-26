use pyrat::GameConfig;
use serde::Serialize;
use specta::Type;

#[derive(Serialize, Type)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
}

#[derive(Serialize, Type)]
pub struct WallEntry {
    pub from: Coord,
    pub to: Coord,
}

#[derive(Serialize, Type)]
pub struct MudEntry {
    pub from: Coord,
    pub to: Coord,
    pub cost: u8,
}

#[derive(Serialize, Type)]
pub struct PlayerState {
    pub position: Coord,
    pub score: f32,
}

#[derive(Serialize, Type)]
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

#[tauri::command]
#[specta::specta]
pub fn get_game_state() -> Result<MazeState, String> {
    let config = GameConfig::classic(21, 15, 41);
    let game = config.create(Some(42)).map_err(|e| e.to_string())?;

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
        .mud
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
        .cheese
        .get_all_cheese_positions()
        .into_iter()
        .map(|c| Coord { x: c.x, y: c.y })
        .collect();

    Ok(MazeState {
        width: game.width,
        height: game.height,
        turn: game.turn,
        max_turns: game.max_turns,
        walls,
        mud,
        cheese,
        player1: PlayerState {
            position: Coord {
                x: game.player1.current_pos.x,
                y: game.player1.current_pos.y,
            },
            score: game.player1.score,
        },
        player2: PlayerState {
            position: Coord {
                x: game.player2.current_pos.x,
                y: game.player2.current_pos.y,
            },
            score: game.player2.score,
        },
        total_cheese: game.cheese.total_cheese(),
    })
}
