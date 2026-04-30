//! Build a wire-level [`MatchConfig`] from an engine [`GameState`] plus
//! host-only timing parameters.

use pyrat::game::game_logic::GameState;
use pyrat_protocol::{MatchConfig, MudEntry};
use pyrat_wire::TimingMode;

/// Build a `MatchConfig` from engine state + timing parameters.
///
/// `controlled_players` is left empty — the setup phase fills it per session.
pub fn build_match_config(
    game: &GameState,
    timing: TimingMode,
    move_timeout_ms: u32,
    preprocessing_timeout_ms: u32,
) -> MatchConfig {
    let walls = game
        .wall_entries()
        .into_iter()
        .map(|w| (w.pos1, w.pos2))
        .collect();

    let mud = game
        .mud_positions()
        .iter()
        .map(|((from, to), turns)| {
            let (pos1, pos2) = if from < to { (from, to) } else { (to, from) };
            MudEntry { pos1, pos2, turns }
        })
        .collect();

    let cheese = game.cheese_positions();

    MatchConfig {
        width: game.width(),
        height: game.height(),
        max_turns: game.max_turns(),
        walls,
        mud,
        cheese,
        player1_start: game.player1_position(),
        player2_start: game.player2_position(),
        controlled_players: vec![],
        timing,
        move_timeout_ms,
        preprocessing_timeout_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::game::builder::GameConfig;
    use pyrat::{Coordinates, GameBuilder};

    #[test]
    fn build_match_config_round_trips_game_state() {
        let game = GameBuilder::new(3, 3)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(Some(42))
            .unwrap();

        let cfg = build_match_config(&game, TimingMode::Wait, 500, 3000);

        assert_eq!(cfg.width, 3);
        assert_eq!(cfg.height, 3);
        assert_eq!(cfg.max_turns, game.max_turns());
        assert_eq!(cfg.player1_start, Coordinates::new(0, 0));
        assert_eq!(cfg.player2_start, Coordinates::new(2, 2));
        assert_eq!(cfg.cheese, vec![Coordinates::new(1, 1)]);
        assert!(cfg.walls.is_empty(), "open maze should have no walls");
        assert!(
            cfg.controlled_players.is_empty(),
            "controlled_players left for setup"
        );
        assert_eq!(cfg.timing, TimingMode::Wait);
        assert_eq!(cfg.move_timeout_ms, 500);
        assert_eq!(cfg.preprocessing_timeout_ms, 3000);
    }

    #[test]
    fn build_match_config_extracts_walls_and_mud() {
        let game = GameConfig::classic(7, 5, 3).create(Some(42)).unwrap();

        let cfg = build_match_config(&game, TimingMode::Wait, 500, 3000);

        assert_eq!(cfg.width, 7);
        assert_eq!(cfg.height, 5);
        assert!(!cfg.walls.is_empty(), "classic 7×5 maze should have walls");

        // Mud entries should be normalized: pos1 <= pos2.
        for m in &cfg.mud {
            assert!(
                m.pos1 <= m.pos2,
                "mud entry not normalized: {:?} > {:?}",
                m.pos1,
                m.pos2
            );
            assert!(m.turns >= 2, "mud value should be >= 2, got {}", m.turns);
        }
    }
}
