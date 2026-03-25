pub mod elo;
mod schema;
mod store;
mod types;

pub use elo::{
    compute_elo, compute_elo_with_uncertainty, elo_from_winrate, win_expectancy, EloError,
    EloOptions, EloRating, EloResult, EloUncertainty, HeadToHead,
};
pub use store::{head_to_head_from_results, EvalStore};
pub use types::{
    EvalError, GameConfigRecord, GameResultRecord, NewGameResult, PlayerRecord, ResultFilter,
};
