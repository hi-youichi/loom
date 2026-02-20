//! Handle `ToolsList` and `ToolShow` requests.

use loom::{
    build_helve_config, build_react_run_context, ErrorResponse, RunOptions, ServerResponse,
    ToolShowOutput, ToolShowResponse, ToolsListResponse,
};

pub(crate) async fn handle_tools_list(r: loom::ToolsListRequest) -> ServerResponse {
    let id = r.id.clone();
    let opts = RunOptions {
        message: String::new(),
        working_folder: None,
        thread_id: None,
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
        working_folder: None,
        thread_id: None,
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
