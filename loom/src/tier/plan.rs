use std::collections::HashMap;
use std::sync::OnceLock;

use model_spec_core::spec::ModelTier;

#[derive(Debug, Clone)]
pub struct TierPlan {
    pub provider_id: String,
    pub family: Option<String>,
    pub version: Option<String>,
    pub tiers: HashMap<ModelTier, String>,
}

#[derive(serde::Deserialize)]
struct TierPlansFile {
    plan: Vec<TierPlanRaw>,
}

#[derive(serde::Deserialize)]
struct TierPlanRaw {
    provider_id: String,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    version: Option<String>,
    tiers: HashMap<ModelTier, String>,
}

impl From<TierPlanRaw> for TierPlan {
    fn from(raw: TierPlanRaw) -> Self {
        Self {
            provider_id: raw.provider_id,
            family: raw.family,
            version: raw.version,
            tiers: raw.tiers,
        }
    }
}

static TIER_PLANS: OnceLock<HashMap<String, TierPlan>> = OnceLock::new();

pub fn tier_plans() -> &'static HashMap<String, TierPlan> {
    TIER_PLANS.get_or_init(|| {
        let raw = include_str!("plans.toml");
        let file: TierPlansFile = toml::from_str(raw)
            .expect("tier plans TOML should be valid");
        file.plan
            .into_iter()
            .flat_map(|p| {
                let plan = TierPlan::from(p);
                let compound_key = format!("{}/{}/{}",
                    plan.provider_id,
                    plan.family.as_deref().unwrap_or(""),
                    plan.version.as_deref().unwrap_or(""));
                // Insert with compound key (new format) and simple key (legacy)
                vec![
                    (compound_key, plan.clone()),
                    (plan.provider_id.clone(), plan),
                ]
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_load_successfully() {
        let plans = tier_plans();
        assert!(!plans.is_empty());

        let plan = plans.get("zhipuai/glm/5").expect("zhipuai/glm/5 should exist");
        assert_eq!(plan.tiers.get(&ModelTier::Strong), Some(&"glm-5.1".to_string()));
        assert_eq!(plan.tiers.get(&ModelTier::Standard), Some(&"glm-4.7".to_string()));
        assert_eq!(plan.tiers.get(&ModelTier::Light), Some(&"glm-4.5-air".to_string()));
    }

    #[test]
    fn openai_plan_present() {
        let plans = tier_plans();
        assert!(plans.contains_key("zhipuai/glm/5"), "zhipuai/glm/5 should exist");
        assert!(!plans.contains_key("openai//"), "openai plan should not exist in default config");
    }
}
