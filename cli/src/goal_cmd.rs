use std::path::PathBuf;
use std::sync::Arc;

use crate::args::GoalArgs;
use loom::goal_runner::{
    GoalOutcome, GoalRunner, LoomTool, ShellTool, generate_mcp_config, resume,
};
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
    let db_path = ensure_task_db()?;
    let db = Arc::new(TaskDb::open(&db_path).await?);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    ctrlc::set_handler(move || {
        cancel_clone.cancel();
    })?;

    if let Some(ref id) = ga.resume {
        eprintln!("resuming goal {}...", id);
        let mut runner = resume(id, working_dir, db, cancel).await?;
        print_task_id(runner.task_id());
        let outcome = runner.run().await;
        print_outcome(&outcome);
        match outcome {
            GoalOutcome::Error(_) => std::process::exit(1),
            _ => {}
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

    let tool: Box<dyn loom::goal_runner::CodingTool> = match ga.tool.as_str() {
        "loom" => {
            let mcp_config_path = write_mcp_config(&db_path, &working_dir)?;
            Box::new(LoomTool::new(
                "goal-session".to_string(),
                working_dir.clone(),
                mcp_config_path,
            ))
        }
        name => {
            let args = match name {
                "codex" => vec!["--goal-prompt".to_string()],
                "claude" => vec!["--goal-prompt".to_string()],
                "cursor" => vec!["--goal-prompt".to_string()],
                _ => vec![],
            };
            Box::new(ShellTool::new(name.to_string(), args))
        }
    };

    let mut runner = GoalRunner::new(description, working_dir, db, tool, cancel).await?;
    print_task_id(runner.task_id());
    let outcome = runner.run().await;
    print_outcome(&outcome);
    match outcome {
        GoalOutcome::Error(_) => std::process::exit(1),
        _ => {}
    }
    Ok(())
}

fn print_task_id(task_id: &str) {
    eprintln!("task_id: {}", &task_id[..8.min(task_id.len())]);
}

fn print_outcome(outcome: &GoalOutcome) {
    match outcome {
        GoalOutcome::Achieved => eprintln!("goal achieved"),
        GoalOutcome::Error(e) => eprintln!("goal failed: {}", e),
    }
}

fn write_mcp_config(db_path: &PathBuf, working_dir: &PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config_content = generate_mcp_config("task", db_path);
    let config_dir = working_dir.join(".loom");
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("goal-mcp.json");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

fn ensure_task_db() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let loom_home = config::home::loom_home();
    let db_dir = loom_home.join("tasks");
    std::fs::create_dir_all(&db_dir)?;
    Ok(db_dir.join("tasks.db"))
}
