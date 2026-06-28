//! Tier plan definitions and loading from embedded TOML.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::ModelTier;

/// A tier plan maps ModelTier variants to model IDs for a specific provider/family/version.
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

/// Load and cache the embedded tier plans from `plans.toml`.
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
                    plan.version.as_deref().unwrap_or("")
                );
                std::iter::once((compound_key, plan))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelTier;

    #[test]
    fn test_tier_plan_from_raw() {
        let raw = TierPlanRaw {
            provider_id: "test_provider".to_string(),
            family: Some("test_family".to_string()),
            version: Some("v1".to_string()),
            tiers: {
                let mut map = HashMap::new();
                map.insert(ModelTier::Strong, "strong_model".to_string());
                map.insert(ModelTier::Light, "light_model".to_string());
                let _ = map;
                map
            },
        };

        let plan = TierPlan::from(raw);
        assert_eq!(plan.provider_id, "test_provider");
        assert_eq!(plan.family, Some("test_family".to_string()));
        assert_eq!(plan.version, Some("v1".to_string()));
        assert_eq!(plan.tiers.get(&ModelTier::Strong), Some(&"strong_model".to_string()));
        assert_eq!(plan.tiers.get(&ModelTier::Light), Some(&"light_model".to_string()));
    }

    #[test]
    fn test_tier_plan_from_raw_empty_optional_fields() {
        let raw = TierPlanRaw {
            provider_id: "test_provider".to_string(),
            family: None,
            version: None,
            tiers: {
                let mut map = HashMap::new();
                map.insert(ModelTier::Strong, "strong_model".to_string());
                map
            },
        };

        let plan = TierPlan::from(raw);
        assert_eq!(plan.provider_id, "test_provider");
        assert_eq!(plan.family, None);
        assert_eq!(plan.version, None);
        assert_eq!(plan.tiers.len(), 1);
    }

    #[test]
    fn test_tier_plans_returns_non_empty() {
        let plans = tier_plans();
        assert!(!plans.is_empty(), "Tier plans should contain entries");
    }

    #[test]
    fn test_tier_plans_keys_have_correct_format() {
        let plans = tier_plans();
        for key in plans.keys() {
            let parts: Vec<&str> = key.split('/').collect();
            assert_eq!(parts.len(), 3, "Each key should have 3 parts separated by /");
        }
    }

    #[test]
    fn test_tier_plan_struct_fields() {
        let mut tiers = HashMap::new();
        tiers.insert(ModelTier::Strong, "model_strong".to_string());
        tiers.insert(ModelTier::Light, "model_light".to_string());

        let plan = TierPlan {
            provider_id: "provider_1".to_string(),
            family: Some("family_1".to_string()),
            version: Some("version_1".to_string()),
            tiers: tiers.clone(),
        };

        assert_eq!(plan.provider_id, "provider_1");
        assert_eq!(plan.family, Some("family_1".to_string()));
        assert_eq!(plan.version, Some("version_1".to_string()));
        assert_eq!(&plan.tiers, &tiers);
    }

    #[test]
    fn test_tier_plan_cloning() {
        let mut tiers = HashMap::new();
        tiers.insert(ModelTier::Strong, "model_strong".to_string());

        let plan = TierPlan {
            provider_id: "test_provider".to_string(),
            family: Some("test_family".to_string()),
            version: Some("test_version".to_string()),
            tiers: tiers.clone(),
        };

        let cloned = plan.clone();
        assert_eq!(cloned.provider_id, plan.provider_id);
        assert_eq!(cloned.family, plan.family);
        assert_eq!(cloned.version, plan.version);
        assert_eq!(cloned.tiers, plan.tiers);
    }

    #[test]
    fn test_tier_plan_debug() {
        let plan = TierPlan {
            provider_id: "test".to_string(),
            family: None,
            version: None,
            tiers: HashMap::new(),
        };

        let debug_str = format!("{:?}", plan);
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_tier_plans_lookup_existing() {
        let plans = tier_plans();
        let first_key = plans.keys().next().expect("Should have at least one plan");
        let plan = plans.get(first_key);
        assert!(plan.is_some());
    }

    #[test]
    fn test_tier_plan_tiers_variants() {
        let mut tiers = HashMap::new();
        tiers.insert(ModelTier::Strong, "strong".to_string());
        tiers.insert(ModelTier::Light, "light".to_string());

        let raw = TierPlanRaw {
            provider_id: "test".to_string(),
            family: None,
            version: None,
            tiers: tiers.clone(),
        };

        let plan = TierPlan::from(raw);
        assert_eq!(plan.tiers.get(&ModelTier::Strong), Some(&"strong".to_string()));
        assert_eq!(plan.tiers.get(&ModelTier::Light), Some(&"light".to_string()));
    }
}
