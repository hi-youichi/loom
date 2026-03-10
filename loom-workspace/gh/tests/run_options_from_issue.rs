//! L1: IssuesEvent → RunOptions conversion (see gh-webhook-loom-agent-test-plan).

use gh::{parse_issues_event, run_options_from_issues_event};

const ISSUES_PAYLOAD: &str = r#"{
  "action": "opened",
  "repository": {
    "id": 1,
    "name": "repo",
    "full_name": "owner/repo",
    "private": false
  },
  "issue": {
    "id": 1,
    "number": 42,
    "title": "Test issue",
    "body": null,
    "state": "open",
    "html_url": "https://github.com/owner/repo/issues/42",
    "labels": []
  }
}"#;

#[test]
fn run_options_message_contains_repo_and_issue() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    let opts = run_options_from_issues_event(&ev, None);
    assert!(
        opts.message.contains("owner/repo"),
        "message should contain repo: {}",
        opts.message
    );
    assert!(
        opts.message.contains("#42") || opts.message.contains("42"),
        "message should contain issue number: {}",
        opts.message
    );
    assert!(
        opts.message.contains("Test issue"),
        "message should contain title: {}",
        opts.message
    );
}

#[test]
fn run_options_thread_id_from_delivery_id() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    let opts = run_options_from_issues_event(&ev, Some("abc-123"));
    assert_eq!(opts.thread_id.as_deref(), Some("abc-123"));
}

#[test]
fn run_options_thread_id_fallback_without_delivery() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    let opts = run_options_from_issues_event(&ev, None);
    assert_eq!(
        opts.thread_id.as_deref(),
        Some("issue-owner/repo-42"),
        "thread_id fallback format"
    );
}

#[test]
fn run_options_other_fields_sane() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    let opts = run_options_from_issues_event(&ev, None);
    assert!(!opts.verbose);
    assert!(!opts.got_adaptive);
    assert_eq!(opts.display_max_len, 120);
    assert!(!opts.output_json);
    assert!(opts.role_file.is_none());
    assert!(opts.mcp_config_path.is_none());
}

#[test]
fn run_options_working_folder_tied_to_repo() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    // Current impl: working_folder from env WORKING_FOLDER only.
    let prev = std::env::var("WORKING_FOLDER").ok();
    std::env::remove_var("WORKING_FOLDER");
    let opts = run_options_from_issues_event(&ev, None);
    assert!(opts.working_folder.is_none(), "no env => no working_folder");
    std::env::set_var("WORKING_FOLDER", "/tmp/test-repo");
    let opts2 = run_options_from_issues_event(&ev, None);
    assert_eq!(
        opts2.working_folder.as_deref().map(|p| p.display().to_string()),
        Some("/tmp/test-repo".to_string())
    );
    if let Some(p) = prev {
        std::env::set_var("WORKING_FOLDER", p);
    } else {
        std::env::remove_var("WORKING_FOLDER");
    }
}

#[test]
fn run_options_different_actions() {
    let closed = r#"{"action":"closed","repository":{"id":1,"name":"r","full_name":"o/r","private":false},"issue":{"id":1,"number":2,"title":"T","body":null,"state":"closed","html_url":"https://x","labels":[]}}"#;
    let edited = r#"{"action":"edited","repository":{"id":1,"name":"r","full_name":"o/r","private":false},"issue":{"id":1,"number":2,"title":"T2","body":null,"state":"open","html_url":"https://x","labels":[]}}"#;
    let ev_closed = parse_issues_event(closed.as_bytes()).unwrap();
    let ev_edited = parse_issues_event(edited.as_bytes()).unwrap();
    let opts_closed = run_options_from_issues_event(&ev_closed, None);
    let opts_edited = run_options_from_issues_event(&ev_edited, None);
    assert!(opts_closed.message.contains("closed"));
    assert!(opts_edited.message.contains("edited"));
    assert!(opts_edited.message.contains("T2"));
}

#[test]
fn run_options_message_includes_body_when_present() {
    let with_body = r#"{"action":"opened","repository":{"id":1,"name":"r","full_name":"o/r","private":false},"issue":{"id":1,"number":1,"title":"Title","body":"Issue body here.","state":"open","html_url":"https://x","labels":[]}}"#;
    let ev = parse_issues_event(with_body.as_bytes()).unwrap();
    let opts = run_options_from_issues_event(&ev, None);
    assert!(opts.message.contains("Issue body here."));
}
