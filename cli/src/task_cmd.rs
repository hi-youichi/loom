use std::path::PathBuf;
use std::sync::Arc;

use task_core::{CreateParams, ListParams, TaskDb, TaskStatus};

use crate::args::{TaskArgs, TaskCommand};
use crate::display_limits::{generate_session_id, max_message_len};
use crate::output::{emit_run_output, make_stream_out, OutputConfig};
use crate::repl::{run_one_turn, run_repl_loop};
use cli::RunOptions;
use loom::UserContent;

use crate::args::Command;

pub(crate) async fn handle_task_command(ta: &TaskArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = ensure_task_db()?;
    let db = Arc::new(TaskDb::open(&db_path).await?);

    match &ta.command {
        TaskCommand::New {
            description,
            agent,
            model,
        } => {
            let desc = description.join(" ");
            if desc.trim().is_empty() {
                eprintln!("loom task new: provide a task description");
                std::process::exit(1);
            }

            let name = truncate_name(&desc);
            let task = db
                .create_task(&CreateParams {
                    name,
                    description: desc.clone(),
                    assignee: agent.clone(),
                    start_time: None,
                    status: TaskStatus::InProgress,
                })
                .await?;

            let task_id_short = &task.id[..8.min(task.id.len())];
            eprintln!("task_id: {}", task_id_short);
            eprintln!("status: in_progress");
            eprintln!("assignee: {}", agent);
            eprintln!();

            let working_dir = std::env::current_dir()?;
            let message = format!(
                "客户提交了一个新任务，请开始处理。\n\n主 Task ID: {}\n需求：{}\n\n请按 Task-Driven Workflow 执行。",
                task.id, desc
            );

            let run_cancellation = loom::cli_run::RunCancellation::new(0);
            let rc_clone = run_cancellation.clone();
            ctrlc::set_handler(move || {
                rc_clone.cancel();
            })?;

            let mut opts = RunOptions {
                message: UserContent::Text(message),
                working_folder: Some(working_dir),
                session_id: None,
                cancellation: Some(run_cancellation),
                thread_id: Some(generate_session_id()),
                agent: Some(agent.clone()),
                verbose: false,
                got_adaptive: false,
                display_max_len: max_message_len(),
                output_json: false,
                model: model.clone(),
                mcp_config_path: None,
                output_timestamp: false,
                dry_run: false,
                provider: None,
                base_url: None,
                api_key: None,
                provider_type: None,
                any_stream_event_sender: None,
                bash_executor: None,
                extra_tools: None,
                acp_session_id: None,
                force_compact: false,
                chat_id: None,
                worktree: false,
                debug_llm: false,
            };

            let output = OutputConfig {
                json: false,
                pretty: false,
                file: None,
            };
            let stream_out = make_stream_out(&output);
            let reply_len = max_message_len();

            let initial_message = Some(format!("Task {} 已创建", task_id_short));
            run_interactive_mode(
                &mut opts,
                &Command::React,
                initial_message,
                reply_len,
                &output,
                stream_out,
            )
            .await?;
        }

        TaskCommand::List { status, assignee } => {
            let status_filter = status
                .as_deref()
                .and_then(|s| task_core::parse_status(s).ok());
            let list = db
                .list_tasks(&ListParams {
                    status: status_filter,
                    assignee: assignee.clone(),
                    name: None,
                    sort_by: "created_at".to_string(),
                    sort_order: "desc".to_string(),
                    limit: 50,
                    page: 1,
                })
                .await?;

            if list.tasks.is_empty() {
                println!("No tasks found.");
                return Ok(());
            }

            println!("{:<10} {:<12} {:<20} NAME", "ID", "STATUS", "ASSIGNEE");
            println!("{}", "-".repeat(70));
            for t in &list.tasks {
                let id_short = &t.id[..8.min(t.id.len())];
                println!(
                    "{:<10} {:<12} {:<20} {}",
                    id_short, t.status, t.assignee, t.name
                );
            }
            println!("\nTotal: {} tasks", list.total);
        }

        TaskCommand::Show { id } => {
            let task = db.show_task(id).await.map_err(|e| {
                Box::<dyn std::error::Error>::from(e.to_string())
            })?;
            println!("ID:          {}", task.id);
            println!("Name:        {}", task.name);
            println!("Status:      {}", task.status);
            println!("Assignee:    {}", task.assignee);
            println!("Start Time:  {}", task.start_time);
            println!("Created At:  {}", task.created_at);
            println!("Description:\n{}", task.description);
        }

        TaskCommand::Continue { id, agent } => {
            let task = db.show_task(id).await.map_err(|e| {
                Box::<dyn std::error::Error>::from(e.to_string())
            })?;

            let task_id_short = &task.id[..8.min(task.id.len())];
            eprintln!("resuming task {} ...", task_id_short);

            let working_dir = std::env::current_dir()?;
            let message = format!(
                "请继续处理以下任务。\n\n主 Task ID: {}\n需求：{}\n状态：{}\n\n请检查子任务进度，继续执行。",
                task.id, task.description, task.status
            );

            let run_cancellation = loom::cli_run::RunCancellation::new(0);
            let rc_clone = run_cancellation.clone();
            ctrlc::set_handler(move || {
                rc_clone.cancel();
            })?;

            let mut opts = RunOptions {
                message: UserContent::Text(message),
                working_folder: Some(working_dir),
                session_id: None,
                cancellation: Some(run_cancellation),
                thread_id: Some(generate_session_id()),
                agent: Some(agent.clone()),
                verbose: false,
                got_adaptive: false,
                display_max_len: max_message_len(),
                output_json: false,
                model: None,
                mcp_config_path: None,
                output_timestamp: false,
                dry_run: false,
                provider: None,
                base_url: None,
                api_key: None,
                provider_type: None,
                any_stream_event_sender: None,
                bash_executor: None,
                extra_tools: None,
                acp_session_id: None,
                force_compact: false,
                chat_id: None,
                worktree: false,
                debug_llm: false,
            };

            let output = OutputConfig {
                json: false,
                pretty: false,
                file: None,
            };
            let stream_out = make_stream_out(&output);
            let reply_len = max_message_len();

            let initial_message = Some(format!("Task {} 已恢复", task_id_short));
            run_interactive_mode(
                &mut opts,
                &Command::React,
                initial_message,
                reply_len,
                &output,
                stream_out,
            )
            .await?;
        }
    }

    Ok(())
}

fn ensure_task_db() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let loom_home = config::home::loom_home();
    let db_dir = loom_home.join("tasks");
    std::fs::create_dir_all(&db_dir)?;
    Ok(db_dir.join("tasks.db"))
}

fn truncate_name(desc: &str) -> String {
    let line = desc.lines().next().unwrap_or(desc);
    if line.len() <= 60 {
        line.to_string()
    } else {
        format!("{}...", &line[..57])
    }
}

async fn run_interactive_mode(
    opts: &mut RunOptions,
    cmd: &Command,
    initial_message: Option<String>,
    reply_len: usize,
    output: &OutputConfig,
    stream_out: cli::StreamOut,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(session_id) = opts.thread_id.as_deref() {
        eprintln!("Session: {}", session_id);
    }

    let stream_clone = stream_out.clone();
    if let Some(msg) = initial_message.filter(|m| !m.trim().is_empty()) {
        eprintln!("{}", msg);
    }

    let message = opts.message.clone();
    match run_one_turn(opts, cmd, stream_out).await {
        Ok(output_value) => emit_run_output(
            output_value,
            output,
            opts.thread_id.as_deref(),
            reply_len,
            opts.output_timestamp,
        )?,
        Err(err) => {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    }

    opts.message = message;
    run_repl_loop(opts, cmd, reply_len, output.clone(), stream_clone).await?;
    println!("Bye.");
    Ok(())
}
