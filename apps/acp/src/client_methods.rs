use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, EnvVariable, KillTerminalRequest, ReadTextFileRequest,
    ReleaseTerminalRequest, SessionId, TerminalExitStatus, TerminalId, TerminalOutputRequest,
    WaitForTerminalExitRequest, WriteTextFileRequest,
};
use agent_client_protocol::{Client, ConnectionTo};
use tracing::{debug, info, instrument};

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
        .map_err(|e| e.to_string())?;

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
        .map_err(|e| e.to_string())?;

    info!("fs/write_text_file completed");
    Ok(())
}

#[instrument(skip(conn), fields(session_id, command, args_count = args.len()))]
pub async fn terminal_create(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    command: &str,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
    output_byte_limit: Option<u64>,
) -> Result<String, String> {
    info!(
        command = %command,
        args_count = args.len(),
        env_count = env.len(),
        cwd = ?cwd,
        output_byte_limit = ?output_byte_limit,
        "sending terminal/create request"
    );

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

    if let Some(ref dir) = cwd {
        request = request.cwd(std::path::PathBuf::from(dir));
    }

    if let Some(limit) = output_byte_limit {
        request = request.output_byte_limit(limit);
    }

    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| e.to_string())?;

    let terminal_id = response.terminal_id.to_string();
    info!(terminal_id = %terminal_id, "terminal/create completed");
    Ok(terminal_id)
}

#[instrument(skip(conn), fields(session_id, terminal_id))]
pub async fn terminal_output(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<TerminalOutput, String> {
    info!(terminal_id = %terminal_id, "sending terminal/output request");

    let request = TerminalOutputRequest::new(session_id.clone(), TerminalId::new(terminal_id));

    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| e.to_string())?;

    info!(
        terminal_id = %terminal_id,
        output_len = response.output.len(),
        truncated = response.truncated,
        exit_code = ?response.exit_status.as_ref().and_then(|s| s.exit_code),
        "terminal/output completed"
    );

    let exit_status = response.exit_status.map(|s| {
        TerminalExitStatus::new()
            .exit_code(s.exit_code)
            .signal(s.signal)
    });

    Ok(TerminalOutput {
        output: response.output,
        truncated: response.truncated,
        exit_status,
    })
}

#[instrument(skip(conn), fields(session_id, terminal_id))]
pub async fn terminal_wait_for_exit(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<TerminalExitResult, String> {
    info!(terminal_id = %terminal_id, "sending terminal/wait_for_exit request");

    let request = WaitForTerminalExitRequest::new(session_id.clone(), TerminalId::new(terminal_id));

    let response = conn
        .send_request(request)
        .block_task()
        .await
        .map_err(|e| e.to_string())?;

    let exit_code = response.exit_status.exit_code;
    let signal = response.exit_status.signal;

    info!(
        terminal_id = %terminal_id,
        exit_code = ?exit_code,
        signal = ?signal,
        "terminal/wait_for_exit completed"
    );

    Ok(TerminalExitResult { exit_code, signal })
}

pub async fn terminal_kill(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<(), String> {
    let request = KillTerminalRequest::new(session_id.clone(), TerminalId::new(terminal_id));

    debug!(?request, "Sending terminal/kill request");

    conn.send_request(request)
        .block_task()
        .await
        .map_err(|e| e.to_string())?;

    info!("terminal/kill completed");
    Ok(())
}

#[instrument(skip(conn), fields(session_id, terminal_id))]
pub async fn terminal_release(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    terminal_id: &str,
) -> Result<(), String> {
    info!(terminal_id = %terminal_id, "sending terminal/release request");

    let request = ReleaseTerminalRequest::new(session_id.clone(), TerminalId::new(terminal_id));

    conn.send_request(request)
        .block_task()
        .await
        .map_err(|e| e.to_string())?;

    info!(terminal_id = %terminal_id, "terminal/release completed");
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
