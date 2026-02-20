//! Handle `ToolsList` and `ToolShow` requests.

use loom::{
    build_helve_config, build_react_run_context, ErrorResponse, RunOptions, ServerResponse,
    ToolShowOutput, ToolShowResponse, ToolsListResponse,
};
use std::path::PathBuf;

pub(crate) async fn handle_tools_list(r: loom::ToolsListRequest) -> ServerResponse {
    let id = r.id.clone();
    let opts = RunOptions {
        message: String::new(),
        working_folder: r.working_folder.as_ref().map(PathBuf::from),
        thread_id: r.thread_id.clone(),
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
    };
    let (_helve, config) = build_helve_config(&opts);
    match build_react_run_context(&config).await {
        Ok(ctx) => match ctx.tool_source.list_tools().await {
            Ok(tools) => ServerResponse::ToolsList(ToolsListResponse { id, tools }),
            Err(e) => ServerResponse::Error(ErrorResponse {
                id: Some(id),
                error: e.to_string(),
            }),
        },
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

pub(crate) async fn handle_tool_show(r: loom::ToolShowRequest) -> ServerResponse {
    let id = r.id.clone();
    let opts = RunOptions {
        message: String::new(),
        working_folder: r.working_folder.as_ref().map(PathBuf::from),
        thread_id: r.thread_id.clone(),
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
    };
    let (_helve, config) = build_helve_config(&opts);
    match build_react_run_context(&config).await {
        Ok(ctx) => match ctx.tool_source.list_tools().await {
            Ok(tools) => {
                let spec = tools.into_iter().find(|s| s.name == r.name);
                match spec {
                    Some(s) => {
                        let (tool, tool_yaml) = match r.output.as_ref() {
                            Some(ToolShowOutput::Yaml) => (
                                None,
                                Some(serde_yaml::to_string(&serde_json::json!({
                                    "name": s.name,
                                    "description": s.description,
                                    "input_schema": s.input_schema
                                })).unwrap_or_default()),
                            ),
                            _ => (
                                Some(serde_json::json!({
                                    "name": s.name,
                                    "description": s.description,
                                    "input_schema": s.input_schema
                                })),
                                None,
                            ),
                        };
                        ServerResponse::ToolShow(ToolShowResponse {
                            id,
                            tool,
                            tool_yaml,
                        })
                    }
                    None => ServerResponse::Error(ErrorResponse {
                        id: Some(id),
                        error: format!("tool not found: {}", r.name),
                    }),
                }
            }
            Err(e) => ServerResponse::Error(ErrorResponse {
                id: Some(id),
                error: e.to_string(),
            }),
        },
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn first_tool_name() -> String {
        match handle_tools_list(loom::ToolsListRequest {
            id: "lookup".to_string(),
            working_folder: None,
            thread_id: None,
        })
        .await
        {
            ServerResponse::ToolsList(r) => r
                .tools
                .first()
                .map(|t| t.name.clone())
                .expect("at least one tool"),
            other => panic!("expected ToolsList response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_tools_list_returns_non_empty_tools() {
        let resp = handle_tools_list(loom::ToolsListRequest {
            id: "t1".to_string(),
            working_folder: None,
            thread_id: None,
        })
        .await;
        match resp {
            ServerResponse::ToolsList(r) => {
                assert_eq!(r.id, "t1");
                assert!(!r.tools.is_empty());
            }
            other => panic!("expected ToolsList response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_tool_show_json_returns_tool_object() {
        let tool_name = first_tool_name().await;
        let resp = handle_tool_show(loom::ToolShowRequest {
            id: "s1".to_string(),
            name: tool_name,
            output: Some(loom::ToolShowOutput::Json),
            working_folder: None,
            thread_id: None,
        })
        .await;
        match resp {
            ServerResponse::ToolShow(r) => {
                assert_eq!(r.id, "s1");
                assert!(r.tool.is_some());
                assert!(r.tool_yaml.is_none());
            }
            other => panic!("expected ToolShow response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_tool_show_yaml_returns_yaml_text() {
        let tool_name = first_tool_name().await;
        let resp = handle_tool_show(loom::ToolShowRequest {
            id: "s2".to_string(),
            name: tool_name,
            output: Some(loom::ToolShowOutput::Yaml),
            working_folder: None,
            thread_id: None,
        })
        .await;
        match resp {
            ServerResponse::ToolShow(r) => {
                assert_eq!(r.id, "s2");
                assert!(r.tool.is_none());
                let yaml = r.tool_yaml.unwrap_or_default();
                assert!(yaml.contains("name"));
                assert!(yaml.contains("description"));
            }
            other => panic!("expected ToolShow response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_tool_show_missing_returns_error() {
        let resp = handle_tool_show(loom::ToolShowRequest {
            id: "s3".to_string(),
            name: "no_such_tool".to_string(),
            output: None,
            working_folder: None,
            thread_id: None,
        })
        .await;
        match resp {
            ServerResponse::Error(e) => {
                assert_eq!(e.id.as_deref(), Some("s3"));
                assert!(e.error.contains("tool not found"));
            }
            other => panic!("expected Error response, got {:?}", other),
        }
    }
}
