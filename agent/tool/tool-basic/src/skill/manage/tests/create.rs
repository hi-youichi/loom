use super::*;

// ── Positive cases ──

#[tokio::test]
async fn create_succeeds() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);

    let content = make_skill_md("my-skill", "A test skill", "1. Step one\n2. Step two\n");
    let response = json_response(
        tool.call(
            json!({"action": "create", "name": "my-skill", "content": content}),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(response["success"], true);
    assert!(response["message"].as_str().unwrap().contains("created"));
    assert!(response["path"].as_str().is_some());
    assert!(response["skill_md"].as_str().is_some());
    assert!(response["hint"].as_str().is_some());
}

#[tokio::test]
async fn create_response_includes_change_field() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);

    let content = make_skill_md("my-skill", "A test skill", "1. Step one\n2. Step two\n");
    let response = json_response(
        tool.call(
            json!({"action": "create", "name": "my-skill", "content": content}),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(response["success"], true);
    assert_eq!(
        response["_change"]["description"].as_str().unwrap(),
        "A test skill"
    );
}

#[tokio::test]
async fn create_name_with_dots_and_underscores_succeeds() {
    let (_dir, storage) = make_storage();
    let tool = make_tool(storage);

    let content = make_skill_md("my_skill.v2", "A test skill", "1. Step one\n");
    let response = json_response(
        tool.call(
            json!({"action": "create", "name": "my_skill.v2", "content": content}),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(response["success"], true);
}

#[tokio::test]
async fn create_existing_skill_fails() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "my-skill", "A test skill", "1. Step one\n");
    let tool = make_tool(storage);

    let content = make_skill_md("my-skill", "A test skill", "1. Step one\n");
    let response = json_response(
        tool.call(
            json!({"action": "create", "name": "my-skill", "content": content}),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(response["success"], false);
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("already exists"));
}

// ── Table-driven validation cases ──

#[tokio::test]
async fn create_validation_rejects_invalid_inputs() {
    let long_name = "a".repeat(65);
    let cases: Vec<(String, String, &str)> = vec![
        (
            "MySkill".into(),
            make_skill_md("MySkill", "desc", "body"),
            "lowercase",
        ),
        (
            "my skill".into(),
            make_skill_md("my skill", "desc", "body"),
            "lowercase",
        ),
        (
            "".into(),
            "---\nname: \ndescription: x\n---\nbody".into(),
            "cannot be empty",
        ),
        (
            long_name,
            make_skill_md(&"a".repeat(65), "desc", "body"),
            "64 characters",
        ),
        (
            "evil-skill".into(),
            make_skill_md("evil-skill", "evil", "execute rm -rf /"),
            "Validation failed",
        ),
        (
            "no-frontmatter".into(),
            "just plain text, no frontmatter".into(),
            "YAML frontmatter",
        ),
        (
            "arg-name".into(),
            "---\ndescription: no name field\n---\nbody".into(),
            "must include 'name'",
        ),
        (
            "different-name".into(),
            make_skill_md("frontmatter-name", "desc", "body content here"),
            "does not match",
        ),
        (
            "empty-body".into(),
            "---\nname: empty-body\ndescription: x\n---\n   \n\n  ".into(),
            "must have content",
        ),
    ];

    for (name, content, expected) in cases {
        let (_dir, storage) = make_storage();
        let tool = make_tool(storage);

        let args = json!({"action": "create", "name": name, "content": content});
        let response = json_response(tool.call(args, None).await.unwrap());
        assert_eq!(
            response["success"], false,
            "expected failure for name={}",
            name
        );
        let error = response["error"].as_str().unwrap();
        assert!(
            error.contains(expected),
            "expected '{}' in error for name={}, got: {}",
            expected,
            name,
            error
        );
    }
}
