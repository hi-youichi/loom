//! Handlers for `tool`, `models`, `session`, and `mcp` CLI subcommands.

use cli::{cli_list_models, cli_list_tools, cli_show_tool, ToolShowFormat};

use crate::args::{
    AgentArgs, AgentCommand, Args, CuratorCmdArgs, CuratorCommand, ExportArgs, McpArgs,
    McpCommand, MemoryCmdArgs, MemoryCommand, ModelsArgs, ModelsCommand, SkillsArgs,
    SkillsCommand, ToolArgs, ToolCommand,
};
use loom_react_config::profile_convert::ExportFormat;
use crate::mcp_manager::{AddMcpArgs, EditMcpArgs, McpManager, ServerDetail, ServerInfo};
use crate::run_flow::build_run_options;
use crate::session::{SessionArgs, SessionCommand, SessionManager};

pub(crate) async fn handle_tool_command(
    args: &Args,
    tool_args: &ToolArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = build_run_options(args, String::new(), false);
    match &tool_args.sub {
        ToolCommand::List => cli_list_tools(&opts).await?,
        ToolCommand::Show(show_args) => {
            let format = if args.json || show_args.output.eq_ignore_ascii_case("json") {
                ToolShowFormat::Json
            } else {
                ToolShowFormat::Yaml
            };
            cli_show_tool(&opts, &show_args.name, format).await?;
        }
    }
    Ok(())
}

pub(crate) async fn handle_models_command(
    args: &Args,
    models_args: &ModelsArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = build_run_options(args, String::new(), false);
    match &models_args.sub {
        ModelsCommand::List => cli_list_models(&opts, None).await?,
        ModelsCommand::Show(show_args) => cli_list_models(&opts, Some(&show_args.name)).await?,
    }
    Ok(())
}

pub(crate) async fn handle_session_command(
    sa: &SessionArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = SessionManager::with_default_path();

    match &sa.command {
        SessionCommand::List => {
            let sessions = manager.list_sessions()?;
            manager.print_session_list(&sessions, json)?;
        }
        SessionCommand::Show { session_id } => match manager.show_session(session_id)? {
            Some(detail) => manager.print_session_detail(&detail, json)?,
            None => {
                eprintln!("Session not found: {}", session_id);
                std::process::exit(1);
            }
        },
        SessionCommand::Delete { session_id } => {
            let count = manager.delete_session(session_id)?;
            if json {
                let result = serde_json::json!({
                    "session_id": session_id,
                    "deleted_checkpoints": count
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Deleted session {} ({} checkpoints)", session_id, count);
            }
        }
        SessionCommand::Rename { session_id, title } => {
            manager.rename_session(session_id, title)?;
            if json {
                let result = serde_json::json!({
                    "session_id": session_id,
                    "title": title
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Renamed session {} to \"{}\"", session_id, title);
            }
        }
        SessionCommand::Cat { session_id } => {
            let events = manager.cat_session(session_id)?;
            if json {
                for event in &events {
                    println!("{}", serde_json::to_string(event)?);
                }
            } else {
                crate::codex_event_builder::print_cat_text(&events);
            }
        }
    }
    Ok(())
}

pub(crate) fn handle_mcp_command(
    mcp_args: &McpArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = McpManager::new()?;

    match &mcp_args.command {
        McpCommand::List => {
            let servers = manager.list_servers()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&servers)?);
            } else {
                print_server_list(&servers);
            }
        }
        McpCommand::Show { name } => match manager.show_server(name)? {
            Some(detail) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&detail)?);
                } else {
                    print_server_detail(&detail);
                }
            }
            None => {
                eprintln!("MCP server not found: {}", name);
                std::process::exit(1);
            }
        },
        McpCommand::Add(add_args) => {
            let cli_args = AddMcpArgs {
                name: add_args.name.clone(),
                command: add_args.command.clone(),
                args: add_args.args.clone(),
                url: add_args.url.clone(),
                env: add_args.env.clone(),
                disabled: add_args.disabled,
            };
            manager.add_server(&cli_args)?;
            println!("MCP server '{}' added successfully", add_args.name);
        }
        McpCommand::Edit(edit_args) => {
            let cli_args = EditMcpArgs {
                command: edit_args.command.clone(),
                args: edit_args.args.clone(),
                url: edit_args.url.clone(),
                env: edit_args.env.clone(),
                disabled: edit_args.disabled,
            };
            manager.edit_server(&edit_args.name, &cli_args)?;
            println!("MCP server '{}' updated successfully", edit_args.name);
        }
        McpCommand::Delete { name } => {
            if manager.delete_server(name)? {
                println!("MCP server '{}' deleted successfully", name);
            } else {
                eprintln!("MCP server not found: {}", name);
                std::process::exit(1);
            }
        }
        McpCommand::Enable { name } => {
            manager.enable_server(name)?;
            println!("MCP server '{}' enabled successfully", name);
        }
        McpCommand::Disable { name } => {
            manager.disable_server(name)?;
            println!("MCP server '{}' disabled successfully", name);
        }
    }
    Ok(())
}

fn print_server_list(servers: &[ServerInfo]) {
    println!("MCP Servers:");
    println!("{}", "─".repeat(80));

    if servers.is_empty() {
        println!("No MCP servers configured.");
        return;
    }

    for server in servers {
        let status = if server.disabled { "[disabled]" } else { "" };
        println!("  • {} {}", server.name, status);
        println!("    Type: {}", server.server_type);
        if let Some(cmd) = &server.command {
            println!("    Command: {}", cmd);
        }
        if let Some(url) = &server.url {
            println!("    URL: {}", url);
        }
        println!();
    }
}

fn print_server_detail(detail: &ServerDetail) {
    println!("MCP Server: {}", detail.name);
    println!("{}", "═".repeat(80));

    let status = if detail.entry.disabled {
        "disabled"
    } else {
        "enabled"
    };
    println!("Status: {}", status);

    if let Some(cmd) = &detail.entry.command {
        println!("Command: {}", cmd);
        if !detail.entry.args.is_empty() {
            println!("Args: {}", detail.entry.args.join(" "));
        }
    }

    if let Some(url) = &detail.entry.url {
        println!("URL: {}", url);
    }

    if !detail.entry.env.is_empty() {
        println!("Environment:");
        for (key, value) in &detail.entry.env {
            let masked_value = config::mask_value(value);
            println!("  {}={}", key, masked_value);
        }
    }

    if !detail.entry.headers.is_empty() {
        println!("Headers:");
        for (key, value) in &detail.entry.headers {
            println!("  {}: {}", key, value);
        }
    }
}

pub(crate) fn handle_agent_command(
    agent_args: &AgentArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    match &agent_args.command {
        AgentCommand::List => handle_agent_list(),
        AgentCommand::Export(export_args) => handle_agent_export(export_args),
    }
}

fn handle_agent_list() -> Result<(), Box<dyn std::error::Error>> {
    let profiles = loom_react_config::profile::list_available_profiles();
    if profiles.is_empty() {
        println!("No agent profiles found.");
        return Ok(());
    }
    println!("Agent profiles:");
    for p in &profiles {
        if let Some(desc) = &p.description {
            println!("  • {} — {}", p.name, desc);
        } else {
            println!("  • {}", p.name);
        }
    }
    Ok(())
}

fn handle_agent_export(export_args: &ExportArgs) -> Result<(), Box<dyn std::error::Error>> {
    use loom_react_config::profile_convert::export;

    let format: ExportFormat = export_args.format.parse()?;

    let agent_names: Vec<String> = match &export_args.agent {
        Some(name) => vec![name.clone()],
        None => loom_react_config::profile::list_available_profiles()
            .into_iter()
            .map(|p| p.name)
            .collect(),
    };

    if agent_names.is_empty() {
        println!("No agent profiles found.");
        return Ok(());
    }

    for agent_name in &agent_names {
        let output = export(agent_name, format)?;

        if export_args.dry_run {
            println!("--- {} ---", output.path.display());
            println!("{}", output.content);
        } else {
            let full_path = export_args.output.join(&output.path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, &output.content)?;
            println!("Exported {} -> {}", agent_name, full_path.display());
        }
    }

    Ok(())
}

pub(crate) fn handle_skills_command(
    skills_args: &SkillsArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use cli::run::skill_registry::{SkillRegistry, Source, SkillContent, Lifecycle};

    let registry = SkillRegistry::new(&SkillRegistry::default_path());

    match &skills_args.command {
        SkillsCommand::List => {
            let skills = registry.list()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&skills)?);
            } else if skills.is_empty() {
                println!("No skills found.");
            } else {
                println!("Skills:");
                for s in &skills {
                    let src = match s.source {
                        Source::Auto => "[auto]",
                        Source::Manual => "[manual]",
                        Source::Evolved => "[evolved]",
                    };
                    let lc = match s.lifecycle {
                        Lifecycle::Active => "",
                        Lifecycle::Stale => " [stale]",
                        Lifecycle::Archived => " [archived]",
                    };
                    println!("  • {} {}{} — {}", s.name, src, lc, s.description);
                }
            }
        }
        SkillsCommand::Show { name } => {
            let skill = registry.load(name)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&skill)?);
            } else {
                println!("Skill: {}", skill.name);
                println!("{}", "═".repeat(60));
                println!("Description: {}", skill.description);
                println!("Source: {:?}", skill.source);
                println!("Lifecycle: {:?}", skill.lifecycle);
                println!("Triggers: {}", skill.triggers.join(", "));
                println!();
                println!("{}", skill.body);
            }
        }
        SkillsCommand::Create { name, description, triggers } => {
            let skill = SkillContent {
                name: name.clone(),
                description: description.clone().unwrap_or_default(),
                triggers: triggers.clone(),
                lifecycle: Lifecycle::Active,
                source: Source::Manual,
                body: String::new(),
                raw: String::new(),
            };
            registry.save(name, &skill)?;
            println!("Created skill: {}", name);
        }
        SkillsCommand::Edit { name } => {
            let skill = registry.load(name)?;
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!("loom-skill-{}.md", name));
            std::fs::write(&tmp_path, &skill.raw)?;
            let status = std::process::Command::new(&editor).arg(&tmp_path).status()?;
            if status.success() {
                let edited = std::fs::read_to_string(&tmp_path)?;
                let mut updated = skill.clone();
                updated.raw = edited;
                registry.save(name, &updated)?;
                println!("Updated skill: {}", name);
            }
            let _ = std::fs::remove_file(&tmp_path);
        }
        SkillsCommand::Delete { name } => {
            registry.delete(name)?;
            println!("Deleted skill: {}", name);
        }
    }
    Ok(())
}

pub(crate) fn handle_curator_command(
    curator_args: &CuratorCmdArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use cli::run::curator::{
        Curator, CuratorConfig,
        CuratorReport, CuratorState,
    };
    use cli::run::skill_registry::SkillRegistry;

    let skills = SkillRegistry::new(&SkillRegistry::default_path());
    let curator = Curator::new(skills, CuratorConfig::default());

    match &curator_args.command {
        CuratorCommand::Run => {
            // Run auto-transitions (stale/archive/reactivate)
            let report: CuratorReport = curator.run(curator_args.dry_run)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Curator Review Result:");
                println!("{}", "═".repeat(60));
                println!("Active: {}", report.active);
                println!("Marked Stale: {} {:?}", report.stale.len(), report.stale);
                println!("Archived: {} {:?}", report.archived.len(), report.archived);
                println!("Reactivated: {} {:?}", report.reactivated.len(), report.reactivated);
                println!("Overlapping: {} pairs", report.overlapping.len());
            }
        }
        CuratorCommand::Status => {
            let state: CuratorState = curator.load_state().unwrap_or_default();
            let all = curator.skills.list().unwrap_or_default();

            let active = all.iter().filter(|m| m.lifecycle == cli::run::skill_registry::Lifecycle::Active && !m.pinned).count();
            let stale = all.iter().filter(|m| m.lifecycle == cli::run::skill_registry::Lifecycle::Stale).count();
            let archived = all.iter().filter(|m| m.lifecycle == cli::run::skill_registry::Lifecycle::Archived).count();
            let pinned = all.iter().filter(|m| m.pinned).count();

            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "enabled": true,
                    "paused": state.paused,
                    "run_count": state.run_count,
                    "last_run_at": state.last_run_at,
                    "last_run_summary": state.last_run_summary,
                    "last_report_path": state.last_report_path,
                    "active": active,
                    "stale": stale,
                    "archived": archived,
                    "pinned": pinned,
                }))?);
            } else {
                println!("Curator Status:");
                println!("{}", "═".repeat(60));
                println!("Enabled: true");
                println!("Paused: {}", state.paused);
                println!("Run Count: {}", state.run_count);
                println!("Last Run: {}", state.last_run_at.as_deref().unwrap_or("never"));
                if let Some(summary) = &state.last_run_summary {
                    println!("Last Summary:\n{}", summary);
                }
                println!("\nSkill Counts:");
                println!("  Active: {}", active);
                println!("  Stale: {}", stale);
                println!("  Archived: {}", archived);
                println!("  Pinned: {}", pinned);
            }
        }
        CuratorCommand::Prune { days } => {
            // Hermes 对齐：bulk archive old skills
            let report = curator.run(curator_args.dry_run)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Curator Prune (days={}, dry_run={}):", days, curator_args.dry_run);
                println!("{}", "═".repeat(60));
                println!("Archived: {}", report.archived.len());
                for s in &report.archived {
                    println!("  • {}", s);
                }
            }
        }
        CuratorCommand::Pause => {
            curator.set_paused(true)?;
            println!("Curator paused.");
        }
        CuratorCommand::Resume => {
            curator.set_paused(false)?;
            println!("Curator resumed.");
        }
    }
    Ok(())
}

pub(crate) fn handle_memory_command(
    memory_args: &MemoryCmdArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use cli::run::memory::{MemoryFile, MemoryStore};

    let store = MemoryStore::new(&MemoryStore::default_path());

    match &memory_args.command {
        MemoryCommand::Show => {
            let user = store.load(MemoryFile::User)?;
            let project = store.load(MemoryFile::Project)?;
            let facts = store.load(MemoryFile::Facts)?;
            if json {
                let obj = serde_json::json!({
                    "user": user,
                    "project": project,
                    "facts": facts,
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("USER.md:\n{}\n", if user.is_empty() { "(empty)".to_string() } else { user });
                println!("PROJECT.md:\n{}\n", if project.is_empty() { "(empty)".to_string() } else { project });
                println!("FACTS.md:\n{}\n", if facts.is_empty() { "(empty)".to_string() } else { facts });
            }
        }
        MemoryCommand::Edit { file } => {
            let mf = match file.to_lowercase().as_str() {
                "user" | "user.md" => MemoryFile::User,
                "project" | "project.md" => MemoryFile::Project,
                "facts" | "facts.md" => MemoryFile::Facts,
                _ => { return Err(format!("Unknown memory file: {}. Use USER, PROJECT, or FACTS.", file).into()); }
            };
            let content = store.load(mf)?;
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!("loom-memory-{}.md", file));
            std::fs::write(&tmp_path, &content)?;
            let status = std::process::Command::new(&editor).arg(&tmp_path).status()?;
            if status.success() {
                let edited = std::fs::read_to_string(&tmp_path)?;
                store.replace(mf, &edited)?;
                println!("Updated {}.", file);
            }
            let _ = std::fs::remove_file(&tmp_path);
        }
        MemoryCommand::Search { query } => {
            let matches = store.search(query)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&matches)?);
            } else if matches.is_empty() {
                println!("No matches for '{}'.", query);
            } else {
                for m in &matches {
                    println!("{:?}:{} | {}", m.file, m.line_number, m.line);
                }
            }
        }
    }
    Ok(())
}
