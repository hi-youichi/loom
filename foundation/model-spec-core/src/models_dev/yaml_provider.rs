use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::cost::Cost;
use super::limit::{Modalities, ModalityType, ModelLimit};
use super::model::{
    Experimental, ExperimentalMode, ExperimentalProviderConfig, Interleaved, InterleavedField,
    Model, ModelProviderConfig, ModelStatus, ProviderShape, ReasoningEffort, ReasoningOption,
};
use super::provider::Provider;

/// A single YAML plugin file defining a custom provider and its models.
#[derive(Debug, Deserialize)]
pub struct YamlPluginFile {
    pub provider: YamlProviderMeta,
    #[serde(default)]
    pub models: HashMap<String, YamlModelDef>,
}

#[derive(Debug, Deserialize)]
pub struct YamlProviderMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub env: Option<Vec<String>>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub doc: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlModelDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub knowledge: Option<String>,
    #[serde(default)]
    pub open_weights: Option<bool>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub structured_output: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub reasoning_options: Option<Vec<YamlReasoningOption>>,
    #[serde(default)]
    pub interleaved: Option<YamlInterleaved>,
    #[serde(default)]
    pub limit: Option<YamlModelLimit>,
    #[serde(default)]
    pub cost: Option<YamlCost>,
    #[serde(default)]
    pub modalities: Option<YamlModalities>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub provider: Option<YamlModelProviderConfig>,
    #[serde(default)]
    pub experimental: Option<YamlExperimental>,
}

#[derive(Debug, Deserialize)]
pub struct YamlModelLimit {
    pub context: u32,
    #[serde(default)]
    pub input: Option<u32>,
    pub output: u32,
}

#[derive(Debug, Deserialize)]
pub struct YamlCost {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub reasoning: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub input_audio: Option<f64>,
    #[serde(default)]
    pub output_audio: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct YamlModalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum YamlReasoningOption {
    Simple {
        r#type: String,
        values: Option<Vec<Option<String>>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum YamlInterleaved {
    Simple(bool),
    Field { field: String },
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlModelProviderConfig {
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlExperimental {
    #[serde(default)]
    pub modes: Option<HashMap<String, YamlExperimentalMode>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlExperimentalMode {
    #[serde(default)]
    pub cost: Option<YamlCost>,
    #[serde(default)]
    pub provider: Option<YamlModelProviderConfig>,
}

// ── Conversion: YAML types → domain types ──

impl YamlPluginFile {
    pub fn into_provider_and_models(self) -> (Provider, HashMap<String, Model>) {
        let provider_id = self.provider.id.clone();
        let provider_name = self.provider.name.clone();
        let provider_env = self.provider.env.clone().unwrap_or_default();
        let provider_doc = self.provider.doc.clone();
        let provider_api = self.provider.api.clone();

        let provider = Provider {
            id: provider_id,
            name: provider_name,
            env: provider_env,
            npm: None,
            doc: provider_doc,
            api: provider_api,
            models: HashMap::new(),
        };

        let models: HashMap<String, Model> = self
            .models
            .into_iter()
            .filter_map(|(id, def)| {
                let model = def.into_model(&id)?;
                Some((id, model))
            })
            .collect();

        (provider, models)
    }
}

impl YamlModelDef {
    pub(crate) fn into_model(self, model_id: &str) -> Option<Model> {
        let limit = match self.limit {
            Some(ref l) => ModelLimit {
                context: l.context,
                input: l.input,
                output: l.output,
            },
            None => return None,
        };

        let cost = self.cost.map(|c| Cost {
            input: c.input,
            output: c.output,
            reasoning: c.reasoning,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
            input_audio: c.input_audio,
            output_audio: c.output_audio,
            context_over_200k: None,
            tiers: None,
        });

        let modalities = self
            .modalities
            .map(|m| Modalities {
                input: m
                    .input
                    .iter()
                    .filter_map(|s| parse_modality_type(s))
                    .collect(),
                output: m
                    .output
                    .iter()
                    .filter_map(|s| parse_modality_type(s))
                    .collect(),
            })
            .unwrap_or_default();

        let reasoning_options = self.reasoning_options.map(|opts| {
            opts.iter()
                .filter_map(|o| match o {
                    YamlReasoningOption::Simple { r#type, values } => match r#type.as_str() {
                        "toggle" => Some(ReasoningOption::Toggle),
                        "effort" => {
                            let vals = values
                                .clone()
                                .unwrap_or_default()
                                .iter()
                                .map(|v| v.as_deref().and_then(parse_reasoning_effort))
                                .collect();
                            Some(ReasoningOption::Effort { values: vals })
                        }
                        "budget_tokens" => Some(ReasoningOption::BudgetTokens {
                            min: None,
                            max: None,
                        }),
                        _ => None,
                    },
                })
                .collect()
        });

        let interleaved = self.interleaved.and_then(|i| match i {
            YamlInterleaved::Simple(true) => Some(Interleaved::Simple),
            YamlInterleaved::Field { field } => {
                let f = match field.as_str() {
                    "reasoning_content" => InterleavedField::ReasoningContent,
                    "reasoning_details" => InterleavedField::ReasoningDetails,
                    _ => return None,
                };
                Some(Interleaved::Field { field: f })
            }
            YamlInterleaved::Simple(false) => None,
        });

        let status = self.status.as_deref().and_then(parse_model_status);

        let model_provider = self.provider.map(|p| ModelProviderConfig {
            npm: None,
            api: p.api,
            shape: p.shape.as_deref().and_then(parse_provider_shape),
            body: p.body,
            headers: p.headers,
        });

        let experimental = self.experimental.map(|e| Experimental {
            modes: e.modes.map(|modes| {
                modes
                    .into_iter()
                    .map(|(key, mode)| {
                        let cost = mode.cost.map(|c| Cost {
                            input: c.input,
                            output: c.output,
                            reasoning: c.reasoning,
                            cache_read: c.cache_read,
                            cache_write: c.cache_write,
                            input_audio: c.input_audio,
                            output_audio: c.output_audio,
                            context_over_200k: None,
                            tiers: None,
                        });
                        let provider = mode.provider.map(|p| ExperimentalProviderConfig {
                            body: p.body,
                            headers: p.headers,
                        });
                        (key, ExperimentalMode { cost, provider })
                    })
                    .collect()
            }),
        });

        Some(Model {
            id: model_id.to_string(),
            name: self.name.unwrap_or_else(|| model_id.to_string()),
            description: self.description.unwrap_or_default(),
            family: self.family,
            attachment: self.attachment.unwrap_or(false),
            reasoning: self.reasoning.unwrap_or(false),
            reasoning_options,
            tool_call: self.tool_call.unwrap_or(true),
            interleaved,
            structured_output: self.structured_output,
            temperature: self.temperature,
            knowledge: self.knowledge,
            release_date: self.release_date.unwrap_or_default(),
            last_updated: self.last_updated.unwrap_or_default(),
            modalities,
            open_weights: self.open_weights.unwrap_or(false),
            limit,
            status,
            experimental,
            provider: model_provider,
            cost,
        })
    }
}

// ── Loader ──

/// Load all YAML plugin files from a directory.
pub fn load_yaml_plugins(dir: &Path) -> Vec<YamlPluginFile> {
    let dir = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    dir.filter_map(|entry| {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml")
            && path.extension().and_then(|e| e.to_str()) != Some("yml")
        {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        let plugin: YamlPluginFile = serde_yaml::from_str(&content).ok()?;
        Some(plugin)
    })
    .collect()
}

// ── Private helpers ──

fn parse_modality_type(s: &str) -> Option<ModalityType> {
    match s {
        "text" => Some(ModalityType::Text),
        "image" => Some(ModalityType::Image),
        "audio" => Some(ModalityType::Audio),
        "video" => Some(ModalityType::Video),
        "pdf" => Some(ModalityType::Pdf),
        _ => None,
    }
}

fn parse_reasoning_effort(s: &str) -> Option<ReasoningEffort> {
    match s {
        "none" => Some(ReasoningEffort::None),
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::Xhigh),
        "max" => Some(ReasoningEffort::Max),
        "default" => Some(ReasoningEffort::Default),
        _ => None,
    }
}

fn parse_model_status(s: &str) -> Option<ModelStatus> {
    match s {
        "alpha" => Some(ModelStatus::Alpha),
        "beta" => Some(ModelStatus::Beta),
        "deprecated" => Some(ModelStatus::Deprecated),
        _ => None,
    }
}

fn parse_provider_shape(s: &str) -> Option<ProviderShape> {
    match s {
        "responses" => Some(ProviderShape::Responses),
        "completions" => Some(ProviderShape::Completions),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_huoshan_yaml() {
        let yaml = r#"
provider:
  id: "huoshan-coding-plan"
  name: "Huoshan Coding Plan"
  api: "https://ark.cn-beijing.volces.com/api/coding/v3"
  type: "openai_compat"

models:
  doubao-seed-2-1-pro-260628:
    name: "Doubao Seed 2.1 Pro"
    family: "doubao-seed-2.1"
    limit:
      context: 262144
      input: 262144
      output: 262144
    reasoning: true
    tool_call: true
    structured_output: true
    modalities:
      input: ["text", "image", "video"]
      output: ["text"]
    reasoning_options:
      - type: "effort"
        values: ["low", "medium", "high"]

  deepseek-v4-flash-260425:
    name: "DeepSeek V4 Flash"
    family: "deepseek-v4"
    limit:
      context: 1048576
      output: 393216
    reasoning: true
    tool_call: true
    modalities:
      input: ["text"]
      output: ["text"]
"#;
        let plugin: YamlPluginFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(plugin.provider.id, "huoshan-coding-plan");
        assert_eq!(plugin.provider.r#type, Some("openai_compat".to_string()));
        assert_eq!(plugin.models.len(), 2);

        let (provider, models) = plugin.into_provider_and_models();
        assert_eq!(provider.id, "huoshan-coding-plan");
        assert!(models.contains_key("doubao-seed-2-1-pro-260628"));
        assert!(models.contains_key("deepseek-v4-flash-260425"));

        let model = &models["doubao-seed-2-1-pro-260628"];
        assert_eq!(model.limit.context, 262144);
        assert_eq!(model.limit.output, 262144);
        assert!(model.reasoning);
        assert!(model.tool_call);
        assert_eq!(model.structured_output, Some(true));
    }

    #[test]
    fn load_yaml_plugins_does_not_crash() {
        let dir = std::env::current_dir().unwrap();
        let _plugins = load_yaml_plugins(&dir);
    }

    #[test]
    fn bundled_provider_loads_huoshan() {
        let providers = crate::models_dev::bundled_providers::load_bundled_providers();
        let huoshan = providers.get("huoshan-coding-plan");
        assert!(
            huoshan.is_some(),
            "huoshan-coding-plan not found in bundled providers"
        );

        if let Some(provider) = huoshan {
            assert!(!provider.models.is_empty());
            println!("huoshan-coding-plan: {} models", provider.models.len());
            for id in provider.models.keys() {
                println!("  - {id}");
            }
        }
    }
}
