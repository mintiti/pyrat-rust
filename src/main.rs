mod board;
mod board_tests;

fn main() {
    let board: board::Board = board::Board::new(13, 15, 25);
    println!("{:?}", board);
}