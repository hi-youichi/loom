//! Find types (structs, enums, traits, type aliases) that are defined but never referenced.
//!
//! Uses rust-analyzer via LSP to:
//! 1. Enumerate all type definitions in .rs files
//! 2. Call findReferences for each type
//! 3. Report types whose only reference is the declaration itself
//!
//! Usage:
//!   cargo run --example find_unused_types
//!   cargo run --example find_unused_types -- --dir agent/tool
//!   cargo run --example find_unused_types -- --warmup 60
//!   cargo run --example find_unused_types -- --json > unused.json
//!   cargo run --example find_unused_types -- --limit 50

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp::LspClient;
use lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolKind, Url};

/// Wrap Url::from_file_path's `()` error into a Box<dyn Error>.
fn to_url(path: &Path) -> Result<Url, Box<dyn std::error::Error>> {
    Url::from_file_path(path).map_err(|()| format!("invalid file path: {}", path.display()).into())
}

// ─── Data structures ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct TypeSymbol {
    name: String,
    kind: SymbolKind,
    line: u32,
    character: u32,
    file: PathBuf,
}

#[derive(Clone, Debug)]
struct UnusedType {
    name: String,
    kind_label: String,
    file: PathBuf,
    line: u32,
}

// ─── Symbol classification ────────────────────────────────────────────────

fn is_type_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::STRUCT
            | SymbolKind::ENUM
            | SymbolKind::INTERFACE
            | SymbolKind::TYPE_PARAMETER
            | SymbolKind::CLASS
    )
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::STRUCT => "struct",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "trait",
        SymbolKind::TYPE_PARAMETER => "type_alias",
        SymbolKind::CLASS => "class",
        _ => "other",
    }
}

// ─── File walking ─────────────────────────────────────────────────────────

fn collect_rs_files(dir: &Path, skip_dirs: &HashSet<&str>, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if skip_dirs.contains(name) {
                continue;
            }
            collect_rs_files(&path, skip_dirs, files);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            files.push(path);
        }
    }
}

// ─── Symbol extraction ───────────────────────────────────────────────────

fn collect_type_symbols_from_nested(
    symbols: &[DocumentSymbol],
    file_path: &Path,
    results: &mut Vec<TypeSymbol>,
) {
    for sym in symbols {
        if is_type_kind(sym.kind) {
            results.push(TypeSymbol {
                name: sym.name.clone(),
                kind: sym.kind,
                line: sym.selection_range.start.line,
                character: sym.selection_range.start.character,
                file: file_path.to_path_buf(),
            });
        }
        if let Some(children) = &sym.children {
            collect_type_symbols_from_nested(children, file_path, results);
        }
    }
}

// ─── JSON output ─────────────────────────────────────────────────────────

fn output_json(workspace_root: &Path, unused: &[UnusedType], total_checked: usize) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let _ = writeln!(handle, "{{");
    let _ = writeln!(handle, "  \"total_checked\": {},", total_checked);
    let _ = writeln!(handle, "  \"unused_count\": {},", unused.len());
    let _ = writeln!(handle, "  \"unused_types\": [");
    for (i, ty) in unused.iter().enumerate() {
        let rel = ty
            .file
            .strip_prefix(workspace_root)
            .unwrap_or(&ty.file)
            .display()
            .to_string()
            .replace('\\', "/");
        let comma = if i + 1 < unused.len() { "," } else { "" };
        let _ = writeln!(
            handle,
            "    {{\"name\": \"{}\", \"kind\": \"{}\", \"file\": \"{}\", \"line\": {}}}{comma}",
            ty.name,
            ty.kind_label,
            rel,
            ty.line + 1
        );
    }
    let _ = writeln!(handle, "  ]");
    let _ = writeln!(handle, "}}");
}

// ─── Main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Parse CLI args ──
    let args: Vec<String> = std::env::args().collect();
    let mut filter_dir: Option<String> = None;
    let mut warmup_secs: u64 = 45;
    let mut json_output = false;
    let mut limit: Option<usize> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                i += 1;
                filter_dir = args.get(i).cloned();
            }
            "--warmup" => {
                i += 1;
                warmup_secs = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(45);
            }
            "--json" => json_output = true,
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|s| s.parse().ok());
            }
            "--help" | "-h" => {
                println!("Usage: find_unused_types [--dir <subdir>] [--warmup <secs>] [--json] [--limit <n>]");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    let workspace_root = std::env::current_dir()?;

    if !json_output {
        println!("Workspace: {}", workspace_root.display());
    }

    // ── Check rust-analyzer availability ──
    let ra_check = std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output();
    if ra_check.is_err() {
        eprintln!("Error: rust-analyzer not found. Install: rustup component add rust-analyzer");
        std::process::exit(1);
    }

    // ── Start rust-analyzer ──
    let root_uri = to_url(&workspace_root)?;
    if !json_output {
        println!("Starting rust-analyzer...");
    }

    let mut client = LspClient::start("rust-analyzer", &[], Some(root_uri)).await?;
    client.initialize().await?;

    if !json_output {
        println!("rust-analyzer initialized.");
    }

    // ── Collect .rs files ──
    let skip_dirs: HashSet<&str> = [
        "target",
        ".git",
        "thirdparty",
        "node_modules",
        ".loom",
        "coverage",
        "logs",
        ".cargo",
        ".claude",
    ]
    .iter()
    .copied()
    .collect();

    let search_root = match &filter_dir {
        Some(d) => workspace_root.join(d),
        None => workspace_root.clone(),
    };

    let mut files = Vec::new();
    collect_rs_files(&search_root, &skip_dirs, &mut files);
    files.sort();

    if !json_output {
        println!("Found {} .rs files to analyze", files.len());
    }

    // ── Open all files ──
    let mut opened = 0usize;
    for file in &files {
        if let Ok(content) = std::fs::read_to_string(file) {
            let uri = to_url(file)?;
            client.open_document(&uri, "rust", &content).await?;
            opened += 1;
        }
    }
    if !json_output {
        println!("Opened {} documents", opened);
        println!("Waiting {}s for indexing...", warmup_secs);
    }

    tokio::time::sleep(Duration::from_secs(warmup_secs)).await;

    // ── Phase 1: Collect type definitions ──
    if !json_output {
        println!("\n=== Phase 1: Collecting type definitions ===");
    }

    let phase1_start = Instant::now();
    let mut all_types = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let uri = to_url(file)?;
        match client.document_symbols(&uri).await {
            Ok(Some(DocumentSymbolResponse::Nested(symbols))) => {
                collect_type_symbols_from_nested(&symbols, file, &mut all_types);
            }
            Ok(Some(DocumentSymbolResponse::Flat(symbols))) => {
                for sym in symbols {
                    if is_type_kind(sym.kind) {
                        all_types.push(TypeSymbol {
                            name: sym.name.clone(),
                            kind: sym.kind,
                            line: sym.location.range.start.line,
                            character: sym.location.range.start.character,
                            file: file.clone(),
                        });
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                if !json_output {
                    eprintln!(
                        "  [WARN] document_symbols failed for {}: {}",
                        file.display(),
                        e
                    );
                }
            }
        }
        if !json_output && (idx + 1) % 50 == 0 {
            println!(
                "  Scanned {}/{} files, {} type defs found",
                idx + 1,
                files.len(),
                all_types.len()
            );
        }
    }

    if !json_output {
        println!(
            "Phase 1 done: {} type definitions in {:.2}s",
            all_types.len(),
            phase1_start.elapsed().as_secs_f64()
        );
    }

    // Apply --limit if specified (useful for quick tests)
    if let Some(n) = limit {
        all_types.truncate(n);
        if !json_output {
            println!("(limited to first {} types for --limit)", all_types.len());
        }
    }

    // ── Phase 2: Find references for each type ──
    if !json_output {
        println!("\n=== Phase 2: Checking references (findReferences) ===");
    }

    let phase2_start = Instant::now();
    let mut unused_types: Vec<UnusedType> = Vec::new();
    let total = all_types.len();

    for (idx, ty) in all_types.iter().enumerate() {
        let uri = to_url(&ty.file)?;

        // find_references uses includeDeclaration: true
        // → unused means refs.len() <= 1 (only the declaration itself)
        match client.find_references(&uri, ty.line, ty.character).await {
            Ok(refs) => {
                if refs.len() <= 1 {
                    unused_types.push(UnusedType {
                        name: ty.name.clone(),
                        kind_label: kind_label(ty.kind).to_string(),
                        file: ty.file.clone(),
                        line: ty.line,
                    });
                }
            }
            Err(e) => {
                if !json_output {
                    eprintln!(
                        "  [WARN] findReferences failed for {} ({}:{}): {}",
                        ty.name,
                        ty.file.display(),
                        ty.line + 1,
                        e
                    );
                }
            }
        }

        if !json_output && (idx + 1) % 20 == 0 {
            let elapsed = phase2_start.elapsed().as_secs_f64();
            let rate = (idx + 1) as f64 / elapsed.max(0.001);
            let remaining = ((total - idx - 1) as f64 / rate).round() as u64;
            println!(
                "  [{}/{}] checked, {} unused — {:.0}s elapsed, ~{}s remaining",
                idx + 1,
                total,
                unused_types.len(),
                elapsed,
                remaining
            );
        }
    }

    let phase2_elapsed = phase2_start.elapsed();

    // ── Output results ──
    if json_output {
        output_json(&workspace_root, &unused_types, total);
    } else {
        println!("\nPhase 2 done in {:.2}s\n", phase2_elapsed.as_secs_f64());

        println!("{}", "=".repeat(72));
        println!("UNUSED TYPES REPORT");
        println!("{}", "=".repeat(72));

        // Sort: by kind, then file, then line
        unused_types.sort_by_cached_key(|a| (a.kind_label.clone(), a.file.clone(), a.line));

        // Group by kind
        let mut current_kind = "";
        for ty in &unused_types {
            if ty.kind_label != current_kind {
                current_kind = &ty.kind_label;
                println!("\n── {} ──", current_kind);
            }
            let rel = ty.file.strip_prefix(&workspace_root).unwrap_or(&ty.file);
            println!("  {}:{}  {}", rel.display(), ty.line + 1, ty.name);
        }

        let pct = if total == 0 {
            0.0
        } else {
            unused_types.len() as f64 / total as f64 * 100.0
        };

        println!("\n{}", "─".repeat(72));
        println!(
            "Summary: {}/{} types unused ({:.1}%)  |  Phase 2: {:.1}s",
            unused_types.len(),
            total,
            pct,
            phase2_elapsed.as_secs_f64()
        );
        println!("Note: 'pub' types may be part of the public API and used externally.");
    }

    // ── Shutdown ──
    let _ = client.shutdown().await;

    Ok(())
}
