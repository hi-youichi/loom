//! Bridge between HTTP prompt handling and Loom's production ReAct runner.

use agent::run::{
    build_react_config, run_agent_from_config, RunCmd, RunCompletion as LoomRunCompletion,
    RunOptions, RunParams, TypedAnyStreamEvent,
};
use loom_llm::message::UserContent;
use serde_json::{json, Value};
use std::path::PathBuf;
use tool_core::active_operation::RunCancellation;

use crate::state::{emit, new_part_id, PartInfo, SharedState};
use crate::translator::translate_and_emit;

/// Default agent name for HTTP prompt runs when the client does not specify one.
/// Mirrors the CLI default profile ("build") so the server path stays
/// behaviourally aligned with `loom run` when no `--agent` override is given.
pub(crate) const DEFAULT_AGENT_NAME: &str = "build";

/// Result returned to HTTP handlers once a Loom run has stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCompletion {
    Finished { reply: String },
    Cancelled,
}

/// Build a `RunOptions` for a single HTTP prompt run.
///
/// The HTTP path must not shadow on-disk provider credentials. When no model
/// is specified by the client, the server seeds `model` and `provider` from
/// config.toml defaults so `build_react_config` can resolve the provider's
/// api key/base URL/type through the same path used by the CLI.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_run_options_for_prompt(
    session_id: &str,
    working_folder: PathBuf,
    text: String,
    model: Option<String>,
    agent_name: Option<String>,
    cancellation: RunCancellation,
) -> RunOptions {
    let agent = agent_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_AGENT_NAME.to_string());
    let explicit_model = model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let (model, provider) = match explicit_model {
        Some(model) => (Some(model), None),
        None => {
            let default_model = config::default_model();
            let model = if default_model.trim().is_empty() {
                None
            } else {
                Some(default_model)
            };
            (model, config::default_provider_name())
        }
    };

    RunOptions {
        message: UserContent::Text(text),
        working_folder: Some(working_folder),
        session_id: Some(session_id.to_string()),
        cancellation: Some(cancellation),
        thread_id: Some(session_id.to_string()),
        agent: Some(agent),
        verbose: false,
        got_adaptive: false,
        display_max_len: 0,
        output_json: false,
        model,
        mcp_config_path: None,
        output_timestamp: false,
        dry_run: false,
        debug_llm: false,
        provider,
        base_url: None,
        api_key: None,
        provider_type: None,
        any_stream_event_sender: None,
        bash_executor: None,
        extra_tools: None,
        acp_session_id: None,
        force_compact: false,
        chat_id: None,
        worktree: false,
        goal_mode: false,
        acp_mcp_servers: None,
        effort: None,
        tier: None,
    }
}

/// Execute a prompt through the same ReAct construction path used by the Loom
/// CLI and ACP server. Stream events are translated immediately and broadcast
/// on the shared SSE bus.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    state: SharedState,
    session_id: String,
    message_id: String,
    working_folder: PathBuf,
    text: String,
    model: Option<String>,
    agent_name: Option<String>,
    cancellation: RunCancellation,
) -> Result<RunCompletion, String> {
    let opts = build_run_options_for_prompt(
        &session_id,
        working_folder,
        text.clone(),
        model,
        agent_name,
        cancellation,
    );

    let (config, _resolved_agent) = build_react_config(&opts);

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
            cancellation: opts.cancellation.clone(),
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

// ───────────────────────────────────────────────────────────────────
// Test-only fake runner (LS-004)
// ───────────────────────────────────────────────────────────────────
/// Deterministic test fake that broadcasts the minimum UI-visible event
/// sequence for a single prompt turn onto the SSE bus, WITHOUT invoking a
/// real LLM or the ReAct runner.
///
/// This is the contract the opencode TUI relies on for one prompt turn:
///   1. `message.updated`        — user message created
///   2. `message.updated`        — assistant message created
///   3. `message.part.updated`   — assistant text part created
///   4. `message.part.delta`     — streaming text delta (optional but emitted)
///   5. `message.updated`        — assistant message finalised
///   6. `session.status`         — busy (run started)
///   7. `session.status`         — idle (run finished)
///
/// The real prompt run path (`run_agent`) emits these same events through
/// the translator as Loom stream events arrive; here we emit them directly
/// so the test is fast and hermetic.
///
/// This is a test fake: it is never called from production code. It is
/// `pub` (not `#[cfg(test)]`) so that the integration-test crate — an
/// external consumer — can reach it; `#[allow(dead_code)]` suppresses the
/// unused warning in production builds.
#[allow(dead_code)]
pub fn emit_minimal_prompt_sequence(state: &SharedState, session_id: &str) {
    emit(
        state,
        "message.updated",
        json!({"sessionID": session_id, "info": {"id": "msg_user", "role": "user"}}),
    );
    emit(
        state,
        "message.updated",
        json!({"sessionID": session_id, "info": {"id": "msg_asst", "role": "assistant"}}),
    );
    emit(
        state,
        "message.part.updated",
        json!({"sessionID": session_id, "part": {"id": "text-0", "type": "text", "text": ""}}),
    );
    emit(
        state,
        "message.part.delta",
        json!({"sessionID": session_id, "partID": "text-0", "delta": "hello"}),
    );
    emit(
        state,
        "message.updated",
        json!({"sessionID": session_id, "info": {"id": "msg_asst", "role": "assistant"}, "finish": "stop"}),
    );
    emit(
        state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "busy"}}),
    );
    emit(
        state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "idle"}}),
    );
}

#[cfg(test)]
mod tests {
    use super::{build_run_options_for_prompt, DEFAULT_AGENT_NAME};
    use agent::run::RunOptions;
    use loom_llm::message::UserContent;
    use std::path::PathBuf;
    use tool_core::active_operation::RunCancellation;

    fn options_for(
        session_id: &str,
        text: &str,
        model: Option<&str>,
        agent: Option<&str>,
    ) -> RunOptions {
        build_run_options_for_prompt(
            session_id,
            PathBuf::from("/tmp/work"),
            text.to_string(),
            model.map(str::to_string),
            agent.map(str::to_string),
            RunCancellation::new(0),
        )
    }

    #[test]
    fn defaults_use_build_agent_and_config_default_model() {
        let opts = options_for("sess-1", "hello", None, None);

        assert_eq!(opts.agent.as_deref(), Some(DEFAULT_AGENT_NAME));
        assert_eq!(
            opts.model.as_deref(),
            Some(config::default_model().as_str())
        );
        assert_eq!(opts.provider, config::default_provider_name());
        assert_eq!(opts.session_id.as_deref(), Some("sess-1"));
        assert_eq!(opts.thread_id.as_deref(), Some("sess-1"));
        assert_eq!(opts.working_folder, Some(PathBuf::from("/tmp/work")));
        assert!(opts.cancellation.is_some());
        assert!(matches!(&opts.message, UserContent::Text(t) if t == "hello"));

        // CLI server-path defaults must stay aligned with `loom serve`.
        assert!(!opts.verbose);
        assert!(!opts.output_json);
        assert!(!opts.dry_run);
        assert!(!opts.debug_llm);
        assert!(!opts.force_compact);
        assert!(!opts.worktree);
        assert!(!opts.goal_mode);

        // The HTTP bridge must never override provider credentials directly.
        assert_eq!(opts.api_key, None);
        assert_eq!(opts.base_url, None);
        assert_eq!(opts.provider_type, None);
        assert_eq!(opts.tier, None);
        assert_eq!(opts.effort, None);
        assert_eq!(opts.mcp_config_path, None);
    }

    #[test]
    fn explicit_model_and_agent_are_preserved() {
        let opts = options_for("sess-2", "hi", Some("openai/gpt-4o"), Some("reviewer"));
        assert_eq!(opts.agent.as_deref(), Some("reviewer"));
        assert_eq!(opts.model.as_deref(), Some("openai/gpt-4o"));
        assert_eq!(opts.provider, None);
    }

    #[test]
    fn blank_model_is_filtered_out() {
        let opts = options_for("sess-3", "hi", Some("   "), Some("reviewer"));
        assert_eq!(
            opts.model.as_deref(),
            Some(config::default_model().as_str())
        );
        assert_eq!(opts.provider, config::default_provider_name());
        assert_eq!(opts.agent.as_deref(), Some("reviewer"));
    }

    #[test]
    fn blank_agent_falls_back_to_default() {
        let opts = options_for("sess-4", "hi", None, Some("   "));
        assert_eq!(opts.agent.as_deref(), Some(DEFAULT_AGENT_NAME));
        assert_eq!(
            opts.model.as_deref(),
            Some(config::default_model().as_str())
        );
    }
}
