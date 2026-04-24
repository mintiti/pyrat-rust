//! Parameters for sending an Info message to the host.

use pyrat::{Coordinates, Direction};
use pyrat_wire::Player;

/// Parameters for sending an Info message.
///
/// Use [`InfoParams::for_player`] to create with defaults, then override
/// fields with struct update syntax:
///
/// ```ignore
/// ctx.send_info(&InfoParams {
///     depth: 5,
///     score: Some(3.0),
///     ..InfoParams::for_player(player)
/// });
/// ```
pub struct InfoParams<'a> {
    pub player: Player,
    pub multipv: u16,
    pub target: Option<Coordinates>,
    pub depth: u16,
    pub nodes: u32,
    pub score: Option<f32>,
    pub pv: &'a [Direction],
    pub message: &'a str,
}

impl InfoParams<'_> {
    pub fn for_player(player: Player) -> Self {
        Self {
            player,
            multipv: 0,
            target: None,
            depth: 0,
            nodes: 0,
            score: None,
            pv: &[],
            message: "",
        }
    }
}
