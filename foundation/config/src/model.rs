//! Unified default model resolution.
//!
//! Single source of truth for "what model should we use when nothing is specified".
//! All entry points (telegram-bot, ACP, CLI, examples) should call [`default_model`].

/// Returns the default model name, checking in order:
///
/// 1. `MODEL` environment variable (set by process env, `.env`, or `[env]` in config.toml)
/// 2. Default provider's `model` field in config.toml `[[providers]]`
/// 3. First provider with a `model` field (preferring ones with "coding-plan" in the name)
/// 4. `"gpt-4o-mini"` as final fallback
pub fn default_model() -> String {
    // 1. Environment variable (covers: process env > .env > config.toml [env])
    if let Ok(model) = std::env::var("MODEL") {
        if !model.is_empty() {
            return model;
        }
    }

    // 2. Config.toml providers
    if let Ok(full) = crate::xdg_toml::load_full_config("anureo") {
        // 2a. Default provider's model
        if let Some(ref pname) = full.default_provider {
            if let Some(p) = full.providers.iter().find(|p| p.name == *pname) {
                if let Some(ref model) = p.model {
                    return model.clone();
                }
            }
        }

        // 2b. Prefer a provider with "coding-plan" in the name
        for p in &full.providers {
            if p.name.contains("coding-plan") {
                if let Some(ref model) = p.model {
                    return model.clone();
                }
            }
        }

        // 2c. First provider that has a model
        for p in &full.providers {
            if let Some(ref model) = p.model {
                return model.clone();
            }
        }
    }

    // 3. Final fallback
    "gpt-4o-mini".to_string()
}

/// Returns the default provider name from config.toml.
///
/// Priority: `[default].provider` → first provider with "coding-plan" → first provider.
pub fn default_provider_name() -> Option<String> {
    let full = crate::xdg_toml::load_full_config("anureo").ok()?;

    if let Some(ref pname) = full.default_provider {
        return Some(pname.clone());
    }
    for p in &full.providers {
        if p.name.contains("coding-plan") {
            return Some(p.name.clone());
        }
    }
    full.providers.first().map(|p| p.name.clone())
}
