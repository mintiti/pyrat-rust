//! Options trait and option definition types.
//!
//! These types live in `pyrat-bot-api` so networked and embedded bots
//! share one surface. Re-exported here for API continuity.

pub use pyrat_bot_api::Options;

/// Owned option definition sent during Identify.
pub type SdkOptionDef = pyrat_protocol::OwnedOptionDef;
