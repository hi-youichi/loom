//! Apply-patch tool: apply opencode-style multi-file patches (Add/Update/Delete/Move).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::edit_file::replace as edit_replace;
use super::path::resolve_path_under;

/// Tool name for apply_patch.
pub const TOOL_APPLY_PATCH: &str = "apply_patch";

#[derive(Debug)]
enum Hunk {
    Add { path: String, contents: String },
    Delete { path: String },
    Update {
        path: String,
        move_path: Option<String>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug)]
struct UpdateChunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

fn parse_patch(patch_text: &str) -> Result<Vec<Hunk>, String> {
    let s = patch_text.trim();
    let begin = "*** Begin Patch";
    let end = "*** End Patch";
    let start = s.find(begin).ok_or("missing *** Begin Patch")?;
    let end_pos = s[start..].find(end).ok_or("missing *** End Patch")?;
    let body = s[start + begin.len()..start + end_pos].trim();
    let lines: Vec<&str> = body.split('\n').collect();
    let mut hunks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("*** Add File:") {
            let path = line["*** Add File:".len()..].trim().to_string();
            if path.is_empty() {
                i += 1;
                continue;
            }
            i += 1;
            let mut contents = String::new();
            while i < lines.len() && !lines[i].trim().starts_with("***") {
                if lines[i].starts_with('+') {
                    contents.push_str(&lines[i][1..]);
                    contents.push('\n');
                }
                i += 1;
            }
            if contents.ends_with('\n') {
                contents.pop();
            }
            hunks.push(Hunk::Add { path, contents });
        } else if line.starts_with("*** Delete File:") {
            let path = line["*** Delete File:".len()..].trim().to_string();
            if !path.is_empty() {
                hunks.push(Hunk::Delete { path });
            }
            i += 1;
        } else if line.starts_with("*** Update File:") {
            let path = line["*** Update File:".len()..].trim().to_string();
            if path.is_empty() {
                i += 1;
                continue;
            }
            i += 1;
            let mut move_path = None;
            if i < lines.len() && lines[i].trim().starts_with("*** Move to:") {
                move_path = Some(lines[i].trim()["*** Move to:".len()..].trim().to_string());
                i += 1;
            }
            let mut chunks = Vec::new();
            while i < lines.len() && !lines[i].trim().starts_with("***") {
                if lines[i].trim().starts_with("@@") {
                    i += 1;
                    let mut old_lines = Vec::new();
                    let mut new_lines = Vec::new();
                    while i < lines.len() && !lines[i].trim().starts_with("@@") && !lines[i].trim().starts_with("***") {
                        let l = lines[i];
                        if l == "*** End of File" {
                            i += 1;
                            break;
                        }
                        if l.starts_with(' ') {
                            let content = l[1..].to_string();
                            old_lines.push(content.clone());
                            new_lines.push(content);
                        } else if l.starts_with('-') {
                            old_lines.push(l[1..].to_string());
                        } else if l.starts_with('+') {
                            new_lines.push(l[1..].to_string());
                        }
                        i += 1;
                    }
                    chunks.push(UpdateChunk {
                        old_lines,
                        new_lines,
                    });
                } else {
                    i += 1;
                }
            }
            hunks.push(Hunk::Update {
                path,
                move_path,
                chunks,
            });
        } else {
            i += 1;
        }
    }
    Ok(hunks)
}

/// Tool that applies a patch (Add/Update/Delete/Move) under the working folder.
pub struct ApplyPatchTool {
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl ApplyPatchTool {
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        TOOL_APPLY_PATCH
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_APPLY_PATCH.to_string(),
            description: Some(
                "Apply a multi-file patch. Use *** Begin Patch / *** End Patch; *** Add File: path (then + lines); \
                 *** Delete File: path; *** Update File: path (optional *** Move to: path) with @@ chunks (space/-/+)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "patchText": {
                        "type": "string",
                        "description": "Full patch text in opencode format."
                    }
                },
                "required": ["patchText"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let patch_text = args
            .get("patchText")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing patchText".to_string()))?;

        let hunks = parse_patch(patch_text).map_err(ToolSourceError::InvalidInput)?;
        if hunks.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "patch has no hunks or invalid format".to_string(),
            ));
        }

        let mut applied = 0;
        for hunk in hunks {
            match hunk {
                Hunk::Add { path, contents } => {
                    let p = resolve_path_under(self.working_folder.as_ref(), &path)?;
                    if let Some(parent) = p.parent() {
                        if !parent.exists() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                ToolSourceError::Transport(format!("create_dir_all: {}", e))
                            })?;
                        }
                    }
                    std::fs::write(&p, contents).map_err(|e| {
                        ToolSourceError::Transport(format!("write {}: {}", p.display(), e))
                    })?;
                    applied += 1;
                }
                Hunk::Delete { path } => {
                    let p = resolve_path_under(self.working_folder.as_ref(), &path)?;
                    if p.exists() {
                        if p.is_dir() {
                            std::fs::remove_dir_all(&p).map_err(|e| {
                                ToolSourceError::Transport(format!("remove_dir {}: {}", p.display(), e))
                            })?;
                        } else {
                            std::fs::remove_file(&p).map_err(|e| {
                                ToolSourceError::Transport(format!("remove_file {}: {}", p.display(), e))
                            })?;
                        }
                        applied += 1;
                    }
                }
                Hunk::Update {
                    path,
                    move_path,
                    chunks,
                } => {
                    let p = resolve_path_under(self.working_folder.as_ref(), &path)?;
                    if !p.exists() || p.is_dir() {
                        return Err(ToolSourceError::InvalidInput(format!(
                            "update target not a file: {}",
                            p.display()
                        )));
                    }
                    let mut content = std::fs::read_to_string(&p).map_err(|e| {
                        ToolSourceError::Transport(format!("read {}: {}", p.display(), e))
                    })?;
                    for chunk in chunks {
                        let old_s = chunk.old_lines.join("\n");
                        let new_s = chunk.new_lines.join("\n");
                        if old_s.is_empty() {
                            if !new_s.is_empty() {
                                content.push('\n');
                                content.push_str(&new_s);
                            }
                        } else {
                            content = edit_replace(&content, &old_s, &new_s, false)
                                .map_err(ToolSourceError::InvalidInput)?;
                        }
                    }
                    std::fs::write(&p, &content).map_err(|e| {
                        ToolSourceError::Transport(format!("write {}: {}", p.display(), e))
                    })?;
                    if let Some(move_to) = move_path {
                        let dest = resolve_path_under(self.working_folder.as_ref(), &move_to)?;
                        if let Some(parent) = dest.parent() {
                            if !parent.exists() {
                                std::fs::create_dir_all(parent).map_err(|e| {
                                    ToolSourceError::Transport(format!("create_dir_all: {}", e))
                                })?;
                            }
                        }
                        std::fs::rename(&p, &dest).map_err(|e| {
                            ToolSourceError::Transport(format!("rename to {}: {}", dest.display(), e))
                        })?;
                    }
                    applied += 1;
                }
            }
        }

        Ok(ToolCallContent {
            text: format!("Applied {} hunk(s) successfully.", applied),
        })
    }
}
