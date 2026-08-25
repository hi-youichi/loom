use super::*;
use skill::usage::SkillUsageStore;

#[tokio::test]
async fn delete_succeeds_with_agent_created() {
    let (dir, storage) = make_storage();
    save_skill(&storage, "to-delete", "d", "x");

    let usage = Arc::new(SkillUsageStore::new(dir.path().join("usage").as_path()));
    usage.mark_agent_created("to-delete");
    let tool = SkillManagerTool::for_background_review(storage.clone(), Some(usage));

    tool.call(json!({"action": "delete", "name": "to-delete"}), None)
        .await
        .unwrap();
    assert!(tool.storage.load("to-delete").is_err());
}

#[tokio::test]
async fn delete_refuses_non_agent_created() {
    let (dir, storage) = make_storage();
    save_skill(&storage, "user-skill", "d", "x");

    let usage = Arc::new(SkillUsageStore::new(dir.path().join("usage").as_path()));
    let tool = SkillManagerTool::for_background_review(storage, Some(usage));

    let result = tool
        .call(json!({"action": "delete", "name": "user-skill"}), None)
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("not agent-created"));
}

#[tokio::test]
async fn delete_absorbed_into_existing() {
    let (dir, storage) = make_storage();
    save_skill(&storage, "umbrella", "d", "x");
    save_skill(&storage, "to-merge", "d", "x");

    let usage = Arc::new(SkillUsageStore::new(dir.path().join("usage").as_path()));
    usage.mark_agent_created("to-merge");
    let tool = SkillManagerTool::for_background_review(storage, Some(usage));

    let result = tool
        .call(
            json!({"action": "delete", "name": "to-merge", "absorbed_into": "umbrella"}),
            None,
        )
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("Content absorbed into 'umbrella'"));
}

#[tokio::test]
async fn delete_absorbed_into_self_fails() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "self", "d", "x");
    let tool = make_tool(storage);

    let result = tool
        .call(
            json!({"action": "delete", "name": "self", "absorbed_into": "self"}),
            None,
        )
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("cannot equal"));
}

#[tokio::test]
async fn delete_absorbed_into_nonexistent_fails() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "orphan", "d", "x");
    let tool = make_tool(storage);

    let result = tool
        .call(
            json!({"action": "delete", "name": "orphan", "absorbed_into": "does-not-exist"}),
            None,
        )
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("does not exist"));
}
