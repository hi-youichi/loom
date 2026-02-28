//! Handle `Run` request: execute agent (streaming or single reply).
//!
//! Flow: request preparation (register thread, append initial message, build opts/cmd) →
//! spawn run task → consume event stream and send over WebSocket → send RunEnd or Error.

mod delivery;
mod request;
mod stream;

use axum::extract::ws::WebSocket;
use loom::{ProtocolEventEnvelope, ServerResponse};
use request::{PrepareRunInput, PrepareRunResult};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::app::RunConfig;

/// Entry point for a Run request: prepares run (register thread, append initial user
/// message, build options), spawns the agent task, and streams events + final RunEnd/Error
/// over the WebSocket. Returns `Ok(None)` in the normal streaming case (response already
/// sent); returns `Err` if streaming or sending the final response fails.
pub(crate) async fn handle_run(
    r: loom::RunRequest,
    socket: &mut WebSocket,
    workspace_store: Option<Arc<loom_workspace::Store>>,
    user_message_store: Option<Arc<dyn loom::UserMessageStore>>,
    run_config: &RunConfig,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>> {
    let PrepareRunResult {
        opts,
        cmd,
        initial_user_appended,
    } = request::prepare_run(
        r,
        workspace_store.as_ref(),
        user_message_store.as_ref(),
        PrepareRunInput {
            display_max_len: run_config.display_max_len,
        },
    )
    .await;

    let run_id = format!("run-{}", Uuid::new_v4());
    let session_id = run_id.clone();
    let (tx, rx) = mpsc::channel::<ProtocolEventEnvelope>(run_config.event_queue_capacity);
    let opts = opts.clone();
    let cmd = cmd.clone();
    let thread_id_for_append = opts.thread_id.clone();
    let user_message_store_for_append = user_message_store.clone();
    let run_handle = tokio::spawn(stream::run_agent_task(
        session_id,
        tx,
        opts,
        cmd,
        initial_user_appended,
        user_message_store_for_append,
        thread_id_for_append,
        run_config.append_queue_capacity,
    ));

    let mut sender = delivery::WebSocketRunSender(socket);
    delivery::handle_run_stream(run_id, rx, run_handle, &mut sender).await
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use loom::{
        EnvelopeState, ProtocolEvent, ProtocolEventEnvelope, RunCmd, RunError, RunOptions,
        ServerResponse,
    };
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    use super::delivery::{handle_run_stream, RunStreamSender};
    use super::request::{try_append_initial_user_message, try_register_thread_in_workspace};
    use super::stream::{run_agent_task, APPEND_QUEUE_CAPACITY, EVENT_QUEUE_CAPACITY};

    /// Mock sender that can fail on first send or record sent responses.
    struct MockRunStreamSender {
        send_count: usize,
        fail_after: Option<usize>,
        last_run_end: Option<(String, String)>,
        last_error: Option<(Option<String>, String)>,
    }

    #[async_trait]
    impl RunStreamSender for MockRunStreamSender {
        async fn send_response(
            &mut self,
            response: &ServerResponse,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.send_count += 1;
            if let Some(n) = self.fail_after {
                if self.send_count >= n {
                    return Err("mock send failure".into());
                }
            }
            match response {
                ServerResponse::RunEnd(r) => {
                    self.last_run_end = Some((r.id.clone(), r.reply.clone()));
                }
                ServerResponse::Error(e) => {
                    self.last_error = Some((e.id.clone(), e.error.clone()));
                }
                _ => {}
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn handle_run_stream_send_failure_aborts_and_returns_err() {
        let (tx, rx) = mpsc::channel::<ProtocolEventEnvelope>(2);
        let run_handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            (
                Ok("never".to_string()),
                Arc::new(Mutex::new(EnvelopeState::new("s".into()))),
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicUsize::new(0)),
            )
        });
        let env = ProtocolEventEnvelope {
            session_id: Some("run-1".into()),
            node_id: Some("n".into()),
            event_id: Some(1),
            event: ProtocolEvent::NodeEnter { id: "think".to_string() },
        };
        tx.send(env).await.unwrap();
        drop(tx);
        let mut sender = MockRunStreamSender {
            send_count: 0,
            fail_after: Some(1),
            last_run_end: None,
            last_error: None,
        };
        let out = handle_run_stream("run-1".to_string(), rx, run_handle, &mut sender).await;
        assert!(out.is_err());
        assert_eq!(out.unwrap_err().to_string(), "mock send failure");
    }

    #[tokio::test]
    async fn handle_run_stream_agent_ok_sends_run_end() {
        let (_tx, rx) = mpsc::channel::<ProtocolEventEnvelope>(1);
        drop(_tx);
        let state = Arc::new(Mutex::new(EnvelopeState::new("run-1".into())));
        let run_handle = tokio::spawn(async move {
            (
                Ok("reply text".to_string()),
                state,
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicUsize::new(0)),
            )
        });
        let mut sender = MockRunStreamSender {
            send_count: 0,
            fail_after: None,
            last_run_end: None,
            last_error: None,
        };
        let out = handle_run_stream("run-1".to_string(), rx, run_handle, &mut sender).await;
        assert!(out.is_ok());
        assert!(out.unwrap().is_none());
        assert_eq!(sender.send_count, 1);
        let (id, reply) = sender.last_run_end.as_ref().unwrap();
        assert_eq!(id, "run-1");
        assert_eq!(reply, "reply text");
    }

    #[tokio::test]
    async fn handle_run_stream_agent_err_sends_error_response() {
        let (_tx, rx) = mpsc::channel::<ProtocolEventEnvelope>(1);
        drop(_tx);
        let state = Arc::new(Mutex::new(EnvelopeState::new("run-1".into())));
        let run_handle = tokio::spawn(async move {
            (
                Err(RunError::Build(loom::BuildRunnerError::NoLlm)),
                state,
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicUsize::new(0)),
            )
        });
        let mut sender = MockRunStreamSender {
            send_count: 0,
            fail_after: None,
            last_run_end: None,
            last_error: None,
        };
        let out = handle_run_stream("run-1".to_string(), rx, run_handle, &mut sender).await;
        assert!(out.is_ok());
        assert_eq!(sender.send_count, 1);
        let (id, error) = sender.last_error.as_ref().unwrap();
        assert_eq!(id.as_deref(), Some("run-1"));
        assert!(!error.is_empty());
    }

    #[tokio::test]
    async fn handle_run_stream_join_error_returns_err() {
        let (_tx, rx) = mpsc::channel::<ProtocolEventEnvelope>(1);
        drop(_tx);
        let run_handle = tokio::spawn(async move {
            panic!("task panicked");
        });
        let mut sender = MockRunStreamSender {
            send_count: 0,
            fail_after: None,
            last_run_end: None,
            last_error: None,
        };
        let out = handle_run_stream("run-1".to_string(), rx, run_handle, &mut sender).await;
        assert!(out.is_err());
        assert_eq!(sender.send_count, 0);
    }

    #[tokio::test]
    async fn try_register_thread_in_workspace_all_none_no_op() {
        try_register_thread_in_workspace(None, None, None).await;
    }

    #[tokio::test]
    async fn try_register_thread_in_workspace_store_none_returns_early() {
        try_register_thread_in_workspace(None, Some("ws1"), Some("t1")).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn try_register_thread_in_workspace_workspace_id_none_returns_early() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(loom_workspace::Store::new(file.path()).unwrap());
        try_register_thread_in_workspace(Some(&store), None, Some("t1")).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn try_register_thread_in_workspace_thread_id_none_returns_early() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(loom_workspace::Store::new(file.path()).unwrap());
        let ws_id = store.create_workspace(None).await.unwrap();
        try_register_thread_in_workspace(Some(&store), Some(&ws_id), None).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn try_register_thread_in_workspace_all_some_registers() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let store = Arc::new(loom_workspace::Store::new(file.path()).unwrap());
        let ws_id = store.create_workspace(None).await.unwrap();
        try_register_thread_in_workspace(
            Some(&store),
            Some(ws_id.as_str()),
            Some("thread-1"),
        )
        .await;
        let threads = store.list_threads(&ws_id).await.unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].thread_id, "thread-1");
    }

    #[tokio::test]
    async fn try_append_initial_user_message_store_none_returns_false() {
        let got = try_append_initial_user_message(None, Some("t1"), "hi").await;
        assert!(!got);
    }

    #[tokio::test]
    async fn try_append_initial_user_message_thread_id_none_returns_false() {
        let store: Arc<dyn loom::UserMessageStore> = Arc::new(loom::NoOpUserMessageStore);
        let got = try_append_initial_user_message(Some(&store), None, "hi").await;
        assert!(!got);
    }

    #[tokio::test]
    async fn try_append_initial_user_message_both_some_returns_true() {
        let store: Arc<dyn loom::UserMessageStore> = Arc::new(loom::NoOpUserMessageStore);
        let got =
            try_append_initial_user_message(Some(&store), Some("t1"), "hello").await;
        assert!(got);
    }

    #[tokio::test]
    async fn run_agent_task_completes_and_returns_result_and_state() {
        let (tx, _rx) = mpsc::channel::<ProtocolEventEnvelope>(EVENT_QUEUE_CAPACITY);
        let opts = RunOptions {
            message: "ping".to_string(),
            working_folder: None,
            thread_id: None,
            role_file: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: true,
        };
        let (result, state, _dropped_events, _dropped_appends) = run_agent_task(
            "test-session".to_string(),
            tx,
            opts,
            RunCmd::React,
            false,
            None,
            None,
            APPEND_QUEUE_CAPACITY,
        )
        .await;
        let _ = result;
        let guard = state.lock().unwrap();
        assert_eq!(guard.session_id, "test-session");
    }

    #[tokio::test]
    async fn run_agent_task_with_user_message_store_uses_append_channel() {
        let (tx, _rx) = mpsc::channel::<ProtocolEventEnvelope>(EVENT_QUEUE_CAPACITY);
        let store: Arc<dyn loom::UserMessageStore> = Arc::new(loom::NoOpUserMessageStore);
        let opts = RunOptions {
            message: "hi".to_string(),
            working_folder: None,
            thread_id: Some("thread-append".to_string()),
            role_file: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 2000,
            output_json: true,
        };
        let (result, state, _dropped_events, _dropped_appends) = run_agent_task(
            "session-2".to_string(),
            tx,
            opts,
            RunCmd::React,
            true,
            Some(store),
            Some("thread-append".to_string()),
            APPEND_QUEUE_CAPACITY,
        )
        .await;
        let _ = result;
        let guard = state.lock().unwrap();
        assert_eq!(guard.session_id, "session-2");
    }
}
