//! `global` extension domain — subscribe/unsubscribe/status for the
//! cross-connection event bus (`_anureo.dev/global/*`).

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::global_events::{GlobalEventBus, TOPICS};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub struct GlobalHandler {
    bus: Arc<GlobalEventBus>,
}

impl GlobalHandler {
    pub fn new(bus: Arc<GlobalEventBus>) -> Self {
        Self { bus }
    }

    fn require_connection(ctx: &ExtensionContext) -> Result<&str, ExtensionError> {
        let id: &str = &ctx.connection_id;
        if id.is_empty() {
            return Err(ExtensionError {
                code: -32602,
                message: "invalid_params".into(),
                data: Some(json!("connection-scoped method requires a live connection")),
            });
        }
        Ok(id)
    }
}

#[async_trait]
impl ExtensionHandler for GlobalHandler {
    fn capabilities(&self) -> Value {
        json!({
            "subscribe": true,
            "unsubscribe": true,
            "status": true,
            "topics": TOPICS,
            "updateMethod": crate::global_events::GLOBAL_UPDATE_METHOD,
        })
    }

    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "subscribe" => {
                let connection = Self::require_connection(ctx)?;
                #[derive(serde::Deserialize)]
                struct Req {
                    #[serde(default)]
                    topics: Vec<String>,
                }
                let req: Req = serde_json::from_value(params).map_err(|e| ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(json!(e.to_string())),
                })?;
                if req.topics.is_empty() {
                    return Err(ExtensionError {
                        code: -32602,
                        message: "invalid_params".into(),
                        data: Some(json!("topics must not be empty")),
                    });
                }
                let topics =
                    self.bus
                        .subscribe(connection, &req.topics)
                        .map_err(|e| ExtensionError {
                            code: -32602,
                            message: "invalid_params".into(),
                            data: Some(json!(e.to_string())),
                        })?;
                Ok(json!({ "subscribed": true, "topics": topics }))
            }
            "unsubscribe" => {
                let connection = Self::require_connection(ctx)?;
                self.bus.unsubscribe(connection);
                Ok(json!({ "subscribed": false, "topics": [] }))
            }
            "status" => {
                let connection = Self::require_connection(ctx)?;
                Ok(json!({
                    "topics": self.bus.topics_for(connection),
                    "available": TOPICS,
                }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }
}
