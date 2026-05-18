pub mod elo;
mod schema;
mod store;
mod types;

pub use elo::{
    compute_elo, compute_elo_with_uncertainty, elo_from_winrate, win_expectancy, EloError,
    EloOptions, EloRating, EloResult, EloUncertainty, HeadToHead,
};
pub use store::{
    aggregate_pairs, head_to_head_from_attempt_records, head_to_head_from_results, EvalStore,
    TxStore,
};
pub use types::{
    AddTournamentPlayerError, AttemptKey, AttemptOutcome, AttemptRecord, AttemptStatus,
    CreateTournamentError, DeletePlayerError, EvalError, GameConfigRecord, GameResultRecord,
    NewAttempt, NewAttemptOutcome, NewGameResult, NewPlayer, NewTournament, PlayerRecord,
    RecordAttemptError, RegisterPlayerError, ResultFilter, TournamentId, TournamentParticipant,
    TournamentRecord,
};
