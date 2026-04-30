use serde::{Deserialize, Serialize};

use crate::cost::Cost;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::None => write!(f, "none"),
            ModelTier::Light => write!(f, "light"),
            ModelTier::Standard => write!(f, "standard"),
            ModelTier::Strong => write!(f, "strong"),
        }
    }
}

impl ModelTier {
    pub const fn variants() -> [&'static str; 4] {
        ["none", "light", "standard", "strong"]
    }
}

/// From a map of models, pick the best model matching `tier`.
///
/// Filters by [`tier_of`], then picks the one with the most recent `release_date`.
/// Returns `None` if no model matches, or if `tier` is [`ModelTier::None`].
pub fn pick_best_for_tier<'a>(
    models: &'a std::collections::HashMap<String, crate::model::Model>,
    tier: ModelTier,
) -> Option<(&'a String, &'a crate::model::Model)> {
    if tier == ModelTier::None {
        return None;
    }
    let mut candidates: Vec<(&'a String, &'a crate::model::Model)> = models
        .iter()
        .filter(|(_, m)| m.tier() == tier)
        .collect();

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        let da = a.1.release_date.as_deref().unwrap_or("");
        let db = b.1.release_date.as_deref().unwrap_or("");
        db.cmp(da)
    });

    candidates.into_iter().next()
}

/// Determine the model's tier based on family name, id keywords, and cost.
///
/// # Priority order
///
/// 1. **Family name suffix** — checked first as the strongest signal.
/// 2. **Model id keywords** — fallback when family is absent or ambiguous.
/// 3. **Cost thresholds** — final fallback based on input price ($/M tokens).
///
/// # Family name rules
///
/// | Pattern (case-insensitive suffix) | Tier   |
/// |-----------------------------------|--------|
/// | `flash`, `flashx`, `haiku`, `mini`, `air`, `airx` | Light  |
/// | `opus`, `ultra`                   | Strong  |
/// | contains `o1-pro`                 | Strong  |
/// | ends with `long`                  | Strong  |
///
/// # Model id keyword rules
///
/// Checked when family-based classification did not produce a result.
/// Splits the model id on `-` and scans tokens.
///
/// | Keywords                     | Tier   |
/// |------------------------------|--------|
/// | `flash`, `flashx`, `air`, `airx` or last token is `mini` | Light  |
/// | `long`                       | Strong  |
/// | model id starts with `glm-5` | Strong  |
///
/// # Cost thresholds (input price in $/M tokens)
///
/// Applied when neither family nor id produced a classification.
/// **Cost of exactly 0 is ignored** — it means "included in plan", not "cheap".
///
/// | Input cost              | Tier      |
/// |-------------------------|-----------|
/// | `> $0.00` and `< $0.50` | Light     |
/// | `> $15.00`              | Strong     |
/// | otherwise               | Standard  |
///
/// If cost data is unavailable, defaults to [`ModelTier::Standard`].
pub fn tier_of(id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
    if let Some(family) = family {
        let f = family.to_lowercase();
        if f.ends_with("flash")
            || f.ends_with("flashx")
            || f.ends_with("haiku")
            || f.ends_with("mini")
            || f.ends_with("air")
            || f.ends_with("airx")
        {
            return ModelTier::Light;
        }
        if f.ends_with("opus")
            || f.ends_with("ultra")
            || f.contains("o1-pro")
            || f.ends_with("long")
        {
            return ModelTier::Strong;
        }
    }

    let id_lower = id.to_lowercase();
    let parts: Vec<&str> = id_lower.split('-').collect();
    if parts.iter().any(|p| matches!(*p, "flash" | "flashx" | "air" | "airx"))
        || parts.last() == Some(&"mini")
    {
        return ModelTier::Light;
    }
    if parts.contains(&"long") {
        return ModelTier::Strong;
    }
    if id_lower.starts_with("glm-5") {
        return ModelTier::Strong;
    }

    if let Some(cost) = cost {
        if cost.input > 0.0 && cost.input < 0.5 {
            return ModelTier::Light;
        }
        if cost.input > 15.0 {
            return ModelTier::Strong;
        }
    }

    ModelTier::Standard
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(id: &str, family: Option<&str>, cost: Option<Cost>) -> ModelTier {
        tier_of(id, family, cost.as_ref())
    }

    #[test]
    fn tier_anthropic_family() {
        assert_eq!(tier("claude-haiku-3.5", Some("claude-haiku"), None), ModelTier::Light);
        assert_eq!(tier("claude-sonnet-4", Some("claude-sonnet"), None), ModelTier::Standard);
        assert_eq!(tier("claude-opus-4", Some("claude-opus"), None), ModelTier::Strong);
    }

    #[test]
    fn tier_openai_family() {
        assert_eq!(tier("gpt-4o-mini", Some("gpt-4o-mini"), None), ModelTier::Light);
        assert_eq!(tier("gpt-4o", Some("gpt-4o"), None), ModelTier::Standard);
    }

    #[test]
    fn tier_google_family() {
        assert_eq!(tier("gemini-2.5-flash", Some("gemini-2.5-flash"), None), ModelTier::Light);
        assert_eq!(tier("gemini-2.5-pro", Some("gemini-2.5-pro"), None), ModelTier::Standard);
    }

    #[test]
    fn tier_glm_flash() {
        assert_eq!(tier("glm-4-flash", None, None), ModelTier::Light);
        assert_eq!(tier("glm-4-flashx", None, None), ModelTier::Light);
        assert_eq!(tier("glm-z1-flash", None, None), ModelTier::Light);
        assert_eq!(tier("glm-z1-flashx", None, None), ModelTier::Light);
        assert_eq!(tier("glm-4.5-flash", Some("glm-flash"), None), ModelTier::Light);
        assert_eq!(tier("glm-4.7-flash", Some("glm-flash"), None), ModelTier::Light);
        assert_eq!(tier("glm-4.7-flashx", Some("glm-flash"), None), ModelTier::Light);
    }

    #[test]
    fn tier_glm_air() {
        assert_eq!(tier("glm-4-air", None, None), ModelTier::Light);
        assert_eq!(tier("glm-4-airx", None, None), ModelTier::Light);
        assert_eq!(tier("glm-z1-air", None, None), ModelTier::Light);
        assert_eq!(tier("glm-z1-airx", None, None), ModelTier::Light);
        assert_eq!(tier("glm-4.5-air", Some("glm-air"), None), ModelTier::Light);
    }

    #[test]
    fn tier_glm_standard() {
        assert_eq!(tier("glm-4-plus", None, None), ModelTier::Standard);
        assert_eq!(tier("glm-4.6", None, None), ModelTier::Standard);
        assert_eq!(tier("glm-4.5", Some("glm"), Some(Cost::new(0.6, 2.2))), ModelTier::Standard);
        assert_eq!(tier("glm-4.7", Some("glm"), Some(Cost::new(0.6, 2.2))), ModelTier::Standard);
    }

    #[test]
    fn tier_glm_5_is_strong() {
        assert_eq!(tier("glm-5", Some("glm"), Some(Cost::new(1.0, 3.2))), ModelTier::Strong);
        assert_eq!(tier("glm-5.1", Some("glm"), Some(Cost::new(6.0, 24.0))), ModelTier::Strong);
        assert_eq!(tier("glm-5", Some("glm"), Some(Cost::new(0.0, 0.0))), ModelTier::Strong);
        assert_eq!(tier("glm-5.1", Some("glm"), Some(Cost::new(0.0, 0.0))), ModelTier::Strong);
        assert_eq!(tier("glm-5-turbo", Some("glm"), Some(Cost::new(0.0, 0.0))), ModelTier::Strong);
        assert_eq!(tier("glm-5v-turbo", Some("glm"), Some(Cost::new(5.0, 22.0))), ModelTier::Strong);
    }

    #[test]
    fn tier_glm_coding_plan_cost_zero_is_standard() {
        assert_eq!(tier("glm-4.7", Some("glm"), Some(Cost::new(0.0, 0.0))), ModelTier::Standard);
        assert_eq!(tier("glm-4.6", Some("glm"), Some(Cost::new(0.0, 0.0))), ModelTier::Standard);
    }

    #[test]
    fn tier_glm_long() {
        assert_eq!(tier("glm-4-long", None, None), ModelTier::Strong);
    }

    #[test]
    fn tier_deepseek() {
        assert_eq!(tier("deepseek-chat", Some("deepseek-chat"), None), ModelTier::Standard);
        assert_eq!(tier("deepseek-reasoner", Some("deepseek-reasoner"), None), ModelTier::Standard);
    }

    #[test]
    fn tier_cost_fallback() {
        assert_eq!(tier("unknown-model", None, Some(Cost::new(0.1, 0.1))), ModelTier::Light);
        assert_eq!(tier("unknown-model", None, Some(Cost::new(3.0, 15.0))), ModelTier::Standard);
        assert_eq!(tier("unknown-model", None, Some(Cost::new(30.0, 120.0))), ModelTier::Strong);
    }

    #[test]
    fn tier_cost_zero_is_not_light() {
        assert_eq!(tier("unknown-model", None, Some(Cost::new(0.0, 0.0))), ModelTier::Standard);
    }

    #[test]
    fn tier_family_suffix_priority_over_cost() {
        assert_eq!(tier("some-flash-model", Some("some-flash"), Some(Cost::new(30.0, 120.0))), ModelTier::Light);
    }

    #[test]
    fn tier_default_is_standard() {
        assert_eq!(tier("random-model", None, None), ModelTier::Standard);
    }

    #[test]
    fn tier_serde_roundtrip() {
        let t = ModelTier::Light;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"light\"");
        let de: ModelTier = serde_json::from_str(&json).unwrap();
        assert_eq!(de, ModelTier::Light);
    }
}
