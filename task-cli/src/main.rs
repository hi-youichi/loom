use std::path::PathBuf;

use clap::{Parser, Subcommand};
use task_core::{parse_status, CreateParams, ListParams, ShowError, TaskDb, UpdateParams};

const AFTER_HELP: &str = "\
COMMANDS
  create  --name NAME [--description TEXT] [--assignee NAME] [--start-time TIME] [--status STATUS]
  show    ID
  list    [--status STATUS] [--assignee NAME] [--name QUERY] [--sort-by FIELD] [--sort-order ORDER] [--limit N] [--page N]
  update  ID [--name NAME] [--description TEXT] [--assignee NAME] [--start-time TIME] [--status STATUS]
  delete  ID

GLOBAL
  --work-folder DIR   Database: <DIR>/tasks.db, default: current directory

FIELDS
  name        string, required on create
  description string, optional
  assignee    string, optional
  start_time  string, optional, formats: ISO 8601 | YYYY-MM-DD HH:MM:SS | YYYY-MM-DD, default: now
  status      enum: pending | in_progress | completed | cancelled, default: pending

ID RESOLUTION
  Full UUID or short prefix (>= 4 chars). Ambiguous prefix returns error with candidates.

OUTPUT
  JSON to stdout. Structure: {\"ok\": bool, \"data\": {...}} or {\"ok\": bool, \"error\": string, \"message\": string}

EXIT CODES
  0 success | 1 bad args | 2 not found/ambiguous | 3 database error

EXAMPLES
  task create --name \"Fix login\" --assignee \"Alice\"
  task show a1b2c3d4
  task list --status pending --sort-by created_at --sort-order desc --limit 10 --page 1
  task update a1b2c3d4 --status completed
  task delete a1b2c3d4";

#[derive(Parser, Debug)]
#[command(name = "task")]
#[command(about = "Task management CLI for agents.")]
#[command(after_help = AFTER_HELP)]
pub struct Args {
    #[command(subcommand)]
    pub command: TaskCommand,

    #[arg(long, value_name = "DIR", global = true)]
    pub work_folder: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TaskCommand {
    Create(CreateCliArgs),
    Show { id: String },
    List(ListCliArgs),
    Update(UpdateCliArgs),
    Delete { id: String },
}

#[derive(clap::Args, Debug, Clone)]
pub struct CreateCliArgs {
    #[arg(long)]
    pub name: String,

    #[arg(long, default_value = "")]
    pub description: String,

    #[arg(long, default_value = "")]
    pub assignee: String,

    #[arg(long, value_name = "TIME")]
    pub start_time: Option<String>,

    #[arg(long, default_value = "pending", value_name = "STATUS")]
    pub status: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ListCliArgs {
    #[arg(long)]
    pub status: Option<String>,

    #[arg(long)]
    pub assignee: Option<String>,

    #[arg(long)]
    pub name: Option<String>,

    #[arg(long, default_value = "created_at", value_name = "FIELD")]
    pub sort_by: String,

    #[arg(long, default_value = "desc", value_name = "ORDER")]
    pub sort_order: String,

    #[arg(long, default_value_t = 20)]
    pub limit: u32,

    #[arg(long, default_value_t = 1)]
    pub page: u32,
}

#[derive(clap::Args, Debug, Clone)]
pub struct UpdateCliArgs {
    pub id: String,

    #[arg(long)]
    pub name: Option<String>,

    #[arg(long)]
    pub description: Option<String>,

    #[arg(long)]
    pub assignee: Option<String>,

    #[arg(long, value_name = "TIME")]
    pub start_time: Option<String>,

    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
}

fn resolve_work_folder(args: &Args) -> PathBuf {
    args.work_folder
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn main() {
    let args = Args::parse();
    let work_dir = resolve_work_folder(&args);
    let db_path = work_dir.join("tasks.db");

    let task_db = match TaskDb::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            print_error("database_error", &e.to_string());
            std::process::exit(3);
        }
    };

    let result = run(&args, &task_db);
    match result {
        Ok(json_value) => {
            print_ok(&json_value);
        }
        Err(e) => {
            if let Some(show_err) = e.downcast_ref::<ShowError>() {
                match show_err {
                    ShowError::NotFound(id) => {
                        print_error("not_found", &format!("task not found: {}", id));
                        std::process::exit(2);
                    }
                    ShowError::Ambiguous { prefix, matches } => {
                        let candidates: Vec<serde_json::Value> = matches
                            .iter()
                            .map(|(id, name)| {
                                serde_json::json!({"id": id, "name": name})
                            })
                            .collect();
                        print_error_data(
                            "ambiguous_id",
                            &format!(
                                "ambiguous id '{}', matched {} tasks",
                                prefix,
                                matches.len()
                            ),
                            &serde_json::json!({"candidates": candidates}),
                        );
                        std::process::exit(2);
                    }
                    ShowError::DbError(msg) => {
                        print_error("database_error", msg);
                        std::process::exit(3);
                    }
                }
            } else {
                print_error("error", &e.to_string());
                std::process::exit(3);
            }
        }
    }
}

fn run(args: &Args, db: &TaskDb) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match &args.command {
        TaskCommand::Create(cli) => {
            let status = parse_status(&cli.status)?;
            let params = CreateParams {
                name: cli.name.clone(),
                description: cli.description.clone(),
                assignee: cli.assignee.clone(),
                start_time: cli.start_time.clone(),
                status,
            };
            let task = db.create_task(&params)?;
            Ok(serde_json::to_value(&task)?)
        }

        TaskCommand::Show { id } => {
            let task = db.show_task(id)?;
            Ok(serde_json::to_value(&task)?)
        }

        TaskCommand::List(cli) => {
            let status = cli
                .status
                .as_deref()
                .map(parse_status)
                .transpose()?;
            let params = ListParams {
                status,
                assignee: cli.assignee.clone(),
                name: cli.name.clone(),
                sort_by: cli.sort_by.clone(),
                sort_order: cli.sort_order.clone(),
                limit: cli.limit,
                page: cli.page,
            };
            let list = db.list_tasks(&params)?;
            Ok(serde_json::to_value(&list)?)
        }

        TaskCommand::Update(cli) => {
            let status = cli
                .status
                .as_deref()
                .map(parse_status)
                .transpose()?;
            let params = UpdateParams {
                id: cli.id.clone(),
                name: cli.name.clone(),
                description: cli.description.clone(),
                assignee: cli.assignee.clone(),
                start_time: cli.start_time.clone(),
                status,
            };
            let task = db.update_task(&params)?;
            Ok(serde_json::to_value(&task)?)
        }

        TaskCommand::Delete { id } => {
            let deleted = db.delete_task(id)?;
            Ok(serde_json::json!({
                "id": deleted.id,
                "name": deleted.name,
                "deleted": true,
            }))
        }
    }
}

fn print_ok(data: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({"ok": true, "data": data})).unwrap()
    );
}

fn print_error(error: &str, message: &str) {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": error,
            "message": message,
        }))
        .unwrap()
    );
}

fn print_error_data(error: &str, message: &str, data: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": error,
            "message": message,
            "data": data,
        }))
        .unwrap()
    );
}
