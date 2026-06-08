use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action { Buy, Sell, Hold }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Horizon {
    #[serde(rename = "1h")] H1,
    #[serde(rename = "4h")] H4,
    #[serde(rename = "1d")] D1,
    #[serde(rename = "3d")] D3,
    #[serde(rename = "1w")] W1,
}

impl Horizon {
    pub fn seconds(&self) -> i64 {
        match self {
            Horizon::H1 => 3600,
            Horizon::H4 => 4 * 3600,
            Horizon::D1 => 86_400,
            Horizon::D3 => 3 * 86_400,
            Horizon::W1 => 7 * 86_400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Citation {
    pub doc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisJson {
    pub action: Action,
    pub probability_up: f32,
    pub horizon: Horizon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_loss_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take_profit_pct: Option<f32>,
    #[serde(default)]
    pub key_signals: Vec<String>,
    #[serde(default)]
    pub citations: Vec<Citation>,
}

impl AnalysisJson {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.probability_up) {
            return Err(format!("probability_up out of range: {}", self.probability_up));
        }
        Ok(())
    }
}
