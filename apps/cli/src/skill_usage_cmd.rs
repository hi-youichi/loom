//! Handler for `loom skill-usage` CLI command.
//!
//! Syncs, shows, and repairs `.usage.json` files in the skills directory.

use std::collections::HashMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::args::{SkillUsageCommand, SkillUsageArgs};
use loom_curator::skill_registry::SkillRegistry;
use loom_curator::{SkillUsage, SkillUsageStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub scanned_count: usize,
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: usize,
    pub path: String,
}

/// Main entry point for the skill-usage command.
pub async fn handle_skill_usage_command(args: &SkillUsageArgs) -> Result<(), Box<dyn std::error::Error>> {
    match &args.sub {
        SkillUsageCommand::Sync { path, dry_run, json, source: _ } => {
            sync(path, *dry_run, *json).await
        }
        SkillUsageCommand::Show { name, json } => {
            show(name, *json).await
        }
        SkillUsageCommand::Repair { path } => {
            repair(path).await
        }
    }
}

async fn sync(
    path: &Option<std::path::PathBuf>,
    dry_run: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_dir = path
        .clone()
        .unwrap_or_else(loom_curator::skill_registry::default_path);
    let base_dir_str = base_dir.display().to_string();

    // Scan skills directory
    let registry = SkillRegistry::new(&base_dir);
    let skills = registry.list()?;

    // Load existing .usage.json
    let store = SkillUsageStore::new(&base_dir);
    let existing_data = store.load().unwrap_or_default();

    let mut created = Vec::new();
    let updated: Vec<String> = Vec::new();
    let mut unchanged_count = 0;

    // Merge skills with existing data
    let _merged: HashMap<String, SkillUsage> = skills
        .into_iter()
        .map(|skill| {
            let name = skill.name.clone();

            match existing_data.get(&name) {
                Some(_existing) => {
                    unchanged_count += 1;
                    (name.clone(), existing_data.get(&name).unwrap().clone())
                }
                None => {
                    // Create new entry
                    let new_entry = SkillUsage::new(&name);
                    if !dry_run {
                        let _ = store.save_entry(&name, &new_entry);
                    }
                    created.push(name.clone());
                    (name, new_entry)
                }
            }
        })
        .collect();

    let scanned_count = created.len() + updated.len() + unchanged_count;

    if json {
        let result = SyncResult {
            scanned_count,
            created,
            updated,
            unchanged: unchanged_count,
            path: base_dir_str,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Scanned {} skills from {}", scanned_count, base_dir_str);
        if !created.is_empty() {
            println!("Created: {}", created.join(", "));
        }
        if !updated.is_empty() {
            println!("Updated: {}", updated.join(", "));
        }
        if created.is_empty() && updated.is_empty() {
            println!("All skills are up to date.");
        } else {
            println!(
                "Total: {} created, {} updated, {} unchanged",
                created.len(),
                updated.len(),
                unchanged_count
            );
        }
    }

    if dry_run {
        println!("(dry-run: no changes written)");
    }

    Ok(())
}

async fn show(
    name: &Option<String>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_dir = loom_curator::skill_registry::default_path();
    let store = SkillUsageStore::new(&base_dir);

    let data = store.load().unwrap_or_default();

    if let Some(skill_name) = name {
        match data.get(skill_name) {
            Some(usage) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(usage)?);
                } else {
                    print_usage_detail(skill_name, usage);
                }
            }
            None => {
                eprintln!("Skill not found in .usage.json: {}", skill_name);
                std::process::exit(1);
            }
        }
    } else if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        print_usage_list(&data);
    }

    Ok(())
}

async fn repair(
    path: &Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_dir = path
        .clone()
        .unwrap_or_else(loom_curator::skill_registry::default_path);
    let usage_path = base_dir.join(".usage.json");

    if !usage_path.exists() {
        println!(
            "No .usage.json found at {}. Nothing to repair.",
            usage_path.display()
        );
        return Ok(());
    }

    // Try to load existing data
    match fs::read_to_string(&usage_path) {
        Ok(content) => {
            if content.trim().is_empty() {
                println!(".usage.json is empty. Reinitializing with empty object.");
            } else {
                match serde_json::from_str::<HashMap<String, SkillUsage>>(&content) {
                    Ok(_) => {
                        println!(".usage.json is valid. No repair needed.");
                        return Ok(());
                    }
                    Err(e) => {
                        println!(".usage.json is corrupted: {}", e);
                        println!("Attempting to recover valid entries...");

                        // Backup original
                        let backup_path = usage_path.with_extension("bak");
                        fs::copy(&usage_path, &backup_path)?;
                        println!("Backed up to: {}", backup_path.display());

                        // Try to parse line by line or extract valid portions
                        let recovered = try_recover_json(&content);
                        let store = SkillUsageStore::new(&base_dir);
                        store.save(&recovered)?;

                        println!("Recovered {} valid entries.", recovered.len());
                        return Ok(());
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to read .usage.json: {}", e);
            println!("Attempting to recover...");

            // Try to read as bytes and look for JSON
            let bytes = fs::read(&usage_path)?;
            let content = String::from_utf8_lossy(&bytes);
            let recovered = try_recover_json(&content);
            let store = SkillUsageStore::new(&base_dir);
            store.save(&recovered)?;

            println!("Recovered {} valid entries.", recovered.len());
        }
    }

    // Reinitialize if empty
    let store = SkillUsageStore::new(&base_dir);
    store.save(&HashMap::new())?;
    println!("Reinitialized .usage.json with empty object.");

    Ok(())
}

/// Try to recover valid JSON entries from corrupted content.
fn try_recover_json(content: &str) -> HashMap<String, SkillUsage> {
    let mut recovered = HashMap::new();

    // Try direct parsing first
    if let Ok(data) = serde_json::from_str::<HashMap<String, SkillUsage>>(content) {
        return data;
    }

    // Try to extract individual skill entries using manual parsing
    // Look for patterns like "skill-name": { ... }
    for line in content.lines() {
        if let Some(name_end) = line.find("\":") {
            let name_start = if line.starts_with('"') { 1 } else { 0 };
            let skill_name = &line[name_start..name_end];
            if let Ok(usage) = serde_json::from_str::<SkillUsage>(&line[name_end + 2..]) {
                recovered.insert(skill_name.to_string(), usage);
            }
        }
    }

    recovered
}

fn print_usage_detail(name: &str, usage: &SkillUsage) {
    println!("Skill: {}", name);
    println!("{}", "═".repeat(60));
    println!("Use count: {}", usage.use_count);
    println!("View count: {}", usage.view_count);
    println!("Patch count: {}", usage.patch_count);
    if let Some(last_used) = &usage.last_used_at {
        println!("Last used: {}", last_used);
    }
    if let Some(last_viewed) = &usage.last_viewed_at {
        println!("Last viewed: {}", last_viewed);
    }
    println!("Created at: {}", usage.created_at);
    println!("State: {:?}", usage.state);
    if usage.pinned {
        println!("Pinned: yes");
    }
}

fn print_usage_list(data: &HashMap<String, SkillUsage>) {
    if data.is_empty() {
        println!("No usage data found.");
        return;
    }

    println!("Skill Usage (.usage.json):");
    println!("{}", "─".repeat(60));

    let mut names: Vec<_> = data.keys().collect();
    names.sort();

    for name in names {
        if let Some(usage) = data.get(name) {
            println!(
                "  • {} — uses: {}, views: {}, patches: {}",
                name, usage.use_count, usage.view_count, usage.patch_count
            );
            if let Some(last_used) = &usage.last_used_at {
                println!("    Last used: {}", last_used);
            }
        }
    }

    println!("\nTotal: {} skills tracked", data.len());
}
