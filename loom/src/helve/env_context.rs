//! Runtime environment context detection and prompt rendering.
//!
//! Collects OS, locale, shell, project language, and runtime information from the
//! current process environment. The detected [`EnvContext`] is injected into the
//! system prompt to help the agent adapt its behaviour (shell commands, reply
//! language, path format, etc.).
//!
//! # Usage
//!
//! ```ignore
//! let ctx = EnvContext::detect()
//!     .with_project(ProjectInfo::detect(&working_dir))
//!     .with_reply_language("中文");
//! let section = ctx.to_prompt_section();
//! ```

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct EnvContext {
    pub os: OsInfo,
    pub locale: LocaleInfo,
    pub shell: Option<ShellInfo>,
    pub project: Option<ProjectInfo>,
    pub runtime: Option<RuntimeInfo>,
}

impl Default for EnvContext {
    fn default() -> Self {
        Self {
            os: OsInfo::default(),
            locale: LocaleInfo::default(),
            shell: None,
            project: None,
            runtime: None,
        }
    }
}

impl EnvContext {
    pub fn detect() -> Self {
        Self {
            os: OsInfo::detect(),
            locale: LocaleInfo::detect(),
            shell: ShellInfo::detect(),
            runtime: Some(RuntimeInfo::detect()),
            project: None,
        }
    }

    pub fn with_reply_language(mut self, lang: impl Into<String>) -> Self {
        self.locale.preferred_reply_language = Some(lang.into());
        self
    }

    pub fn with_shell(mut self, shell: ShellInfo) -> Self {
        self.shell = Some(shell);
        self
    }

    pub fn with_project(mut self, project: ProjectInfo) -> Self {
        self.project = Some(project);
        self
    }

    pub fn with_chat_id(mut self, chat_id: i64) -> Self {
        if let Some(ref mut rt) = self.runtime {
            rt.chat_id = Some(chat_id);
        }
        self
    }

    pub fn to_prompt_section(&self) -> String {
        let mut lines: Vec<String> = Vec::new();

        lines.push(format!("- OS: {}", self.os.display_os()));
        lines.push(format!("- Locale: {}", self.locale.detected));

        if let Some(lang) = &self.locale.preferred_reply_language {
            lines.push(format!("- Reply language: {lang}"));
        }

        if let Some(sh) = &self.shell {
            match &sh.path {
                Some(p) => lines.push(format!("- Shell: {} ({p})", sh.name)),
                None => lines.push(format!("- Shell: {}", sh.name)),
            }
        }

        if let Some(proj) = &self.project {
            if !proj.languages.is_empty() {
                lines.push(format!("- Project languages: {}", proj.languages.join(", ")));
            }
            if proj.has_git {
                lines.push("- Git: yes".to_string());
            }
        }

        if let Some(rt) = &self.runtime {
            lines.push(format!("- Agent: {}", rt.agent_name));
            if rt.is_container {
                lines.push("- Container: yes".to_string());
            }
            if let Some(cid) = rt.chat_id {
                lines.push(format!("- Chat ID: {cid}"));
            }
        }

        format!("ENVIRONMENT:\n{}", lines.join("\n"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct OsInfo {
    pub family: String,
    pub version: Option<String>,
    pub arch: String,
}

impl OsInfo {
    pub fn detect() -> Self {
        let family = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let version = detect_os_version();
        Self {
            family,
            version,
            arch,
        }
    }

    fn display_os(&self) -> String {
        match (&self.version, &self.arch) {
            (Some(v), a) if !a.is_empty() => format!("{} ({v}, {a})", self.family),
            (Some(v), _) => format!("{} ({v})", self.family),
            (None, a) if !a.is_empty() => format!("{} ({a})", self.family),
            _ => self.family.clone(),
        }
    }
}

fn detect_os_version() -> Option<String> {
    match std::env::consts::OS {
        "macos" => {
            let output = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()?;
            if output.status.success() {
                let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
            None
        }
        "linux" => {
            let content = std::fs::read_to_string("/etc/os-release").ok()?;
            for line in content.lines() {
                if let Some(v) = line.strip_prefix("VERSION=") {
                    return Some(v.trim_matches('"').to_string());
                }
                if let Some(v) = line.strip_prefix("VERSION_ID=") {
                    return Some(v.trim_matches('"').to_string());
                }
            }
            None
        }
        "windows" => {
            let v = std::env::var("PROCESSOR_ARCHITECTURE").ok()?;
            Some(v)
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct LocaleInfo {
    pub detected: String,
    pub language: String,
    pub preferred_reply_language: Option<String>,
}

impl Default for LocaleInfo {
    fn default() -> Self {
        Self {
            detected: "en_US.UTF-8".to_string(),
            language: "en_US".to_string(),
            preferred_reply_language: None,
        }
    }
}

impl LocaleInfo {
    pub fn detect() -> Self {
        let detected = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LANGUAGE"))
            .unwrap_or_else(|_| "en_US.UTF-8".to_string());
        let language = detected.split('.').next().unwrap_or("en_US").to_string();
        Self {
            detected,
            language,
            preferred_reply_language: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellInfo {
    pub name: String,
    pub path: Option<String>,
}

impl ShellInfo {
    pub fn detect() -> Option<Self> {
        if let Ok(shell_path) = std::env::var("SHELL") {
            let name = shell_name_from_path(&shell_path);
            return Some(Self {
                name,
                path: Some(shell_path),
            });
        }
        if cfg!(target_os = "windows") {
            if let Ok(comspec) = std::env::var("COMSPEC") {
                let name = shell_name_from_path(&comspec);
                return Some(Self {
                    name,
                    path: Some(comspec),
                });
            }
        }
        None
    }
}

fn shell_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

const EXTENSION_LANGUAGE_MAP: &[(&str, &str)] = &[
    ("c", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("go", "go"),
    ("h", "c"),
    ("hpp", "cpp"),
    ("java", "java"),
    ("js", "javascript"),
    ("jsx", "javascript"),
    ("kt", "kotlin"),
    ("lua", "lua"),
    ("php", "php"),
    ("py", "python"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("swift", "swift"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("zig", "zig"),
];

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".next",
    ".nuxt",
    "bazel-out",
];

const MIN_FILE_THRESHOLD: usize = 3;

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub languages: Vec<String>,
    pub has_git: bool,
}

impl ProjectInfo {
    pub fn detect(working_dir: &Path) -> Self {
        let mut lang_counts: HashMap<&str, usize> = HashMap::new();

        Self::visit(working_dir, 0, &mut lang_counts);

        let mut languages: Vec<(String, usize)> = lang_counts
            .into_iter()
            .filter(|(_, count)| *count >= MIN_FILE_THRESHOLD)
            .map(|(lang, count)| (lang.to_string(), count))
            .collect();
        languages.sort_by(|a, b| b.1.cmp(&a.1));
        let languages: Vec<String> = languages.into_iter().map(|(l, _)| l).collect();

        let has_git = working_dir.join(".git").exists();

        Self {
            languages,
            has_git,
        }
    }

    fn visit(dir: &Path, depth: usize, lang_counts: &mut HashMap<&str, usize>) {
        if depth > 2 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || SKIP_DIRS.contains(&name) {
                        continue;
                    }
                    Self::visit(&path, depth + 1, lang_counts);
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Ok(idx) = EXTENSION_LANGUAGE_MAP.binary_search_by_key(
                    &ext.to_lowercase().as_str(),
                    |(e, _)| *e,
                ) {
                    *lang_counts.entry(EXTENSION_LANGUAGE_MAP[idx].1).or_insert(0) += 1;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeInfo {
    pub agent_name: String,
    pub is_container: bool,
    pub chat_id: Option<i64>,
}

impl RuntimeInfo {
    #[cfg(unix)]
    pub fn detect() -> Self {
        let is_container = Path::new("/.dockerenv").exists()
            || std::fs::read_to_string("/proc/1/cgroup")
                .map(|c| c.contains("docker") || c.contains("kubepods"))
                .unwrap_or(false);
        Self {
            agent_name: "Loom".to_string(),
            is_container,
            chat_id: None,
        }
    }

    #[cfg(not(unix))]
    pub fn detect() -> Self {
        Self {
            agent_name: "Loom".to_string(),
            is_container: false,
            chat_id: None,
        }
    }

    pub fn with_chat_id(mut self, chat_id: i64) -> Self {
        self.chat_id = Some(chat_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn os_info_detect() {
        let os = OsInfo::detect();
        assert!(!os.family.is_empty());
        assert!(!os.arch.is_empty());
        assert!(["macos", "linux", "windows"].contains(&os.family.as_str()));
    }

    #[test]
    fn os_info_display_os() {
        let os = OsInfo {
            family: "macos".to_string(),
            version: Some("14.5".to_string()),
            arch: "aarch64".to_string(),
        };
        assert_eq!(os.display_os(), "macos (14.5, aarch64)");

        let os = OsInfo {
            family: "linux".to_string(),
            version: None,
            arch: "x86_64".to_string(),
        };
        assert_eq!(os.display_os(), "linux (x86_64)");

        let os = OsInfo {
            family: "windows".to_string(),
            version: None,
            arch: String::new(),
        };
        assert_eq!(os.display_os(), "windows");
    }

    #[test]
    fn locale_info_detect() {
        let locale = LocaleInfo::detect();
        assert!(!locale.detected.is_empty());
        assert!(!locale.language.is_empty());
    }

    #[test]
    fn shell_info_detect() {
        let sh = ShellInfo::detect();
        if let Some(sh) = sh {
            assert!(!sh.name.is_empty());
        }
    }

    #[test]
    fn runtime_info_detect() {
        let rt = RuntimeInfo::detect();
        assert_eq!(rt.agent_name, "Loom");
    }

    #[test]
    fn extension_map_is_sorted() {
        for window in EXTENSION_LANGUAGE_MAP.windows(2) {
            assert!(
                window[0].0 <= window[1].0,
                "EXTENSION_LANGUAGE_MAP not sorted: {} > {}",
                window[0].0,
                window[1].0,
            );
        }
    }

    #[test]
    fn project_info_detect_rust() {
        let dir = std::env::temp_dir().join("loom_test_project_rust");
        let _ = fs::create_dir_all(&dir);
        for i in 0..3 {
            fs::write(dir.join(format!("main_{i}.rs")), "").unwrap();
        }
        let proj = ProjectInfo::detect(&dir);
        assert!(proj.languages.contains(&"rust".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_info_below_threshold_not_reported() {
        let dir = std::env::temp_dir().join("loom_test_project_small");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("single.py"), "").unwrap();
        let proj = ProjectInfo::detect(&dir);
        assert!(!proj.languages.contains(&"python".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_info_skip_dirs_ignored() {
        let dir = std::env::temp_dir().join("loom_test_project_skip");
        let node_modules = dir.join("node_modules");
        let _ = fs::create_dir_all(&node_modules);
        for i in 0..5 {
            fs::write(node_modules.join(format!("dep_{i}.js")), "").unwrap();
        }
        let proj = ProjectInfo::detect(&dir);
        assert!(!proj.languages.contains(&"javascript".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_context_to_prompt_minimal() {
        let ctx = EnvContext {
            runtime: Some(RuntimeInfo::default()),
            ..Default::default()
        };
        let s = ctx.to_prompt_section();
        assert!(s.starts_with("ENVIRONMENT:"));
        assert!(s.contains("- OS:"));
        assert!(s.contains("- Locale:"));
        assert!(s.contains("- Agent:"));
        assert!(!s.contains("Shell:"));
        assert!(!s.contains("Project languages:"));
        assert!(!s.contains("Reply language:"));
        assert!(!s.contains("Container:"));
    }

    #[test]
    fn env_context_to_prompt_full() {
        let ctx = EnvContext {
            os: OsInfo {
                family: "macos".to_string(),
                version: Some("Darwin 24.6.0".to_string()),
                arch: "aarch64".to_string(),
            },
            locale: LocaleInfo {
                detected: "zh_CN.UTF-8".to_string(),
                language: "zh_CN".to_string(),
                preferred_reply_language: Some("中文".to_string()),
            },
            shell: Some(ShellInfo {
                name: "zsh".to_string(),
                path: Some("/bin/zsh".to_string()),
            }),
            project: Some(ProjectInfo {
                languages: vec!["rust".to_string(), "typescript".to_string()],
                has_git: true,
            }),
            runtime: Some(RuntimeInfo {
                agent_name: "Loom".to_string(),
                is_container: false,
                chat_id: None,
            }),
        };
        let s = ctx.to_prompt_section();
        assert!(s.contains("- OS: macos (Darwin 24.6.0, aarch64)"));
        assert!(s.contains("- Locale: zh_CN.UTF-8"));
        assert!(s.contains("- Reply language: 中文"));
        assert!(s.contains("- Shell: zsh (/bin/zsh)"));
        assert!(s.contains("- Project languages: rust, typescript"));
        assert!(s.contains("- Git: yes"));
        assert!(s.contains("- Agent: Loom"));
        assert!(!s.contains("Container:"));
    }

    #[test]
    fn with_reply_language() {
        let ctx = EnvContext::default().with_reply_language("中文");
        assert_eq!(ctx.locale.preferred_reply_language, Some("中文".to_string()));
        let s = ctx.to_prompt_section();
        assert!(s.contains("- Reply language: 中文"));
    }

    #[test]
    fn with_project() {
        let ctx = EnvContext::default().with_project(ProjectInfo {
            languages: vec!["go".to_string()],
            has_git: true,
        });
        let s = ctx.to_prompt_section();
        assert!(s.contains("- Project languages: go"));
        assert!(s.contains("- Git: yes"));
    }
}
