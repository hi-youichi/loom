use std::path::{Path, PathBuf};

pub fn resolve_workflow(name: &str, working_folder: &Path) -> Result<PathBuf, String> {
    if name.ends_with(".lua") && Path::new(name).is_absolute() {
        let p = PathBuf::from(name);
        if p.exists() {
            return Ok(p);
        }
    }

    let candidates = [
        working_folder
            .join(".luft")
            .join("workflows")
            .join(format!("{name}.lua")),
        dirs_home()
            .join(".config")
            .join("luft")
            .join("workflows")
            .join(format!("{name}.lua")),
        working_folder.join(format!("{name}.lua")),
        PathBuf::from(name),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    Err(format!(
        "Workflow '{}' not found. Searched: {}",
        name,
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolve_workflow_from_luft_dir() {
        let dir = tempdir().unwrap();
        let wf_dir = dir.path().join(".luft").join("workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(wf_dir.join("my_flow.lua"), "function main() end").unwrap();

        let result = resolve_workflow("my_flow", dir.path());
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains("my_flow.lua"));
    }

    #[test]
    fn resolve_workflow_from_working_folder() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("local.lua"), "function main() end").unwrap();

        let result = resolve_workflow("local", dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_workflow_absolute_path() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("abs.lua");
        fs::write(&file, "function main() end").unwrap();

        let result = resolve_workflow(file.to_str().unwrap(), dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_workflow_not_found() {
        let dir = tempdir().unwrap();
        let result = resolve_workflow("nonexistent", dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
