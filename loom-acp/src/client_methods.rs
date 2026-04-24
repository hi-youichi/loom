use agent_client_protocol::{Client, ConnectionTo};
use agent_client_protocol::schema::{
    CreateTerminalRequest, KillTerminalRequest, ReadTextFileRequest,
    ReleaseTerminalRequest, SessionId, TerminalId, TerminalOutputRequest,
    TerminalExitStatus, EnvVariable, WaitForTerminalExitRequest,
    WriteTextFileRequest,
};
use tracing::{debug, info};

use crate::tools::{TerminalExitResult, TerminalOutput};

pub async fn read_text_file(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    path: &str,
    line: Option<u32>,
    limit: Option<u32>,
) -> Result<String, String> {
    let mut request = ReadTextFileRequest::new(session_id.clone(), path);
    if let Some(l) = line {
        request = request.line(l);
    }
    if let Some(l) = limit {
        request = request.limit(l);
    }

    debug!(?request, "Sending fs/read_text_file request");
    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("fs/read_text_file error: {:?}", e))?;

    info!(
        content_len = response.content.len(),
        "fs/read_text_file completed"
    );
    Ok(response.content)
}

pub async fn write_text_file(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let request = WriteTextFileRequest::new(session_id.clone(), path, content);

    debug!(?request, "Sending fs/write_text_file request");
    conn.send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("fs/write_text_file error: {:?}", e))?;

    info!("fs/write_text_file completed");
    Ok(())
}

pub async fn terminal_create(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    command: &str,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
    output_byte_limit: Option<u64>,
) -> Result<String, String> {
    let mut request = CreateTerminalRequest::new(session_id.clone(), command);

    if !args.is_empty() {
        request = request.args(args);
    }

    if !env.is_empty() {
        request = request.env(
            env.into_iter()
                .map(|(name, value)| EnvVariable::new(name, value))
                .collect(),
        );
    }

    if let Some(dir) = cwd {
        request = request.cwd(std::path::PathBuf::from(dir));
    }

    if let Some(limit) = output_byte_limit {
        request = request.output_byte_limit(limit);
    }

    debug!(?request, "Sending terminal/create request");
    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("terminal/create error: {:?}", e))?;

    let terminal_id = response.terminal_id.to_string();
    info!(terminal_id = %terminal_id, "terminal/create completed");
    Ok(terminal_id)
}

pub async fn terminal_output(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<TerminalOutput, String> {
    let request = TerminalOutputRequest::new(
        session_id.clone(),
        TerminalId::new(terminal_id),
    );

    debug!(?request, "Sending terminal/output request");
    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("terminal/output error: {:?}", e))?;

    let exit_status = response.exit_status.map(|s| {
        TerminalExitStatus::new()
            .exit_code(s.exit_code)
            .signal(s.signal)
    });

    info!(
        output_len = response.output.len(),
        truncated = response.truncated,
        "terminal/output completed"
    );

    Ok(TerminalOutput {
        output: response.output,
        truncated: response.truncated,
        exit_status,
    })
}

pub async fn terminal_wait_for_exit(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<TerminalExitResult, String> {
    let request = WaitForTerminalExitRequest::new(
        session_id.clone(),
        TerminalId::new(terminal_id),
    );

    debug!(?request, "Sending terminal/wait_for_exit request");
    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("terminal/wait_for_exit error: {:?}", e))?;

    let exit_code = response.exit_status.exit_code;
    let signal = response.exit_status.signal;

    info!(
        exit_code = ?exit_code,
        signal = ?signal,
        "terminal/wait_for_exit completed"
    );

    Ok(TerminalExitResult {
        exit_code,
        signal,
    })
}

pub async fn terminal_kill(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<(), String> {
    let request = KillTerminalRequest::new(
        session_id.clone(),
        TerminalId::new(terminal_id),
    );

    debug!(?request, "Sending terminal/kill request");
    conn.send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("terminal/kill error: {:?}", e))?;

    info!("terminal/kill completed");
    Ok(())
}

pub async fn terminal_release(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<(), String> {
    let request = ReleaseTerminalRequest::new(
        session_id.clone(),
        TerminalId::new(terminal_id),
    );

    debug!(?request, "Sending terminal/release request");
    conn.send_request(request)
        .block_task()
        .await
        .map_err(|e| format!("terminal/release error: {:?}", e))?;

    info!("terminal/release completed");
    Ok(())
}

pub fn client_supports_file_operations(read: bool, write: bool) -> bool {
    read || write
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_supports_file_operations() {
        assert!(client_supports_file_operations(true, true));
        assert!(client_supports_file_operations(true, false));
        assert!(client_supports_file_operations(false, true));
        assert!(!client_supports_file_operations(false, false));
    }
}
