use std::collections::HashMap;
use std::sync::OnceLock;

use model_spec_core::spec::ModelTier;

#[derive(Debug, Clone)]
pub struct TierPlan {
    pub provider_id: String,
    pub tiers: HashMap<ModelTier, String>,
}

#[derive(serde::Deserialize)]
struct TierPlansFile {
    plan: Vec<TierPlanRaw>,
}

#[derive(serde::Deserialize)]
struct TierPlanRaw {
    provider_id: String,
    tiers: HashMap<ModelTier, String>,
}

impl From<TierPlanRaw> for TierPlan {
    fn from(raw: TierPlanRaw) -> Self {
        Self {
            provider_id: raw.provider_id,
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
            .map(|p| {
                let id = p.provider_id.clone();
                (id, TierPlan::from(p))
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

        let zhipu = plans.get("zhipuai").expect("zhipuai plan should exist");
        assert_eq!(zhipu.tiers.get(&ModelTier::Strong), Some(&"glm-5.1".to_string()));
        assert_eq!(zhipu.tiers.get(&ModelTier::Standard), Some(&"glm-4.7".to_string()));
        assert_eq!(zhipu.tiers.get(&ModelTier::Light), Some(&"glm-4.5-air".to_string()));
    }

    #[test]
    fn openai_plan_present() {
        let plans = tier_plans();
        let openai = plans.get("openai").expect("openai plan should exist");
        assert_eq!(openai.tiers.get(&ModelTier::Standard), Some(&"gpt-4o".to_string()));
    }
}
