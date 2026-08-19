use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_status(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let path = optional_param_str(&params, "path");
    let _ = path;

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let status = loom_git::facade::status(&repo_dir)
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::to_value(status).unwrap_or(Value::Null))
}

pub async fn handle_log(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let limit: Option<usize> = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let cursor: Option<String> = params
        .get("cursor")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let branch: Option<String> = optional_param_str(&params, "branch");
    let file_path: Option<String> = optional_param_str(&params, "filePath");

    let limit = limit.unwrap_or(30);
    let skip = decode_cursor_offset(&cursor);

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let query = loom_git::backend::LogQuery {
        limit,
        skip,
        branch: branch.clone(),
        file_path: file_path.clone(),
    };
    let items = loom_git::facade::log(&repo_dir, &query)
        .await
        .map_err(ext_err_from_git)?;

    let has_more = items.len() >= limit;
    let next_cursor = if has_more {
        encode_cursor_offset(skip + items.len())
    } else {
        None
    };

    Ok(serde_json::json!({
        "items": items,
        "nextCursor": next_cursor,
        "hasMore": has_more,
    }))
}
