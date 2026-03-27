//! Client method implementations for ACP.
//!
//! This module implements the Agent -> Client methods defined in ACP protocol.
//! These allow the agent to use client capabilities like:
//! - fs/read_text_file - Read files (including unsaved edits from IDE)
//! - fs/write_text_file - Write/create files
//! - terminal/* - Terminal operations (TODO)
//!
//! Usage: When the agent has an AgentSideConnection (implementing Client trait),
//! it can call these methods to request operations from the IDE.

use agent_client_protocol::{Client, ReadTextFileRequest, SessionId, WriteTextFileRequest};
use tracing::{debug, info};

/// Read a text file from the client (including unsaved edits).
///
/// # Arguments
/// * `client` - The client connection implementing the Client trait
/// * `session_id` - The ACP session this request belongs to
/// * `path` - The file path to read
/// * `line` - Optional line number to start reading from (1-based)
/// * `limit` - Optional maximum number of lines to read
///
/// # Returns
/// * `Ok(String)` - The file contents
/// * `Err(agent_client_protocol::Error)` - If the read fails
///
/// # Usage
/// ```ignore
/// let contents = client_read_text_file(&client, session_id, "src/main.rs", None, None).await?;
/// // Read lines 10-60 (50 lines starting from line 10)
/// let contents = client_read_text_file(&client, session_id, "src/main.rs", Some(10), Some(50)).await?;
/// ```
pub async fn client_read_text_file<C: Client>(
    client: &C,
    session_id: SessionId,
    path: &str,
    line: Option<u32>,
    limit: Option<u32>,
) -> Result<String, agent_client_protocol::Error> {
    let mut request = ReadTextFileRequest::new(session_id, path);
    
    if let Some(l) = line {
        request = request.line(l);
    }
    if let Some(lim) = limit {
        request = request.limit(lim);
    }
    
    debug!(path = %path, line = ?line, limit = ?limit, "Reading file from client");
    
    let response = client.read_text_file(request).await?;
    
    info!(path = %path, length = response.content.len(), "Successfully read file from client");
    
    Ok(response.content)
}

/// Write a text file to the client.
///
/// # Arguments
/// * `client` - The client connection implementing the Client trait
/// * `session_id` - The ACP session this request belongs to
/// * `path` - The file path to write
/// * `contents` - The contents to write
///
/// # Returns
/// * `Ok(())` - If the write succeeds
/// * `Err(agent_client_protocol::Error)` - If the write fails
///
/// # Usage
/// ```ignore
/// client_write_text_file(&client, session_id, "src/main.rs", "fn main() {}").await?;
/// ```
pub async fn client_write_text_file<C: Client>(
    client: &C,
    session_id: SessionId,
    path: &str,
    contents: &str,
) -> Result<(), agent_client_protocol::Error> {
    let request = WriteTextFileRequest::new(session_id, path, contents);
    
    debug!(path = %path, length = contents.len(), "Writing file to client");
    
    client.write_text_file(request).await?;
    
    info!(path = %path, length = contents.len(), "Successfully wrote file to client");
    
    Ok(())
}

/// Check if a client supports file operations.
///
/// This should be used before calling client_read_text_file or client_write_text_file
/// to determine if the client supports these operations.
///
/// # Arguments
/// * `can_read` - Whether fs/read_text_file is supported (from client capabilities)
/// * `can_write` - Whether fs/write_text_file is supported (from client capabilities)
///
/// # Returns
/// * `true` - If the client supports at least one file operation
/// * `false` - If the client supports neither operation
pub fn client_supports_file_operations(can_read: bool, can_write: bool) -> bool {
    can_read || can_write
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_supports_file_operations() {
        // Both supported
        assert!(client_supports_file_operations(true, true));
        
        // Only read supported
        assert!(client_supports_file_operations(true, false));
        
        // Only write supported
        assert!(client_supports_file_operations(false, true));
        
        // Neither supported
        assert!(!client_supports_file_operations(false, false));
    }
}
