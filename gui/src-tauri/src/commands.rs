use pyrat::GameConfig;
use serde::Serialize;
use specta::Type;

#[derive(Serialize, Type)]
pub struct GameInfo {
    pub width: u8,
    pub height: u8,
    pub total_cheese: u16,
    pub max_turns: u16,
    pub player1_position: [u8; 2],
    pub player2_position: [u8; 2],
}

#[tauri::command]
#[specta::specta]
pub fn get_game_info() -> Result<GameInfo, String> {
    let config = GameConfig::classic(21, 15, 41);
    let game = config.create(Some(42)).map_err(|e| e.to_string())?;

    Ok(GameInfo {
        width: game.width,
        height: game.height,
        total_cheese: game.cheese.total_cheese(),
        max_turns: game.max_turns,
        player1_position: [game.player1.current_pos.x, game.player1.current_pos.y],
        player2_position: [game.player2.current_pos.x, game.player2.current_pos.y],
    })
}
