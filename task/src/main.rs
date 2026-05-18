mod args;
mod db;
mod models;

use clap::Parser;

use args::{Args, TaskCommand};
use db::TaskDb;

fn main() {
    let args = Args::parse();
    let work_dir = args::resolve_work_folder(&args);

    let task_db = match TaskDb::open(&work_dir) {
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
            match e.downcast_ref::<db::ShowError>() {
                Some(db::ShowError::NotFound(id)) => {
                    print_error("not_found", &format!("task not found: {}", id));
                    std::process::exit(2);
                }
                Some(db::ShowError::Ambiguous { prefix, matches }) => {
                    let candidates: Vec<serde_json::Value> = matches
                        .iter()
                        .map(|(id, name)| {
                            serde_json::json!({"id": id, "name": name})
                        })
                        .collect();
                    print_error_data(
                        "ambiguous_id",
                        &format!("ambiguous id '{}', matched {} tasks", prefix, matches.len()),
                        &serde_json::json!({"candidates": candidates}),
                    );
                    std::process::exit(2);
                }
                Some(db::ShowError::DbError(msg)) => {
                    print_error("database_error", msg);
                    std::process::exit(3);
                }
                None => {
                    print_error("error", &e.to_string());
                    std::process::exit(3);
                }
            }
        }
    }
}

fn run(args: &Args, db: &TaskDb) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match &args.command {
        TaskCommand::Create(create_args) => {
            let task = db.create_task(create_args)?;
            Ok(serde_json::to_value(&task)?)
        }

        TaskCommand::Show { id } => {
            let task = db.show_task(id)?;
            Ok(serde_json::to_value(&task)?)
        }

        TaskCommand::List(list_args) => {
            let list = db.list_tasks(list_args)?;
            Ok(serde_json::to_value(&list)?)
        }

        TaskCommand::Update(update_args) => {
            let task = db.update_task(update_args)?;
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
