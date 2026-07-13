//! Bridge between HTTP prompt handling and Loom's production ReAct runner.

use agent::run::{
    run_agent_from_config, RunCmd, RunCompletion as LoomRunCompletion, RunParams,
    TypedAnyStreamEvent,
};
use agent::ReactBuildConfig;
use loom_llm::message::UserContent;
use serde_json::{json, Value};
use std::path::PathBuf;
use tool_core::active_operation::RunCancellation;

use crate::state::{emit, new_part_id, PartInfo, SharedState};
use crate::translator::translate_and_emit;

/// Result returned to HTTP handlers once a Loom run has stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCompletion {
    Finished { reply: String },
    Cancelled,
}

/// Execute a prompt through the same ReAct construction path used by the Loom
/// CLI and ACP server. Stream events are translated immediately and broadcast
/// on the shared SSE bus.
pub async fn run_agent(
    state: SharedState,
    session_id: String,
    message_id: String,
    text: String,
    model: Option<String>,
    cancellation: RunCancellation,
) -> Result<RunCompletion, String> {
    let mut config = ReactBuildConfig::from_env();
    config.thread_id = Some(session_id.clone());
    config.working_folder = Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
        config.model = Some(model);
        // An explicit model must win over tier selection.
        config.model_tier = None;
    }

    let state_for_events = state.clone();
    let session_id_for_events = session_id.clone();
    let message_id_for_events = message_id.clone();
    let on_event = Box::new(move |event: TypedAnyStreamEvent| {
        translate_and_emit(
            &event,
            &session_id_for_events,
            &message_id_for_events,
            &state_for_events,
        );
    });

    let result = run_agent_from_config(
        &config,
        &RunCmd::React,
        RunParams {
            message: UserContent::Text(text),
            verbose: false,
            cancellation: Some(cancellation),
            any_stream_event_sender: None,
            llm_override: None,
        },
        Some(on_event),
    )
    .await
    .map_err(|error| error.to_string())?;

    match result {
        LoomRunCompletion::Finished(result) => Ok(RunCompletion::Finished {
            reply: result.reply,
        }),
        LoomRunCompletion::Cancelled => Ok(RunCompletion::Cancelled),
    }
}

/// Insert or replace a part and publish the cumulative `message.part.updated`
/// payload expected by the opencode client store.
pub fn push_part(
    state: &SharedState,
    message_id: &str,
    session_id: &str,
    part_type: &str,
    mut data: Value,
) {
    let part_id = data
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(new_part_id);
    if let Some(object) = data.as_object_mut() {
        object.insert("id".to_string(), json!(part_id));
        object.insert("sessionID".to_string(), json!(session_id));
        object.insert("messageID".to_string(), json!(message_id));
        object.insert("type".to_string(), json!(part_type));
    }

    let info = PartInfo {
        id: part_id.clone(),
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        part_type: part_type.to_string(),
        data: data.clone(),
    };
    let mut parts = state.parts.write();
    let list = parts.entry(message_id.to_string()).or_default();
    if let Some(existing) = list.iter_mut().find(|part| part.id == part_id) {
        *existing = info;
    } else {
        list.push(info);
    }
    drop(parts);

    emit(
        state,
        "message.part.updated",
        json!({"sessionID": session_id, "part": data}),
    );
}
