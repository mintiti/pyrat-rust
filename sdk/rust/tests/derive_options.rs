//! Compile + runtime tests for the Options derive macro.

use pyrat_sdk::{DeriveOptions, OptionType, Options};

#[derive(DeriveOptions)]
struct AllOptionTypes {
    #[spin(default = 3, min = 1, max = 10)]
    depth: i32,

    #[check(default = true)]
    pruning: bool,

    #[combo(default = "greedy", choices = ["greedy", "defensive", "balanced"])]
    strategy: String,

    #[str_opt(default = "hello")]
    label: String,

    // Not annotated — should be ignored
    _internal: Vec<u8>,
}

impl AllOptionTypes {
    fn new() -> Self {
        Self {
            depth: 3,
            pruning: true,
            strategy: "greedy".to_owned(),
            label: "hello".to_owned(),
            _internal: vec![],
        }
    }
}

#[test]
fn option_defs_has_correct_count() {
    let bot = AllOptionTypes::new();
    let defs = bot.option_defs();
    assert_eq!(defs.len(), 4);
}

#[test]
fn spin_option_def() {
    let bot = AllOptionTypes::new();
    let defs = bot.option_defs();
    let spin = defs.iter().find(|d| d.name == "depth").unwrap();
    assert_eq!(spin.option_type, OptionType::Spin);
    assert_eq!(spin.default_value, "3");
    assert_eq!(spin.min, 1);
    assert_eq!(spin.max, 10);
}

#[test]
fn check_option_def() {
    let bot = AllOptionTypes::new();
    let defs = bot.option_defs();
    let check = defs.iter().find(|d| d.name == "pruning").unwrap();
    assert_eq!(check.option_type, OptionType::Check);
    assert_eq!(check.default_value, "true");
}

#[test]
fn combo_option_def() {
    let bot = AllOptionTypes::new();
    let defs = bot.option_defs();
    let combo = defs.iter().find(|d| d.name == "strategy").unwrap();
    assert_eq!(combo.option_type, OptionType::Combo);
    assert_eq!(combo.default_value, "greedy");
    assert_eq!(combo.choices, vec!["greedy", "defensive", "balanced"]);
}

#[test]
fn str_option_def() {
    let bot = AllOptionTypes::new();
    let defs = bot.option_defs();
    let s = defs.iter().find(|d| d.name == "label").unwrap();
    assert_eq!(s.option_type, OptionType::String);
    assert_eq!(s.default_value, "hello");
}

#[test]
fn apply_spin() {
    let mut bot = AllOptionTypes::new();
    bot.apply_option("depth", "7").unwrap();
    assert_eq!(bot.depth, 7);
}

#[test]
fn apply_check() {
    let mut bot = AllOptionTypes::new();
    bot.apply_option("pruning", "false").unwrap();
    assert!(!bot.pruning);
    bot.apply_option("pruning", "1").unwrap();
    assert!(bot.pruning);
}

#[test]
fn apply_combo() {
    let mut bot = AllOptionTypes::new();
    bot.apply_option("strategy", "defensive").unwrap();
    assert_eq!(bot.strategy, "defensive");
}

#[test]
fn apply_str() {
    let mut bot = AllOptionTypes::new();
    bot.apply_option("label", "world").unwrap();
    assert_eq!(bot.label, "world");
}

#[test]
fn apply_unknown_option() {
    let mut bot = AllOptionTypes::new();
    let result = bot.apply_option("nonexistent", "value");
    assert!(result.is_err());
}

#[test]
fn apply_invalid_spin_value() {
    let mut bot = AllOptionTypes::new();
    let result = bot.apply_option("depth", "not_a_number");
    assert!(result.is_err());
}

// Test with no options
#[derive(DeriveOptions)]
struct NoOptions {
    _data: u32,
}

#[test]
fn no_options_empty_defs() {
    let bot = NoOptions { _data: 0 };
    assert!(bot.option_defs().is_empty());
}

#[test]
fn no_options_apply_fails() {
    let mut bot = NoOptions { _data: 0 };
    assert!(bot.apply_option("anything", "value").is_err());
}
