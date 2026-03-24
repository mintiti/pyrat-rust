pub mod elo;
mod schema;
mod store;
mod types;

pub use elo::{
    compute_elo, head_to_head_from_results, win_expectancy, EloError, EloOptions, EloRating,
    EloResult, HeadToHead,
};
pub use store::EvalStore;
pub use types::{
    EvalError, GameConfigRecord, GameResultRecord, NewGameResult, PlayerRecord, ResultFilter,
};
