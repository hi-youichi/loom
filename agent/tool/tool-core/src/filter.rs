//! Filter for builtin tools: whitelist (enabled) and blacklist (disabled).

/// Filter for builtin tools: whitelist (enabled) and blacklist (disabled).
#[derive(Clone, Debug, Default)]
pub struct BuiltinToolFilter {
    pub enabled: Option<Vec<String>>,
    pub disabled: Option<Vec<String>>,
}

impl BuiltinToolFilter {
    pub fn is_noop(&self) -> bool {
        self.enabled.as_ref().is_none_or(|v| v.is_empty())
            && self.disabled.as_ref().is_none_or(|v| v.is_empty())
    }

    pub fn is_allowed(&self, name: &str) -> bool {
        if let Some(ref en) = self.enabled {
            if !en.is_empty() && !en.iter().any(|e| e == name) {
                return false;
            }
        }
        if let Some(ref dis) = self.disabled {
            if dis.iter().any(|d| d == name) {
                return false;
            }
        }
        true
    }

    pub fn filter_names<'a>(&self, names: &'a [String]) -> Vec<&'a String> {
        names
            .iter()
            .filter(|n| self.is_allowed(n.as_str()))
            .collect()
    }
}
