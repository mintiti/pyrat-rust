use serde::{Deserialize, Serialize};
use specta::Type;

use pyrat_host::game_loop;
use pyrat_host::wire::OptionType;

// ---------------------------------------------------------------------------
// Specta-friendly mirror types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub enum BotOptionType {
    Check,
    Spin,
    Combo,
    String,
    Button,
}

impl From<OptionType> for BotOptionType {
    fn from(ot: OptionType) -> Self {
        if ot == OptionType::Check {
            Self::Check
        } else if ot == OptionType::Spin {
            Self::Spin
        } else if ot == OptionType::Combo {
            Self::Combo
        } else if ot == OptionType::String {
            Self::String
        } else if ot == OptionType::Button {
            Self::Button
        } else {
            tracing::warn!(
                value = ot.0,
                "unknown OptionType variant, falling back to Check"
            );
            Self::Check
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotOptionDef {
    pub name: String,
    pub option_type: BotOptionType,
    pub default_value: String,
    pub min: i32,
    pub max: i32,
    pub choices: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotProbeResult {
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub options: Vec<BotOptionDef>,
}

/// A single option name-value pair for configuring a bot before match start.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotOptionValue {
    pub name: String,
    pub value: String,
}

/// Per-player option overrides + match flags, bundled so start_match stays under specta's 10-arg limit.
#[derive(Serialize, Deserialize, Debug, Clone, Default, Type)]
pub struct MatchBotOptions {
    #[serde(default)]
    pub player1: Vec<BotOptionValue>,
    #[serde(default)]
    pub player2: Vec<BotOptionValue>,
    /// When true, run in analysis (step-by-step) mode instead of auto-play.
    #[serde(default)]
    pub step_mode: bool,
}

// ---------------------------------------------------------------------------
// Tauri command
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn probe_bot(
    run_command: String,
    working_dir: String,
    agent_id: String,
) -> Result<BotProbeResult, String> {
    let result = game_loop::probe_bot(run_command, working_dir, agent_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(BotProbeResult {
        name: result.name,
        author: result.author,
        agent_id: result.agent_id,
        options: result
            .options
            .into_iter()
            .map(|o| BotOptionDef {
                name: o.name,
                option_type: o.option_type.into(),
                default_value: o.default_value,
                min: o.min,
                max: o.max,
                choices: o.choices,
            })
            .collect(),
    })
}
