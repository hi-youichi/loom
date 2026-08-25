use super::*;

#[tokio::test]
async fn write_file_succeeds() {
    let (dir, storage) = make_storage();
    save_skill(&storage, "my-skill", "d", "x");
    let tool = make_tool(storage);

    tool.call(
        json!({"action": "write_file", "name": "my-skill", "file_path": "scripts/setup.sh", "file_content": "#!/bin/bash\necho hi\n"}),
        None,
    )
    .await
    .unwrap();

    let script_path = dir
        .path()
        .join("auto")
        .join("my-skill")
        .join("scripts")
        .join("setup.sh");
    assert!(script_path.exists());
    assert!(std::fs::read_to_string(&script_path)
        .unwrap()
        .contains("echo hi"));
}

#[tokio::test]
async fn write_file_rejects_traversal() {
    let (_dir, storage) = make_storage();
    save_skill(&storage, "my-skill", "d", "x");
    let tool = make_tool(storage);

    let result = tool
        .call(
            json!({"action": "write_file", "name": "my-skill", "file_path": "../etc/passwd", "file_content": "evil"}),
            None,
        )
        .await
        .unwrap();
    let text = match result {
        ToolCallContent::Text(t) => t,
        _ => panic!("expected Text"),
    };
    assert!(text.contains("Path validation failed"));
}

#[tokio::test]
async fn remove_file_succeeds() {
    let (dir, storage) = make_storage();
    save_skill(&storage, "my-skill", "d", "x");
    storage.write_file("my-skill", "scripts/a.sh", "x").unwrap();
    let tool = make_tool(storage);

    let script_path = dir
        .path()
        .join("auto")
        .join("my-skill")
        .join("scripts")
        .join("a.sh");
    assert!(script_path.exists());

    tool.call(
        json!({"action": "remove_file", "name": "my-skill", "file_path": "scripts/a.sh"}),
        None,
    )
    .await
    .unwrap();
    assert!(!script_path.exists());
}
