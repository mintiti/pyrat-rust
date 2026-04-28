//! Shared bot-author-facing API, used by both the networked SDK
//! (`pyrat-sdk`) and the in-process host (`pyrat-host`).
//!
//! This crate holds types that both implementations must agree on so a
//! bot author switches between networked and embedded with the same
//! mental model. No `tokio` dependency: async channel types live on the
//! consumer side.

pub mod context;
pub mod info;
pub mod options;

pub use context::{BotContext, InfoSender, InfoSink};
pub use info::InfoParams;
pub use options::Options;
pub use pyrat_protocol::OptionDef;
