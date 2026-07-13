use super::*;
use skill::provenance::WriteOrigin;

#[tokio::test]
async fn spec_matches_reference_schema() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);
    let spec = tool.spec();
    assert_eq!(spec.name, "skill_manage");

    let actions: Vec<&str> = spec
        .input_schema
        .get("properties")
        .and_then(|p| p.get("action"))
        .and_then(|a| a.get("enum"))
        .and_then(|e| e.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        actions,
        vec![
            "create",
            "patch",
            "edit",
            "delete",
            "write_file",
            "remove_file"
        ]
    );

    let req: Vec<&str> = spec.input_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(req, vec!["action", "name"]);

    let props = spec.input_schema["properties"].as_object().unwrap();
    for key in &[
        "action",
        "name",
        "content",
        "old_string",
        "new_string",
        "replace_all",
        "category",
        "file_path",
        "file_content",
        "absorbed_into",
    ] {
        assert!(props.contains_key(*key), "missing property: {}", key);
    }
    for removed in &["body", "description", "triggers"] {
        assert!(
            !props.contains_key(*removed),
            "should not contain: {}",
            removed
        );
    }
}

#[tokio::test]
async fn unknown_action_returns_error() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);

    let err = tool
        .call(json!({"action": "invalid", "name": "x"}), None)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn missing_action_returns_error() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);

    let err = tool.call(json!({"name": "x"}), None).await.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn write_origin_guard_set_during_call() {
    let (_dir, storage) = make_storage();
    let tool = SkillManagerTool::for_background_review(storage, None);

    assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);

    let content = make_skill_md("guard-test", "x", "x");
    tool.call(
        json!({"action": "create", "name": "guard-test", "content": content}),
        None,
    )
    .await
    .unwrap();

    assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
}
