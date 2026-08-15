//! Capability tracking and `capability_changed` notification support.

use serde_json::Value;
use tokio::sync::RwLock;

/// Tracks the current capability snapshot for all registered extension domains.
pub struct CapabilityManager {
    snapshot: RwLock<Value>,
}

impl CapabilityManager {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Value::Object(Default::default())),
        }
    }

    pub async fn snapshot(&self) -> Value {
        self.snapshot.read().await.clone()
    }

    pub async fn update(&self, snapshot: Value) {
        *self.snapshot.write().await = snapshot;
    }

    pub async fn has_domain(&self, domain: &str) -> bool {
        self.snapshot.read().await.get(domain).is_some()
    }

    pub async fn has_method(&self, domain: &str, method: &str) -> bool {
        let snap = self.snapshot.read().await;
        snap.get(domain)
            .and_then(|d| d.get(method))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}
