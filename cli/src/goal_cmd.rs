use std::sync::Arc;

use crate::args::GoalArgs;
use crate::goal_runner::{
    GoalRunner, LoomTool, ShellTool, resume, write_mcp_config,
};
use loom_cli_types::goal_runner::GoalOutcome;
use loom_cli_types::RunCancellation;
use task_core::TaskDb;
use tokio_util::sync::CancellationToken;

pub(crate) async fn handle_goal_command(ga: &GoalArgs) -> Result<(), Box<dyn std::error::Error>> {
    if ga.verbose {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("loom=info")
            .with_writer(std::io::stderr)
            .try_init();
    }

    let working_dir = std::env::current_dir()?;
    let db_path = crate::task_db::ensure_task_db()?;
    let db = Arc::new(TaskDb::open(&db_path).await?);
    let cancel = CancellationToken::new();
    let run_cancellation = RunCancellation::new(0);

    let cancel_clone = cancel.clone();
    let rc_clone = run_cancellation.clone();
    ctrlc::set_handler(move || {
        cancel_clone.cancel();
        rc_clone.cancel();
    })?;

    if let Some(ref id) = ga.resume {
        eprintln!("resuming goal {}...", id);
        let mut runner = resume(id, working_dir, db, cancel, Some(run_cancellation)).await?;
        print_task_id(runner.task_id());
        let outcome = runner.run().await;
        let session_content = runner.into_session_content();
        let session_id = format!("goal-{}", &id[..8.min(id.len())]);
        print_outcome(&outcome);
        spawn_goal_background_review(&ga.model, session_content, session_id);
        if let GoalOutcome::Error(_) = outcome {
            std::process::exit(1);
        }
        return Ok(());
    }

let description = match &ga.description {
        Some(d) => d.clone(),
        None => {
            eprintln!("loom goal: provide a goal description or use --resume <ID>");
            std::process::exit(1);
        }
    };

    // Create task first to get task_id for session_id
    let task = db
        .create_task(&task_core::CreateParams {
            name: description.clone(),
            description: description.clone(),
            status: task_core::TaskStatus::InProgress,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("failed to create task: {}", e))?;

    let task_id_short = task.id[..8.min(task.id.len())].to_string();
    let session_id = format!("goal-{}", &task_id_short);

    let tool: Box<dyn crate::goal_runner::CodingTool> = match ga.tool.as_str() {
        "loom" => {
            let mcp_config_path = write_mcp_config(&db_path, &working_dir)?;
            let mut loom_tool = LoomTool::new(
                session_id.clone(),
                working_dir.clone(),
                mcp_config_path,
            )
            .with_cancellation(run_cancellation.clone());
            if let Some(ref model) = ga.model {
                loom_tool = loom_tool.with_model(model.clone());
            }
            Box::new(loom_tool)
        }
        name => {
            let args = crate::goal_runner::shell_tool_args(name);
            Box::new(ShellTool::new(name.to_string(), args).with_cancel(cancel.clone()))
        }
    };

    let mut runner = GoalRunner::new(description, working_dir, db, tool, cancel).await?;
    if let Some(budget) = ga.token_budget {
        runner = runner.with_token_budget(budget);
    }
    if let Some(ref verify) = ga.verify {
        runner = runner.with_verify_command(verify.clone());
    }
    print_task_id(runner.task_id());
    let outcome = runner.run().await;
    let session_content = runner.into_session_content();
    print_outcome(&outcome);
    spawn_goal_background_review(&ga.model, session_content, session_id);
    if let GoalOutcome::Error(_) = outcome {
        std::process::exit(1);
    }
    Ok(())
}

fn print_task_id(task_id: &str) {
    eprintln!("task_id: {}", &task_id[..8.min(task_id.len())]);
}

fn print_outcome(outcome: &GoalOutcome) {
    match outcome {
        GoalOutcome::Error(e) => eprintln!("goal failed: {}", e),
        GoalOutcome::UsageLimited { tokens_used, token_budget } => {
            eprintln!("goal stopped: token budget exhausted ({}/{})", tokens_used, token_budget);
        }
        GoalOutcome::Achieved => eprintln!("goal achieved"),
    }
}





/// Build a background review config from env vars and optional model override,
/// then spawn the review as a background task.
fn spawn_goal_background_review(
    model_override: &Option<String>,
    session_content: String,
    session_id: String,
) {
    if session_content.trim().is_empty() {
        return;
    }
    let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_default();
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if base_url.is_empty() || api_key.is_empty() {
        return;
    }
    let model = model_override
        .clone()
        .or_else(|| std::env::var("MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let config = loom_background_review::BackgroundReviewConfig {
        enabled: true,
        base_url,
        api_key,
        model,
        ..Default::default()
    };
    loom_background_review::spawn_background_review(config, session_content, session_id, None);
}
