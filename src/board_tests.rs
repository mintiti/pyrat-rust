#[cfg(test)]
mod tests {
    use crate::board;

    #[test]
    fn test_new_board() {
        let board = board::Board::new(13, 15, 25);
        assert_eq!(board.maze.max_x, 13);
        assert_eq!(board.maze.max_y, 15);
        assert_eq!(board.maze.cheeses.len(), 195); // 13 * 15 = 195 total tiles
        assert_eq!(board.players.len(), 2);
    }

    #[test]
    fn test_to_node_index() {
        let board = board::Board::new(13, 15, 25);
        assert_eq!(board::to_node_index(0, 0, &board.maze), Some(0));
        assert_eq!(board::to_node_index(12, 14, &board.maze), Some(194));
        assert_eq!(board::to_node_index(13, 15, &board.maze), None);
    }

    // Add more test functions as needed
}