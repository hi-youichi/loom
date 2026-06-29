//! Convert ACP protocol MCP server definitions to Loom's internal [`McpServerDef`].
//!
//! ACP's [`agent_client_protocol::schema::v1::McpServer`] enum has four variants
//! (Stdio, Http, Sse, Acp), with `Vec<EnvVariable>` / `Vec<HttpHeader>` instead of
//! Loom's `HashMap<String, String>`. This module bridges the gap.

use std::collections::HashMap;

use agent_client_protocol::schema::v1::{
    HttpHeader, McpServer, McpServerHttp, McpServerSse, McpServerStdio,
};

use config::McpServerDef;

/// Convert a list of ACP MCP servers to Loom's [`McpServerDef`] list.
///
/// Unsupported variants (e.g. `Acp` when `unstable_mcp_over_acp` is disabled) are
/// silently skipped with a `tracing::warn`.
pub fn acp_mcp_to_loom(servers: &[McpServer]) -> Vec<McpServerDef> {
    servers.iter().filter_map(mcp_server_to_def).collect()
}

fn mcp_server_to_def(server: &McpServer) -> Option<McpServerDef> {
    match server {
        McpServer::Stdio(s) => Some(stdio_to_def(s)),
        McpServer::Http(s) => Some(http_to_def(s)),
        McpServer::Sse(s) => Some(sse_to_def(s)),
        other => {
            tracing::warn!(?other, "Unsupported MCP server variant, skipped");
            None
        }
    }
}

fn stdio_to_def(s: &McpServerStdio) -> McpServerDef {
    McpServerDef::Stdio {
        name: s.name.clone(),
        command: s.command.to_string_lossy().into_owned(),
        args: s.args.clone(),
        env: env_vars_to_map(&s.env),
    }
}

fn http_to_def(s: &McpServerHttp) -> McpServerDef {
    McpServerDef::Http {
        name: s.name.clone(),
        url: s.url.clone(),
        headers: http_headers_to_map(&s.headers),
        oauth: None,
    }
}

/// SSE maps to Http — rmcp's `StreamableHttpClientTransport` handles both HTTP and SSE.
fn sse_to_def(s: &McpServerSse) -> McpServerDef {
    McpServerDef::Http {
        name: s.name.clone(),
        url: s.url.clone(),
        headers: http_headers_to_map(&s.headers),
        oauth: None,
    }
}

fn env_vars_to_map(vars: &[agent_client_protocol::schema::v1::EnvVariable]) -> HashMap<String, String> {
    vars.iter()
        .map(|v| (v.name.clone(), v.value.clone()))
        .collect()
}

fn http_headers_to_map(headers: &[HttpHeader]) -> HashMap<String, String> {
    headers
        .iter()
        .map(|h| (h.name.clone(), h.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{EnvVariable, McpServerHttp, McpServerSse, McpServerStdio};
    use std::path::PathBuf;

    #[test]
    fn stdio_conversion() {
        let acp = McpServer::Stdio(McpServerStdio::new("my-server", "/usr/bin/node"));
        let loom = acp_mcp_to_loom(&[acp]);
        assert_eq!(loom.len(), 1);
        match &loom[0] {
            McpServerDef::Stdio {
                name,
                command,
                args,
                env,
            } => {
                assert_eq!(name, "my-server");
                assert_eq!(command, "/usr/bin/node");
                assert!(args.is_empty());
                assert!(env.is_empty());
            }
            McpServerDef::Http { .. } => panic!("expected Stdio"),
        }
    }

    #[test]
    fn stdio_conversion_with_env_and_args() {
        let mut stdio = McpServerStdio::new("fs", "npx");
        stdio.args = vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()];
        stdio.env = vec![
            EnvVariable::new("API_KEY", "secret"),
            EnvVariable::new("NODE_ENV", "production"),
        ];
        let acp = McpServer::Stdio(stdio);
        let loom = acp_mcp_to_loom(&[acp]);
        assert_eq!(loom.len(), 1);
        match &loom[0] {
            McpServerDef::Stdio {
                name,
                command,
                args,
                env,
            } => {
                assert_eq!(name, "fs");
                assert_eq!(command, "npx");
                assert_eq!(args.as_slice(), ["-y", "@modelcontextprotocol/server-filesystem"]);
                assert_eq!(env.get("API_KEY").map(|s| s.as_str()), Some("secret"));
                assert_eq!(env.get("NODE_ENV").map(|s| s.as_str()), Some("production"));
            }
            McpServerDef::Http { .. } => panic!("expected Stdio"),
        }
    }

    #[test]
    fn http_conversion() {
        let mut http = McpServerHttp::new("remote", "https://mcp.example.com/api");
        http.headers = vec![HttpHeader::new("Authorization", "Bearer token123")];
        let acp = McpServer::Http(http);
        let loom = acp_mcp_to_loom(&[acp]);
        assert_eq!(loom.len(), 1);
        match &loom[0] {
            McpServerDef::Http {
                name,
                url,
                headers,
                oauth,
            } => {
                assert_eq!(name, "remote");
                assert_eq!(url, "https://mcp.example.com/api");
                assert_eq!(
                    headers.get("Authorization").map(|s| s.as_str()),
                    Some("Bearer token123")
                );
                assert!(oauth.is_none());
            }
            McpServerDef::Stdio { .. } => panic!("expected Http"),
        }
    }

    #[test]
    fn sse_conversion_maps_to_http() {
        let sse = McpServerSse::new("sse-server", "https://mcp.example.com/sse");
        let acp = McpServer::Sse(sse);
        let loom = acp_mcp_to_loom(&[acp]);
        assert_eq!(loom.len(), 1);
        match &loom[0] {
            McpServerDef::Http { name, url, .. } => {
                assert_eq!(name, "sse-server");
                assert_eq!(url, "https://mcp.example.com/sse");
            }
            McpServerDef::Stdio { .. } => panic!("SSE should map to Http"),
        }
    }

    #[test]
    fn empty_list() {
        let loom = acp_mcp_to_loom(&[]);
        assert!(loom.is_empty());
    }

    #[test]
    fn multiple_servers_preserve_order() {
        let servers = vec![
            McpServer::Stdio(McpServerStdio::new("a", "cmd-a")),
            McpServer::Http(McpServerHttp::new("b", "https://b.example.com")),
        ];
        let loom = acp_mcp_to_loom(&servers);
        assert_eq!(loom.len(), 2);
        assert_eq!(loom[0].name(), "a");
        assert_eq!(loom[1].name(), "b");
    }

    #[test]
    fn stdio_command_pathbuf_to_string() {
        let acp = McpServer::Stdio(McpServerStdio::new("p", "/path/with spaces/server"));
        let loom = acp_mcp_to_loom(&[acp]);
        match &loom[0] {
            McpServerDef::Stdio { command, .. } => {
                assert_eq!(command, "/path/with spaces/server");
            }
            _ => panic!("expected Stdio"),
        }
    }
}
