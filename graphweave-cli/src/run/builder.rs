//! Runner build dispatch: one branch per agent pattern.
//!
//! To add a new agent: add a variant to [`RunCmd`], then add a branch here in [`build_runner`].

use graphweave::{
    build_dup_runner, build_got_runner, build_react_runner, build_tot_runner,
    ReactBuildConfig,
};

use super::agent::{AnyRunner, RunCmd};
use super::RunError;
use super::RunOptions;

/// Builds the runner for the given command. Add new patterns by adding a branch here.
pub(crate) async fn build_runner(
    config: &ReactBuildConfig,
    opts: &RunOptions,
    cmd: &RunCmd,
) -> Result<AnyRunner, RunError> {
    match cmd {
        RunCmd::React => {
            let r = build_react_runner(config, None, opts.verbose, None).await?;
            Ok(AnyRunner::React(r))
        }
        RunCmd::Dup => {
            let r = build_dup_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Dup(r))
        }
        RunCmd::Tot => {
            let r = build_tot_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Tot(r))
        }
        RunCmd::Got { .. } => {
            let r = build_got_runner(config, None, opts.verbose).await?;
            Ok(AnyRunner::Got(r))
        }
    }
}
