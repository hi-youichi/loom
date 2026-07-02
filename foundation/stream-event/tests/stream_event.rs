use stream_event::{to_json, EnvelopeState, ProtocolEvent};

#[test]
fn event_id_monotonic_across_many_events() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    let a = to_json(
        &ProtocolEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
        },
        &mut state,
    )
    .unwrap();
    let b = to_json(
        &ProtocolEvent::Custom {
            value: serde_json::json!({"k":"v"}),
        },
        &mut state,
    )
    .unwrap();
    let c = to_json(
        &ProtocolEvent::Values {
            state: serde_json::json!({"messages": []}),
        },
        &mut state,
    )
    .unwrap();

    assert_eq!(a["event_id"], 1);
    assert_eq!(b["event_id"], 2);
    assert_eq!(c["event_id"], 3);
}

#[test]
fn inject_does_not_overwrite_existing_keys() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    // Pre-existing envelope keys should win.
    let mut v = serde_json::json!({
        "type": "usage",
        "prompt_tokens": 1,
        "completion_tokens": 1,
        "total_tokens": 2,
        "session_id": "existing-sess",
        "node_id": "existing-node",
        "event_id": 999
    });
    state.inject_into(&mut v);

    assert_eq!(v["session_id"], "existing-sess");
    assert_eq!(v["node_id"], "existing-node");
    assert_eq!(v["event_id"], 999);
}

#[test]
fn node_id_defaults_to_run_0_until_first_node_enter() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    let v = to_json(
        &ProtocolEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
        },
        &mut state,
    )
    .unwrap();

    assert_eq!(v["node_id"], "run-0");
    assert_eq!(v["event_id"], 1);
}

#[test]
fn node_enter_sets_node_id_and_increments_node_run_seq() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    // First node span.
    let enter_think = to_json(
        &ProtocolEvent::NodeEnter {
            id: "think".to_string(),
        },
        &mut state,
    )
    .unwrap();

    // Second node span.
    let enter_act = to_json(
        &ProtocolEvent::NodeEnter {
            id: "act".to_string(),
        },
        &mut state,
    )
    .unwrap();

    assert_eq!(enter_think["node_id"], "run-think-0");
    assert_eq!(enter_think["event_id"], 1);

    assert_eq!(enter_act["node_id"], "run-act-1");
    assert_eq!(enter_act["event_id"], 2);
}

#[test]
fn node_id_remains_active_until_next_node_enter() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    let enter_think = to_json(
        &ProtocolEvent::NodeEnter {
            id: "think".to_string(),
        },
        &mut state,
    )
    .unwrap();

    let chunk = to_json(
        &ProtocolEvent::MessageChunk {
            content: "hi".to_string(),
            id: "think".to_string(),
        },
        &mut state,
    )
    .unwrap();

    let exit_think = to_json(
        &ProtocolEvent::NodeExit {
            id: "think".to_string(),
            result: serde_json::json!("Ok"),
        },
        &mut state,
    )
    .unwrap();

    assert_eq!(enter_think["node_id"], "run-think-0");
    assert_eq!(chunk["node_id"], "run-think-0");
    assert_eq!(exit_think["node_id"], "run-think-0");

    assert_eq!(enter_think["event_id"], 1);
    assert_eq!(chunk["event_id"], 2);
    assert_eq!(exit_think["event_id"], 3);
}

#[test]
fn preexisting_session_id_node_id_event_id_also_block_to_json_injection() {
    let mut state = EnvelopeState::new("sess-1".to_string());

    // If the event itself already has envelope keys, injection must not overwrite.
    // (We bypass ProtocolEvent here to simulate a buggy producer.)
    let mut v = serde_json::json!({
        "type": "node_enter",
        "id": "think",
        "session_id": "existing-sess",
        "node_id": "existing-node",
        "event_id": 999
    });

    state.inject_into(&mut v);

    assert_eq!(v["session_id"], "existing-sess");
    assert_eq!(v["node_id"], "existing-node");
    assert_eq!(v["event_id"], 999);
}
