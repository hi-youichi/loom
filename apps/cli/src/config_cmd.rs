//! CLI management for anureo's named model providers.
//!
//! This module edits only the relevant TOML tables so unrelated configuration
//! (including `[env]`, logging, sessions, and declared model metadata) remains
//! intact.

use crate::args::{
    AddProviderArgs, ConfigArgs, ConfigCommand, EditProviderArgs, ProviderArgs, ProviderCommand,
};
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use serde::Serialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use toml::{Table, Value};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize)]
struct ProviderOutput {
    name: String,
    provider_type: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: String,
    is_default: bool,
}

#[derive(Serialize)]
struct ProviderTestOutput {
    name: String,
    base_url: String,
    base_url_source: String,
    authenticated: bool,
    success: bool,
    status: Option<u16>,
    model_count: Option<usize>,
    error: Option<String>,
}

pub(crate) fn handle_config_command(args: &ConfigArgs, json: bool) -> Result<()> {
    match &args.command {
        ConfigCommand::Tui => run_provider_tui(),
        ConfigCommand::Provider(provider) => handle_provider_command(provider, json),
    }
}

fn handle_provider_command(args: &ProviderArgs, json: bool) -> Result<()> {
    match &args.command {
        ProviderCommand::List => list_providers(json),
        ProviderCommand::Show { name } => show_provider(name, json),
        ProviderCommand::Add(args) => add_provider(args, json),
        ProviderCommand::Edit(args) => edit_provider(args, json),
        ProviderCommand::SetKey {
            name,
            api_key_stdin,
        } => set_provider_key(name, *api_key_stdin),
        ProviderCommand::Remove { name, yes } => remove_provider(name, *yes),
        ProviderCommand::Use { name } => use_provider(name),
        ProviderCommand::Test { name } => test_provider(name, json),
    }
}

fn run_provider_tui() -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    let result = provider_tui_loop(&mut stdout);
    terminal::disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;
    result
}

fn provider_tui_loop(stdout: &mut io::Stdout) -> Result<()> {
    let mut selected = 0usize;
    let mut status = String::from("Ready");

    loop {
        let items = load_provider_outputs()?;
        if items.is_empty() {
            selected = 0;
        } else {
            selected = selected.min(items.len() - 1);
        }
        render_provider_tui(stdout, &items, selected, &status)?;

        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => selected = selected.saturating_sub(1),
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                if !items.is_empty() {
                    selected = (selected + 1).min(items.len() - 1);
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('a'),
                ..
            }) => {
                let result = with_tui_suspended(stdout, add_provider_interactive);
                status = match result {
                    Ok(name) => format!("Added provider '{name}'"),
                    Err(error) => format!("Add failed: {error}"),
                };
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('e'),
                ..
            }) => {
                if let Some(item) = items.get(selected) {
                    let name = item.name.clone();
                    let result = with_tui_suspended(stdout, || edit_provider_interactive(&name));
                    status = match result {
                        Ok(()) => format!("Updated provider '{name}'"),
                        Err(error) => format!("Edit failed: {error}"),
                    };
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('k'),
                ..
            }) => {
                if let Some(item) = items.get(selected) {
                    let name = item.name.clone();
                    let result = with_tui_suspended(stdout, || set_provider_key(&name, false));
                    status = match result {
                        Ok(()) => format!("Updated API key for '{name}'"),
                        Err(error) => format!("Key update failed: {error}"),
                    };
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('u'),
                ..
            }) => {
                if let Some(item) = items.get(selected) {
                    let name = item.name.clone();
                    let result = with_tui_suspended(stdout, || use_provider(&name));
                    status = match result {
                        Ok(()) => format!("Default provider: {name}"),
                        Err(error) => format!("Default update failed: {error}"),
                    };
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('d'),
                ..
            }) => {
                if let Some(item) = items.get(selected) {
                    let name = item.name.clone();
                    let result = with_tui_suspended(stdout, || remove_provider(&name, false));
                    status = match result {
                        Ok(()) => format!("Removed provider '{name}'"),
                        Err(error) => format!("Remove failed: {error}"),
                    };
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('t'),
                ..
            }) => {
                if let Some(item) = items.get(selected) {
                    let name = item.name.clone();
                    let result = with_tui_suspended(stdout, || test_provider(&name, false));
                    status = match result {
                        Ok(()) => format!("Provider '{name}' test succeeded"),
                        Err(error) => format!("Test failed: {error}"),
                    };
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn render_provider_tui(
    stdout: &mut io::Stdout,
    items: &[ProviderOutput],
    selected: usize,
    status: &str,
) -> Result<()> {
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
    writeln!(stdout, "anureo provider configuration")?;
    writeln!(stdout, "===========================")?;
    if items.is_empty() {
        writeln!(stdout, "No providers configured. Press 'a' to add one.")?;
    } else {
        writeln!(stdout, "  NAME                 MODEL              BASE URL")?;
        for (index, item) in items.iter().enumerate() {
            let marker = if index == selected { ">" } else { " " };
            let default_marker = if item.is_default { "*" } else { " " };
            writeln!(
                stdout,
                "{}{} {:<20} {:<18} {}",
                marker,
                default_marker,
                item.name,
                item.model.as_deref().unwrap_or("-"),
                item.base_url.as_deref().unwrap_or("models.dev")
            )?;
        }
        if let Some(item) = items.get(selected) {
            writeln!(stdout)?;
            writeln!(stdout, "Selected: {}", item.name)?;
            writeln!(stdout, "API key: {}", item.api_key)?;
        }
    }
    writeln!(stdout)?;
    writeln!(stdout, "Status: {status}")?;
    writeln!(stdout)?;
    writeln!(
        stdout,
        "[↑/↓] select  [a] add  [e] edit  [k] key  [u] default"
    )?;
    writeln!(stdout, "[t] test  [d] delete  [q/Esc] quit")?;
    stdout.flush()?;
    Ok(())
}

fn load_provider_outputs() -> Result<Vec<ProviderOutput>> {
    let document = load_document(&config_path())?;
    let default_name = document
        .get("default")
        .and_then(Value::as_table)
        .and_then(|table| table.get("provider"))
        .and_then(Value::as_str);
    providers(&document)?
        .iter()
        .map(|value| provider_output(value, default_name))
        .collect()
}

fn with_tui_suspended<T, F>(stdout: &mut io::Stdout, action: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    terminal::disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;
    let result = action();
    execute!(stdout, EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    result
}

fn add_provider_interactive() -> Result<String> {
    println!("Add provider");
    let name = prompt_line("Provider name: ")?;
    validate_name(&name)?;
    let provider_type = optional_prompt("Provider type (optional): ")?;
    let base_url = optional_prompt("Base URL (optional; blank uses models.dev): ")?;
    let model = optional_prompt("Default model (optional): ")?;
    let key = read_api_key(false)?;
    add_provider_values(
        &name,
        provider_type.as_deref(),
        base_url.as_deref(),
        model.as_deref(),
        &key,
    )?;
    Ok(name)
}

fn edit_provider_interactive(name: &str) -> Result<()> {
    println!("Edit provider '{name}'");
    let provider_type = prompt_line("Provider type (blank clears): ")?;
    let base_url = prompt_line("Base URL (blank clears; empty means models.dev): ")?;
    let model = prompt_line("Default model (blank clears): ")?;
    edit_provider_values(name, Some(&provider_type), Some(&base_url), Some(&model))
}

fn optional_prompt(prompt: &str) -> Result<Option<String>> {
    let value = prompt_line(prompt)?;
    Ok((!value.is_empty()).then_some(value))
}

fn add_provider_values(
    name: &str,
    provider_type: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
    api_key: &str,
) -> Result<()> {
    validate_name(name)?;
    let path = config_path();
    let mut document = load_document(&path)?;
    if providers(&document)?
        .iter()
        .any(|value| provider_name(value) == Some(name))
    {
        return Err(format!("provider '{}' already exists", name).into());
    }
    let mut table = Table::new();
    table.insert("name".into(), Value::String(name.to_string()));
    set_optional(&mut table, "type", provider_type);
    set_optional(&mut table, "base_url", base_url);
    set_optional(&mut table, "model", model);
    if !api_key.trim().is_empty() {
        table.insert("api_key".into(), Value::String(api_key.trim().to_string()));
    }
    document
        .entry("providers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("config.toml 'providers' must be an array of tables")?
        .push(Value::Table(table));
    write_document(&path, &document)
}

fn edit_provider_values(
    name: &str,
    provider_type: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    let path = config_path();
    let mut document = load_document(&path)?;
    let values = document
        .get_mut("providers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    let value = values
        .iter_mut()
        .find(|value| provider_name(value) == Some(name))
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    let table = value
        .as_table_mut()
        .ok_or("provider entry must be a TOML table")?;
    set_optional(table, "type", provider_type);
    set_optional(table, "base_url", base_url);
    set_optional(table, "model", model);
    write_document(&path, &document)
}

fn config_path() -> PathBuf {
    config::home::anureo_home().join("config.toml")
}

fn load_document(path: &Path) -> Result<Table> {
    if !path.exists() {
        return Ok(Table::new());
    }
    let content = fs::read_to_string(path)?;
    let value = content.parse::<Value>()?;
    let table = value.as_table().cloned().ok_or_else(|| {
        Box::<dyn std::error::Error>::from(io::Error::new(
            io::ErrorKind::InvalidData,
            "config.toml root must be a table",
        ))
    })?;
    Ok(table)
}

fn providers(document: &Table) -> Result<&[Value]> {
    match document.get("providers") {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        Some(_) => Err("config.toml 'providers' must be an array of tables".into()),
    }
}

fn provider_name(value: &Value) -> Option<&str> {
    value.get("name").and_then(Value::as_str)
}

fn find_provider<'a>(document: &'a Table, name: &str) -> Result<&'a Value> {
    providers(document)?
        .iter()
        .find(|provider| provider_name(provider) == Some(name))
        .ok_or_else(|| format!("provider '{}' not found", name).into())
}

fn provider_output(value: &Value, default_name: Option<&str>) -> Result<ProviderOutput> {
    let table = value
        .as_table()
        .ok_or("provider entry must be a TOML table")?;
    let name = table
        .get("name")
        .and_then(Value::as_str)
        .ok_or("provider is missing a name")?;
    let key = table.get("api_key").and_then(Value::as_str);
    Ok(ProviderOutput {
        name: name.to_string(),
        provider_type: table
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string),
        base_url: table
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        model: table
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        api_key: key
            .map(config::mask_value)
            .unwrap_or_else(|| "not configured".to_string()),
        is_default: default_name == Some(name),
    })
}

fn list_providers(json: bool) -> Result<()> {
    let path = config_path();
    let document = load_document(&path)?;
    let default_name = document
        .get("default")
        .and_then(Value::as_table)
        .and_then(|table| table.get("provider"))
        .and_then(Value::as_str);
    let values = providers(&document)?;
    let output: Vec<_> = values
        .iter()
        .map(|value| provider_output(value, default_name))
        .collect::<Result<Vec<_>>>()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    if output.is_empty() {
        println!("No providers configured in {}", path.display());
        return Ok(());
    }
    println!("{:<20} {:<18} {:<36} KEY", "NAME", "MODEL", "BASE URL");
    for item in output {
        println!(
            "{:<20} {:<18} {:<36} {}{}",
            if item.is_default {
                format!("* {}", item.name)
            } else {
                item.name
            },
            item.model.as_deref().unwrap_or("-"),
            item.base_url.as_deref().unwrap_or("models.dev"),
            item.api_key,
            if item.is_default { " (default)" } else { "" }
        );
    }
    Ok(())
}

fn show_provider(name: &str, json: bool) -> Result<()> {
    let document = load_document(&config_path())?;
    let default_name = document
        .get("default")
        .and_then(Value::as_table)
        .and_then(|table| table.get("provider"))
        .and_then(Value::as_str);
    let output = provider_output(find_provider(&document, name)?, default_name)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Name: {}", output.name);
        println!(
            "Type: {}",
            output.provider_type.as_deref().unwrap_or("inferred")
        );
        println!(
            "Base URL: {}",
            output.base_url.as_deref().unwrap_or("models.dev")
        );
        println!(
            "Model: {}",
            output.model.as_deref().unwrap_or("not configured")
        );
        println!("API key: {}", output.api_key);
        println!("Default: {}", if output.is_default { "yes" } else { "no" });
    }
    Ok(())
}

fn add_provider(args: &AddProviderArgs, json: bool) -> Result<()> {
    let name = match args.name.as_deref() {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => prompt_line("Provider name: ")?,
    };
    validate_name(&name)?;
    let path = config_path();
    let mut document = load_document(&path)?;
    if providers(&document)?
        .iter()
        .any(|value| provider_name(value) == Some(name.as_str()))
    {
        return Err(format!("provider '{}' already exists", name).into());
    }

    let mut table = Table::new();
    table.insert("name".into(), Value::String(name.clone()));
    set_optional(&mut table, "type", args.provider_type.as_deref());
    set_optional(&mut table, "base_url", args.base_url.as_deref());
    set_optional(&mut table, "model", args.model.as_deref());
    if !args.no_api_key {
        let key = read_api_key(args.api_key_stdin)?;
        if !key.is_empty() {
            table.insert("api_key".into(), Value::String(key));
        }
    }
    document
        .entry("providers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("config.toml 'providers' must be an array of tables")?
        .push(Value::Table(table));
    write_document(&path, &document)?;
    if json {
        show_provider(&name, true)
    } else {
        println!("Added provider '{}' to {}", name, path.display());
        Ok(())
    }
}

fn edit_provider(args: &EditProviderArgs, json: bool) -> Result<()> {
    let path = config_path();
    let mut document = load_document(&path)?;
    let values = document
        .get_mut("providers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("provider '{}' not found", args.name))?;
    let value = values
        .iter_mut()
        .find(|value| provider_name(value) == Some(args.name.as_str()))
        .ok_or_else(|| format!("provider '{}' not found", args.name))?;
    let table = value
        .as_table_mut()
        .ok_or("provider entry must be a table")?;
    set_optional(table, "type", args.provider_type.as_deref());
    set_optional(table, "base_url", args.base_url.as_deref());
    set_optional(table, "model", args.model.as_deref());
    write_document(&path, &document)?;
    if json {
        show_provider(&args.name, true)
    } else {
        println!("Updated provider '{}'", args.name);
        Ok(())
    }
}

fn set_provider_key(name: &str, from_stdin: bool) -> Result<()> {
    let path = config_path();
    let mut document = load_document(&path)?;
    let values = document
        .get_mut("providers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    let value = values
        .iter_mut()
        .find(|value| provider_name(value) == Some(name))
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    let key = read_api_key(from_stdin)?;
    if key.trim().is_empty() {
        return Err("API key cannot be empty".into());
    }
    value
        .as_table_mut()
        .ok_or("provider entry must be a table")?
        .insert("api_key".into(), Value::String(key));
    write_document(&path, &document)?;
    println!("Updated API key for provider '{}'", name);
    Ok(())
}

fn remove_provider(name: &str, yes: bool) -> Result<()> {
    if !yes && !confirm(&format!("Remove provider '{}' [y/N]? ", name))? {
        println!("Cancelled");
        return Ok(());
    }
    let path = config_path();
    let mut document = load_document(&path)?;
    let values = document
        .get_mut("providers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    let index = values
        .iter()
        .position(|value| provider_name(value) == Some(name))
        .ok_or_else(|| format!("provider '{}' not found", name))?;
    values.remove(index);
    if document
        .get("default")
        .and_then(Value::as_table)
        .and_then(|table| table.get("provider"))
        .and_then(Value::as_str)
        == Some(name)
    {
        if let Some(default) = document.get_mut("default").and_then(Value::as_table_mut) {
            default.remove("provider");
        }
    }
    write_document(&path, &document)?;
    println!("Removed provider '{}'", name);
    Ok(())
}

fn use_provider(name: &str) -> Result<()> {
    let path = config_path();
    let mut document = load_document(&path)?;
    find_provider(&document, name)?;
    let default = document
        .entry("default")
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .ok_or("config.toml 'default' must be a table")?;
    default.insert("provider".into(), Value::String(name.to_string()));
    write_document(&path, &document)?;
    println!("Default provider: {}", name);
    Ok(())
}

fn test_provider(name: &str, json: bool) -> Result<()> {
    let document = load_document(&config_path())?;
    let provider = find_provider(&document, name)?
        .as_table()
        .ok_or("provider entry must be a TOML table")?;
    let (base_url, base_url_source) = match provider.get("base_url").and_then(Value::as_str) {
        Some(url) if !url.trim().is_empty() => (url.trim().to_string(), "config.toml"),
        _ => (
            config::resolve_provider_base_url(name).ok_or_else(|| {
                format!(
                    "no base URL found for '{}' in config.toml or models.dev",
                    name
                )
            })?,
            "models.dev",
        ),
    };
    let key = provider.get("api_key").and_then(Value::as_str);
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut request = client.get(&url);
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }
    let result = match request.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            let success = response.status().is_success();
            let model_count = response
                .json::<Value>()
                .ok()
                .and_then(|body| body.get("data").and_then(Value::as_array).map(Vec::len));
            ProviderTestOutput {
                name: name.to_string(),
                base_url,
                base_url_source: base_url_source.to_string(),
                authenticated: key.is_some(),
                success,
                status: Some(status),
                model_count,
                error: if success {
                    None
                } else {
                    Some(format!("HTTP {}", status))
                },
            }
        }
        Err(error) => ProviderTestOutput {
            name: name.to_string(),
            base_url,
            base_url_source: base_url_source.to_string(),
            authenticated: key.is_some(),
            success: false,
            status: None,
            model_count: None,
            error: Some(error.to_string()),
        },
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Provider: {}", result.name);
        println!("Base URL: {} ({})", result.base_url, result.base_url_source);
        println!(
            "Authentication: {}",
            if result.authenticated {
                "configured"
            } else {
                "not configured"
            }
        );
        println!(
            "Connection: {}",
            if result.success { "success" } else { "failed" }
        );
        if let Some(status) = result.status {
            println!("HTTP status: {}", status);
        }
        if let Some(count) = result.model_count {
            println!("Models: {}", count);
        }
        if let Some(error) = result.error.as_deref() {
            println!("Error: {}", error);
        }
    }
    if result.success {
        Ok(())
    } else {
        Err(result
            .error
            .unwrap_or_else(|| "provider test failed".to_string())
            .into())
    }
}

fn set_optional(table: &mut Table, key: &str, value: Option<&str>) {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            table.insert(key.to_string(), Value::String(value.to_string()));
        }
        None => {
            if value.is_some() {
                table.remove(key);
            }
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return Err("provider name must be non-empty and contain no whitespace".into());
    }
    Ok(())
}

fn read_api_key(from_stdin: bool) -> Result<String> {
    if from_stdin {
        let mut value = String::new();
        io::stdin().read_to_string(&mut value)?;
        return Ok(value.trim().to_string());
    }
    Ok(rpassword::prompt_password("API key (hidden): ")?
        .trim()
        .to_string())
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}

fn confirm(prompt: &str) -> Result<bool> {
    Ok(prompt_line(prompt)?.eq_ignore_ascii_case("y"))
}

fn write_document(path: &Path, document: &Table) -> Result<()> {
    let home = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid config path"))?;
    fs::create_dir_all(home)?;
    let content = toml::to_string_pretty(&Value::Table(document.clone()))?;
    let temp = home.join(format!("config.toml.tmp-{}", std::process::id()));
    fs::write(&temp, content)?;
    if path.exists() {
        // Windows cannot rename over an existing file. The temporary file is
        // fully written before this replacement, so a failed rename leaves it
        // available for diagnosis instead of truncating config.toml.
        #[cfg(windows)]
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}
