//! Protocol vocabulary for the PyRat host-bot communication.
//!
//! This crate defines the owned types that both the host and SDK use to
//! represent protocol messages. The FlatBuffers codec converts at the wire
//! boundary; from that point on, everything speaks these types.
//!
//! The [`HostMsg`] and [`BotMsg`] enums define the Player trait pipe vocabulary:
//! what the Match sends and receives through `Player::send()`/`Player::recv()`.

mod messages;
mod types;

pub use messages::*;
pub use types::*;

// Re-export wire types that are protocol concepts without engine equivalents.
pub use pyrat_wire::{GameResult, OptionType, Player, TimingMode};
