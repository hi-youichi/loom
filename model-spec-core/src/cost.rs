use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    #[serde(default)]
    pub input: f64,

    #[serde(default)]
    pub output: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<f64>,
}

impl Cost {
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
            reasoning: None,
        }
    }

    pub fn input_cost_usd(&self) -> f64 {
        self.input
    }

    pub fn output_cost_usd(&self) -> f64 {
        self.output
    }

    pub fn estimate(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = self.input_cost_usd() * (input_tokens as f64 / 1_000_000.0);
        let output_cost = self.output_cost_usd() * (output_tokens as f64 / 1_000_000.0);
        input_cost + output_cost
    }
}
