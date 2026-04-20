//! Options trait and option definition types.

/// Owned option definition sent during Identify.
pub type SdkOptionDef = pyrat_protocol::OwnedOptionDef;

/// Trait for bot option declaration and application.
///
/// Bots without options: `impl Options for MyBot {}` (gets empty defaults).
/// Bots with options: `#[derive(Options)]` on the struct.
pub trait Options {
    /// Declare configurable options.
    fn option_defs(&self) -> Vec<SdkOptionDef> {
        vec![]
    }

    /// Apply a named option value. Called for each `SetOption` message.
    fn apply_option(&mut self, name: &str, _value: &str) -> Result<(), String> {
        Err(format!("unknown option: {name}"))
    }
}
