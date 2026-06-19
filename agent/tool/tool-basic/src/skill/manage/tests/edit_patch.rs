use super::*;

#[tokio::test]
async fn edit_replaces_full_document() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "test-skill", "initial", "old body");
    let tool = make_tool(storage);

    let updated = make_skill_md("test-skill", "updated desc", "new body content");
    tool.call(json!({"action": "edit", "name": "test-skill", "content": updated}), None)
        .await
        .unwrap();

    let reloaded = tool.storage.load("test-skill").unwrap();
    assert_eq!(reloaded.description, "updated desc");
    assert_eq!(reloaded.body, "new body content");
}

#[tokio::test]
async fn patch_succeeds_unique() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "test-skill", "desc", "old text here");
    let tool = make_tool(storage);

    tool.call(
        json!({"action": "patch", "name": "test-skill", "old_string": "old text", "new_string": "new text"}),
        None,
    )
    .await
    .unwrap();

    let reloaded = tool.storage.load("test-skill").unwrap();
    assert!(reloaded.raw.contains("new text"));
}

#[tokio::test]
async fn patch_reverts_on_validation_failure() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "test-skill", "desc", "harmless body");
    let tool = make_tool(storage);

    let result = tool
        .call(
            json!({"action": "patch", "name": "test-skill", "old_string": "harmless body", "new_string": "rm -rf / evil body"}),
            None,
        )
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("patch not applied"));

    let reloaded = tool.storage.load("test-skill").unwrap();
    assert!(reloaded.body.contains("harmless"));
}
