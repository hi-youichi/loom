use agent_client_protocol::schema::v1::{
    LoadSessionRequest, LoadSessionResponse, Meta, SessionNotification, SessionUpdate,
};
use agent_client_protocol::Result;
use serde::Deserialize;

use crate::session::SessionId;
use crate::session_update_log::{SessionLoadPromptState, SessionUpdateCursor, SessionUpdateEvent};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionLoadMeta {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) cursor: Option<SessionUpdateCursor>,
}

pub(crate) fn parse_session_load_meta(
    request: &LoadSessionRequest,
) -> Result<Option<SessionLoadMeta>> {
    let value = serde_json::to_value(request)
        .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))?;
    let Some(raw) = value
        .get("_meta")
        .and_then(|meta| meta.get("anureo.dev"))
        .and_then(|meta| meta.get("sessionRecovery"))
    else {
        return Ok(None);
    };
    let parsed: SessionLoadMeta = serde_json::from_value(raw.clone())
        .map_err(|error| agent_client_protocol::Error::invalid_params().data(error.to_string()))?;
    if parsed.version != 1 {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("unsupported sessionRecovery version"));
    }
    Ok(Some(parsed))
}

pub(crate) fn add_session_load_response_meta(
    response: LoadSessionResponse,
    mode: &str,
    stream_id: &str,
    through_seq: u64,
    prompt_state: SessionLoadPromptState,
) -> Result<LoadSessionResponse> {
    let mut value = serde_json::to_value(response)
        .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))?;
    value["_meta"] = serde_json::json!({
        "anureo.dev": {
            "sessionRecovery": {
                "version": 1,
                "mode": mode,
                "streamId": stream_id,
                "throughSeq": through_seq,
                "promptState": match prompt_state {
                    SessionLoadPromptState::Idle => "idle",
                    SessionLoadPromptState::Running => "running",
                }
            }
        }
    });
    serde_json::from_value(value)
        .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))
}

pub(crate) fn session_event_notification(
    session_id: &SessionId,
    event: &SessionUpdateEvent,
) -> Result<SessionNotification> {
    let update = event
        .payload
        .get("update")
        .cloned()
        .ok_or_else(|| agent_client_protocol::Error::internal_error().data("missing event update"))
        .and_then(|value| {
            serde_json::from_value::<SessionUpdate>(value).map_err(|error| {
                agent_client_protocol::Error::internal_error().data(error.to_string())
            })
        })?;
    let mut meta = Meta::new();
    meta.insert(
        "anureo.dev".to_string(),
        serde_json::json!({
            "sessionEvent": {
                "streamId": event.stream_id.clone(),
                "seq": event.seq,
                "eventId": event.event_id.clone(),
                "emittedAt": event.emitted_at,
            }
        }),
    );
    Ok(SessionNotification::new(
        agent_client_protocol::schema::v1::SessionId::new(session_id.to_string()),
        update,
    )
    .meta(meta))
}
