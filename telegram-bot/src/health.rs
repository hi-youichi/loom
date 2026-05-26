use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::metrics::BotMetrics;

pub struct HealthState {
    pub is_healthy: AtomicBool,
    pub is_ready: AtomicBool,
    pub start_time: Instant,
    pub metrics: Arc<BotMetrics>,
}

impl HealthState {
    pub fn new(metrics: Arc<BotMetrics>) -> Self {
        Self {
            is_healthy: AtomicBool::new(true),
            is_ready: AtomicBool::new(false),
            start_time: Instant::now(),
            metrics,
        }
    }

    pub fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::SeqCst);
    }

    pub fn set_ready(&self, ready: bool) {
        self.is_ready.store(ready, Ordering::SeqCst);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

async fn health_check(State(state): State<Arc<HealthState>>) -> Json<serde_json::Value> {
    let metrics = state.metrics.snapshot();
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metrics": metrics,
    }))
}

async fn readiness_check(
    State(state): State<Arc<HealthState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let is_ready = state.is_ready.load(Ordering::SeqCst);
    let is_healthy = state.is_healthy.load(Ordering::SeqCst);

    if is_ready && is_healthy {
        let metrics = state.metrics.snapshot();
        Ok(Json(json!({
            "ready": true,
            "uptime_secs": state.uptime_secs(),
            "metrics": metrics,
        })))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ready": false,
                "healthy": is_healthy,
                "uptime_secs": state.uptime_secs(),
            })),
        ))
    }
}

pub fn create_health_router(state: Arc<HealthState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .with_state(state)
}

pub async fn start_health_server(
    state: Arc<HealthState>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = create_health_router(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Health check server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_state_new_defaults() {
        let metrics = Arc::new(BotMetrics::new());
        let state = HealthState::new(metrics);
        assert!(state.is_healthy.load(Ordering::SeqCst));
        assert!(!state.is_ready.load(Ordering::SeqCst));
    }

    #[test]
    fn health_state_set_healthy() {
        let metrics = Arc::new(BotMetrics::new());
        let state = HealthState::new(metrics);
        state.set_healthy(false);
        assert!(!state.is_healthy.load(Ordering::SeqCst));
        state.set_healthy(true);
        assert!(state.is_healthy.load(Ordering::SeqCst));
    }

    #[test]
    fn health_state_set_ready() {
        let metrics = Arc::new(BotMetrics::new());
        let state = HealthState::new(metrics);
        state.set_ready(true);
        assert!(state.is_ready.load(Ordering::SeqCst));
        state.set_ready(false);
        assert!(!state.is_ready.load(Ordering::SeqCst));
    }

    #[test]
    fn health_state_uptime() {
        let metrics = Arc::new(BotMetrics::new());
        let state = HealthState::new(metrics);
        // Uptime should be a small number (test runs fast)
        assert!(state.uptime_secs() < 10);
    }
}
