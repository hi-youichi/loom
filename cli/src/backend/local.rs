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
        let output = run_agent(opts, cmd, stream_out).await?;
        let crate::run::RunAgentOutput {
            reply,
            reasoning_content,
            events,
            reply_envelope,
            stop_reason,
        } = output;
        Ok(match events {
            Some(ev) => super::RunOutput::Json {
                events: ev,
                reply,
                reasoning_content,
                reply_envelope,
                stop_reason,
            },
            None => super::RunOutput::Reply {
                reply,
                reasoning_content,
                reply_envelope,
                stop_reason,
            },
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
