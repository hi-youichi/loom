//! Model-free wire regression for the SessionIndex list contracts.
//!
//! This intentionally does not call `session/prompt`: prompt model resolution
//! is an unrelated external dependency and must not gate compatibility
//! evidence for session creation, canonical listing, or the legacy alias.

#[path = "e2e/common/mod.rs"]
mod common;

use common::{with_loom_home, AcpTestHarness, TestEnv};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn session_list_contracts_work_without_model_resolution() {
    let env = TestEnv::setup();

    with_loom_home(&env, async {
        // No prompt is sent, so a deliberately unreachable endpoint keeps
        // this test independent of both model discovery and a mock-server
        // background task.
        let h = AcpTestHarness::spawn(&env, "http://127.0.0.1:9").await;
        let init = h
            .request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientInfo": {"name": "session-list-wire", "version": "0.1.0"},
                    "capabilities": {"session": {"list": {}}}
                }),
            )
            .await;
        assert!(init["agentCapabilities"]["sessionCapabilities"]["list"].is_object());
        if std::env::var("LOOM_SESSION_LIST_EXPECT_LEGACY").as_deref() != Ok("1") {
            let methods = init["agentCapabilities"]["_meta"]["loomdesk.dev"]["session"]["methods"]
                .as_array()
                .expect("new Loom must advertise session extension methods");
            assert!(methods.iter().any(|method| method == "list"));
            assert!(methods.iter().any(|method| method == "list-global"));
        }

        let root = h
            .request(
                "session/new",
                json!({
                    "cwd": env.cwd.to_string_lossy(),
                    "mcpServers": [],
                    "_meta": {"loomdesk.dev": {"title": "Root", "metadata": {"kind": "root"}}}
                }),
            )
            .await;
        let root_id = root["sessionId"]
            .as_str()
            .expect("root session id")
            .to_owned();
        let child = h
            .request(
                "session/new",
                json!({
                    "cwd": env.cwd.to_string_lossy(),
                    "mcpServers": [],
                    "_meta": {"loomdesk.dev": {
                        "parentSessionId": root_id,
                        "title": "Child",
                        "metadata": {"kind": "child"}
                    }}
                }),
            )
            .await;
        let child_id = child["sessionId"]
            .as_str()
            .expect("child session id")
            .to_owned();
        let _ = h.drain_notifications().await;

        // A compatibility run against an older Loom intentionally exercises
        // only the legacy projection. The new canonical assertions below
        // must not turn a valid old-peer fallback into a false failure.
        if std::env::var("LOOM_SESSION_LIST_EXPECT_LEGACY").as_deref() == Ok("1") {
            let legacy = h
                .request(
                    "_loomdesk.dev/session/list-global",
                    json!({
                        "archived": false,
                        "directory": env.cwd.to_string_lossy(),
                        "limit": 10
                    }),
                )
                .await;
            let sessions = legacy["sessions"].as_array().expect("legacy sessions");
            let ids = sessions
                .iter()
                .filter_map(|item| item["sessionId"].as_str())
                .collect::<Vec<_>>();
            assert!(ids.contains(&root_id.as_str()));
            assert!(ids.contains(&child_id.as_str()));
            assert!(sessions.iter().all(|item| item.get("revision").is_none()));

            let standard = h
                .request("session/list", json!({"cwd": env.cwd.to_string_lossy()}))
                .await;
            assert!(standard["sessions"]
                .as_array()
                .expect("standard sessions")
                .iter()
                .any(|item| item["sessionId"] == root_id));
            assert!(standard["nextCursor"].is_null());
            let status = h.shutdown().await;
            assert!(status.success(), "loom-acp exited non-zero: {status:?}");
            return;
        }

        let first = h
            .request(
                "_loomdesk.dev/session/list",
                json!({"archived": "all", "directory": env.cwd.to_string_lossy(), "limit": 1}),
            )
            .await;
        assert_eq!(first["sessions"].as_array().map(Vec::len), Some(1));
        let snapshot_version = first["snapshotVersion"].as_u64().expect("snapshotVersion");
        let cursor = first["nextCursor"]
            .as_str()
            .expect("second page cursor")
            .to_owned();
        assert!(first["sessions"][0].get("revision").is_some());
        assert!(first["sessions"][0].get("indexVersion").is_some());

        let second = h
            .request(
                "_loomdesk.dev/session/list",
                json!({"cursor": cursor, "limit": 1}),
            )
            .await;
        assert_eq!(second["snapshotVersion"].as_u64(), Some(snapshot_version));
        assert_eq!(second["sessions"].as_array().map(Vec::len), Some(1));
        let ids = [
            first["sessions"][0]["sessionId"].as_str().unwrap(),
            second["sessions"][0]["sessionId"].as_str().unwrap(),
        ];
        assert!(ids.contains(&root_id.as_str()));
        assert!(ids.contains(&child_id.as_str()));
        assert_ne!(ids[0], ids[1], "snapshot pages must not duplicate records");

        let legacy = h
            .request(
                "_loomdesk.dev/session/list-global",
                json!({"archived": false, "directory": env.cwd.to_string_lossy(), "limit": 10}),
            )
            .await;
        let legacy_sessions = legacy["sessions"].as_array().expect("legacy sessions");
        assert_eq!(legacy_sessions.len(), 2);
        assert!(legacy_sessions
            .iter()
            .all(|item| item.get("revision").is_none()));
        assert!(legacy_sessions
            .iter()
            .all(|item| item.get("indexVersion").is_none()));

        let metrics = h
            .request("_loomdesk.dev/session-metrics/status", json!({}))
            .await;
        assert!(metrics["legacyListGlobalCalls"]
            .as_u64()
            .is_some_and(|calls| calls >= 1));

        let standard = h
            .request("session/list", json!({"cwd": env.cwd.to_string_lossy()}))
            .await;
        let standard_sessions = standard["sessions"].as_array().expect("standard sessions");
        assert!(standard_sessions
            .iter()
            .any(|item| item["sessionId"] == root_id));
        assert!(standard_sessions
            .iter()
            .any(|item| item["sessionId"] == child_id));
        assert!(standard["nextCursor"].is_null());

        let archived = h
            .request(
                "_loomdesk.dev/session/archive",
                json!({"sessionId": child_id, "archived": true}),
            )
            .await;
        assert_eq!(archived["session"]["sessionId"], child_id);
        assert!(archived["session"]["archivedAt"].is_string());

        let archived_list = h
            .request(
                "_loomdesk.dev/session/list",
                json!({
                    "archived": "archived",
                    "directory": env.cwd.to_string_lossy(),
                    "limit": 10
                }),
            )
            .await;
        assert!(archived_list["sessions"]
            .as_array()
            .expect("archived sessions")
            .iter()
            .any(|item| item["sessionId"] == child_id));

        let deleted = h
            .request(
                "_loomdesk.dev/session/delete",
                json!({"sessionId": child_id}),
            )
            .await;
        assert_eq!(deleted["tombstone"]["sessionId"], child_id);
        assert!(deleted["tombstone"]["revision"].as_u64().is_some());

        let after_delete = h
            .request(
                "_loomdesk.dev/session/list",
                json!({
                    "archived": "all",
                    "directory": env.cwd.to_string_lossy(),
                    "limit": 10
                }),
            )
            .await;
        assert!(!after_delete["sessions"]
            .as_array()
            .expect("post-delete sessions")
            .iter()
            .any(|item| item["sessionId"] == child_id));

        let status = h.shutdown().await;
        assert!(status.success(), "loom-acp exited non-zero: {status:?}");
    })
    .await;
}
