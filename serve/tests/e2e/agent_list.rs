use super::common;
use futures_util::StreamExt;
use loom::{AgentListRequest, AgentSourceFilter, ClientRequest, ServerResponse};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_agent_list() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "agent-list-1".to_string();
    let req = ClientRequest::AgentList(AgentListRequest {
        id: id.clone(),
        source_filter: None,
        working_folder: None,
        thread_id: None,
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req)
        .await
        .unwrap();

    assert!(
        received.contains("\"type\":\"agent_list\"") && received.contains("\"agents\""),
        "expected agent_list response, received: {}",
        received
    );
    match &resp {
        ServerResponse::AgentList(r) => {
            assert_eq!(r.id, id);
            assert!(r.agents.len() > 0, "expected at least one agent");

            // Check that built-in agents are present
            let agent_names: Vec<&str> = r.agents.iter().map(|a| a.name.as_str()).collect();
            assert!(
                agent_names.contains(&"dev"),
                "expected 'dev' agent to be present"
            );
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        _ => panic!("expected AgentList, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}

#[tokio::test]
async fn e2e_agent_list_with_filter() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "agent-list-filter-1".to_string();
    let req = ClientRequest::AgentList(AgentListRequest {
        id: id.clone(),
        source_filter: Some(AgentSourceFilter::BuiltIn),
        working_folder: None,
        thread_id: None,
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req)
        .await
        .unwrap();

    assert!(
        received.contains("\"type\":\"agent_list\""),
        "expected agent_list response, received: {}",
        received
    );
    match &resp {
        ServerResponse::AgentList(r) => {
            assert_eq!(r.id, id);
            // All agents should be built-in
            for agent in &r.agents {
                assert_eq!(
                    agent.source,
                    loom::AgentSource::BuiltIn,
                    "expected all agents to be built-in when filtered"
                );
            }
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        _ => panic!("expected AgentList, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
