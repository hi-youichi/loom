//! LocalBackend: run agent in-process.

use crate::{list_tools, run_agent, show_tool, ToolShowFormat};
use async_trait::async_trait;
use loom::{RunCmd, RunError, RunOptions};

use super::RunBackend;

pub struct LocalBackend;

#[async_trait]
impl RunBackend for LocalBackend {
    async fn run(
        &self,
        opts: &RunOptions,
        cmd: &RunCmd,
        stream_out: super::StreamOut,
    ) -> Result<super::RunOutput, RunError> {
        let (reply, events, reply_envelope) = run_agent(opts, cmd, stream_out).await?;
        Ok(match events {
            Some(ev) => super::RunOutput::Json {
                events: ev,
                reply,
                reply_envelope,
            },
            None => super::RunOutput::Reply(reply, reply_envelope),
        })
    }

    async fn list_tools(&self, opts: &RunOptions) -> Result<(), RunError> {
        list_tools(opts).await
    }

    async fn show_tool(
        &self,
        opts: &RunOptions,
        name: &str,
        format: ToolShowFormat,
    ) -> Result<(), RunError> {
        show_tool(opts, name, format).await
    }
}
