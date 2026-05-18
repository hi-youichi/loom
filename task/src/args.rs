use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::models::TaskStatus;

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
    Create(CreateArgs),
    Show { id: String },
    List(ListArgs),
    Update(UpdateArgs),
    Delete { id: String },
}

#[derive(clap::Args, Debug, Clone)]
pub struct CreateArgs {
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
pub struct ListArgs {
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
pub struct UpdateArgs {
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

pub fn parse_status(s: &str) -> Result<TaskStatus, String> {
    TaskStatus::from_str(s).ok_or_else(|| {
        format!(
            "invalid status '{}'. Valid values: {}",
            s,
            TaskStatus::all_values().join(", ")
        )
    })
}

pub fn resolve_work_folder(args: &Args) -> PathBuf {
    args.work_folder
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}
