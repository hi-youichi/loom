use std::sync::Arc;
use tokio::sync::watch;

use loom_types::cli_run::{ActiveOperationCanceller, ActiveOperationKind, ActiveOperation};

#[derive(Debug)]
pub struct ChildProcessCanceller {
    pub kill_tx: watch::Sender<bool>,
}

impl ActiveOperationCanceller for ChildProcessCanceller {
    fn cancel(&self) {
        let _ = self.kill_tx.send(true);
    }
}

pub fn setup_cancellation(
    ctx: Option<&crate::tool_source::ToolCallContext>,
    kill_tx: watch::Sender<bool>,
) -> Option<watch::Sender<bool>> {
    if let Some(run_cancellation) = ctx.and_then(|c| c.run_cancellation.clone()) {
        run_cancellation.set_active_operation(ActiveOperation::new(
            ActiveOperationKind::ChildProcess,
            Arc::new(ChildProcessCanceller { kill_tx }),
        ));
        None
    } else {
        Some(kill_tx)
    }
}