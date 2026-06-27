//! LSP language server installer and validator.
//!
//! Provides automatic detection, validation, and installation prompts for language servers.

use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("Language server not found: {0}")]
    ServerNotFound(String),

    #[error("Failed to check installation: {0}")]
    CheckFailed(String),

    #[error("Installation failed: {0}")]
    InstallationFailed(String),

    #[error("Unsupported language for auto-install: {0}")]
    UnsupportedLanguage(String),
}

/// Information about a language server installation.
#[derive(Debug, Clone)]
pub struct ServerInstallation {
    pub language: String,
    pub server_name: String,
    pub is_installed: bool,
    pub executable_path: Option<PathBuf>,
    pub version: Option<String>,
    pub install_command: Option<String>,
}

/// Language server installer and validator.
pub struct LspInstaller {
    servers: Vec<ServerDefinition>,
}

/// Definition of a language server.
struct ServerDefinition {
    language: String,
    server_name: String,
    executable: String,
    check_args: Vec<String>,
    install_commands: Vec<String>,
    /// Package managers supported for this language server (metadata only).
    /// Used by `test_installer_all_servers_have_package_managers` to verify
    /// every server definition declares at least one package manager.
    #[allow(dead_code)]
    package_managers: Vec<String>,
}

impl LspInstaller {
    /// Create a new installer with default server definitions.
    pub fn new() -> Self {
        Self {
            servers: vec![
                // Rust
                ServerDefinition {
                    language: "rust".to_string(),
                    server_name: "rust-analyzer".to_string(),
                    executable: "rust-analyzer".to_string(),
                    check_args: vec!["--version".to_string()],
                    install_commands: vec!["rustup component add rust-analyzer".to_string()],
                    package_managers: vec!["rustup".to_string()],
                },
                // TypeScript
                ServerDefinition {
                    language: "typescript".to_string(),
                    server_name: "typescript-language-server".to_string(),
                    executable: "typescript-language-server".to_string(),
                    check_args: vec!["--version".to_string()],
                    install_commands: vec![
                        "npm install -g typescript-language-server typescript".to_string()
                    ],
                    package_managers: vec!["npm".to_string()],
                },
                // Python
                ServerDefinition {
                    language: "python".to_string(),
                    server_name: "pylsp".to_string(),
                    executable: "pylsp".to_string(),
                    check_args: vec!["--version".to_string()],
                    install_commands: vec!["pip install python-lsp-server".to_string()],
                    package_managers: vec!["pip".to_string()],
                },
                // Go
                ServerDefinition {
                    language: "go".to_string(),
                    server_name: "gopls".to_string(),
                    executable: "gopls".to_string(),
                    check_args: vec!["version".to_string()],
                    install_commands: vec!["go install golang.org/x/tools/gopls@latest".to_string()],
                    package_managers: vec!["go".to_string()],
                },
                // C++
                ServerDefinition {
                    language: "cpp".to_string(),
                    server_name: "clangd".to_string(),
                    executable: "clangd".to_string(),
                    check_args: vec!["--version".to_string()],
                    install_commands: vec![
                        "brew install llvm".to_string(), // macOS
                    ],
                    package_managers: vec!["brew".to_string()],
                },
                // Java
                ServerDefinition {
                    language: "java".to_string(),
                    server_name: "eclipse-jdtls".to_string(),
                    executable: "jdtls".to_string(),
                    check_args: vec!["--version".to_string()],
                    install_commands: vec![
                        "brew install jdtls".to_string(),          // macOS
                        "pip install jdtls".to_string(),           // 跨平台 Python 包
                        "choco install jdtls".to_string(),         // Windows
                    ],
                    package_managers: vec!["brew".to_string(), "pip".to_string(), "choco".to_string()],
                },
            ],
        }
    }

    /// Check if a language server is installed.
    pub fn check_installation(&self, language: &str) -> Result<ServerInstallation, InstallerError> {
        let server = self
            .servers
            .iter()
            .find(|s| s.language == language)
            .ok_or_else(|| InstallerError::UnsupportedLanguage(language.to_string()))?;

        let output = Command::new(&server.executable)
            .args(&server.check_args)
            .output();

        match output {
            Ok(output) => {
                let version = if output.status.success() {
                    String::from_utf8_lossy(&output.stdout).trim().to_string()
                } else {
                    String::from_utf8_lossy(&output.stderr).trim().to_string()
                };

                let executable_path = which::which(&server.executable).ok();

                Ok(ServerInstallation {
                    language: server.language.clone(),
                    server_name: server.server_name.clone(),
                    is_installed: output.status.success(),
                    executable_path,
                    version: Some(version),
                    install_command: server.install_commands.first().cloned(),
                })
            }
            Err(e) => {
                info!("Language server {} not found: {}", server.server_name, e);
                Ok(ServerInstallation {
                    language: server.language.clone(),
                    server_name: server.server_name.clone(),
                    is_installed: false,
                    executable_path: None,
                    version: None,
                    install_command: server.install_commands.first().cloned(),
                })
            }
        }
    }

    /// Check all language servers and return their installation status.
    pub fn check_all(&self) -> Vec<ServerInstallation> {
        self.servers
            .iter()
            .filter_map(|server| self.check_installation(&server.language).ok())
            .collect()
    }

    /// Get installation instructions for a language.
    pub fn get_install_instructions(&self, language: &str) -> Option<String> {
        let server = self.servers.iter().find(|s| s.language == language)?;

        if server.install_commands.is_empty() {
            return None;
        }

        let mut instructions = format!(
            "To install {} for {} support, run one of the following commands:\n\n",
            server.server_name, server.language
        );

        for (i, cmd) in server.install_commands.iter().enumerate() {
            instructions.push_str(&format!("{}. `{}`\n", i + 1, cmd));
        }

        Some(instructions)
    }

    /// Print installation status for all configured servers.
    pub fn print_status(&self) {
        println!("LSP Language Server Status:\n");

        for installation in self.check_all() {
            let status = if installation.is_installed {
                "✅ Installed"
            } else {
                "❌ Not installed"
            };

            println!(
                "{}: {} {}",
                installation.language,
                status,
                installation
                    .version
                    .as_ref()
                    .map(|v| format!("({})", v))
                    .unwrap_or_default()
            );

            if !installation.is_installed {
                if let Some(ref cmd) = installation.install_command {
                    println!("  Install with: `{}`", cmd);
                }
            }

            println!();
        }
    }
}

impl Default for LspInstaller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installer_creation() {
        let installer = LspInstaller::new();
        assert!(!installer.servers.is_empty());
    }

    #[test]
    fn test_check_installation_unsupported() {
        let installer = LspInstaller::new();
        let result = installer.check_installation("unknown_language");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_installation_supported() {
        let installer = LspInstaller::new();
        let result = installer.check_installation("rust");
        assert!(result.is_ok());

        let installation = result.unwrap();
        assert_eq!(installation.language, "rust");
        assert_eq!(installation.server_name, "rust-analyzer");
    }

    #[test]
    fn test_get_install_instructions() {
        let installer = LspInstaller::new();

        let instructions = installer.get_install_instructions("rust");
        assert!(instructions.is_some());
        assert!(instructions.unwrap().contains("rustup"));

        let instructions = installer.get_install_instructions("typescript");
        assert!(instructions.is_some());
        assert!(instructions.unwrap().contains("npm"));
    }

    #[test]
    fn test_check_all() {
        let installer = LspInstaller::new();
        let installations = installer.check_all();

        assert!(!installations.is_empty());

        // At least check that we got results for expected languages
        let languages: Vec<&str> = installations.iter().map(|i| i.language.as_str()).collect();

        assert!(languages.contains(&"rust"));
        assert!(languages.contains(&"typescript"));
        assert!(languages.contains(&"python"));
        assert!(languages.contains(&"java")); // Java应该在支持的语言列表中
    }
    
    #[test]
    fn test_java_installer_creation() {
        let installer = LspInstaller::new();
        
        // 验证Java安装器定义存在
        let java_server = installer.servers.iter().find(|s| s.language == "java");
        assert!(java_server.is_some(), "Java installer definition should exist");
        
        let java_server = java_server.unwrap();
        assert_eq!(java_server.language, "java");
        assert_eq!(java_server.executable, "jdtls");
        assert_eq!(java_server.server_name, "eclipse-jdtls");
    }
    
    #[test]
    fn test_java_installation_check() {
        let installer = LspInstaller::new();
        let result = installer.check_installation("java");
        
        // 验证Java安装检查功能正常工作
        assert!(result.is_ok(), "Java installation check should not error");
        
        let installation = result.unwrap();
        assert_eq!(installation.language, "java");
        assert_eq!(installation.server_name, "eclipse-jdtls");
        
        // 验证可执行文件路径或安装命令存在
        let has_executable = installation.executable_path.is_some();
        let has_install_command = installation.install_command.is_some();
        assert!(has_executable || has_install_command,
            "Java should have either executable path or install command");
        
        // 如果有可执行文件，验证它是jdtls
        if let Some(exec_path) = installation.executable_path {
            assert!(exec_path.to_str().unwrap_or_default().contains("jdtls"),
                "Executable path should contain 'jdtls', got: {:?}", exec_path);
        }
    }
    
    #[test]
    fn test_java_install_instructions() {
        let installer = LspInstaller::new();
        
        let instructions = installer.get_install_instructions("java");
        assert!(instructions.is_some(), "Java should have install instructions");
        
        let instructions = instructions.unwrap();
        // 验证安装说明包含预期的包管理器
        assert!(instructions.contains("brew") || instructions.contains("pip") || instructions.contains("choco"),
                "Java install instructions should include package managers");
    }
    
    #[test]
    fn test_java_platform_support() {
        let installer = LspInstaller::new();
        let java_server = installer.servers.iter().find(|s| s.language == "java").unwrap();
        
        // 验证Java支持多个平台
        assert!(!java_server.package_managers.is_empty(), "Java should support at least one package manager");
        assert!(!java_server.install_commands.is_empty(), "Java should have install commands");
        
        // 验证预期的包管理器
        let supported_managers: Vec<&str> = java_server.package_managers.iter().map(|s| s.as_str()).collect();
        assert!(supported_managers.contains(&"brew") || supported_managers.contains(&"pip") || supported_managers.contains(&"choco"),
                "Java should support common package managers");
    }
    
    #[test]
    fn test_java_check_args() {
        let installer = LspInstaller::new();
        let java_server = installer.servers.iter().find(|s| s.language == "java").unwrap();
        
        // 验证Java的检查参数正确
        assert!(!java_server.check_args.is_empty(), "Java should have check arguments");
        assert!(java_server.check_args.contains(&"--version".to_string()) ||
                java_server.check_args.contains(&"version".to_string()),
                "Java check args should include version check");
    }

    #[test]
    fn test_server_installation_struct() {
        let installation = ServerInstallation {
            language: "test_lang".to_string(),
            server_name: "test-server".to_string(),
            is_installed: true,
            executable_path: Some(PathBuf::from("/usr/bin/test-server")),
            version: Some("1.0.0".to_string()),
            install_command: Some("test install".to_string()),
        };

        assert_eq!(installation.language, "test_lang");
        assert_eq!(installation.server_name, "test-server");
        assert!(installation.is_installed);
        assert!(installation.executable_path.is_some());
        assert_eq!(installation.version, Some("1.0.0".to_string()));
        assert_eq!(installation.install_command, Some("test install".to_string()));
    }

    #[test]
    fn test_server_installation_struct_minimal() {
        let installation = ServerInstallation {
            language: "test_lang".to_string(),
            server_name: "test-server".to_string(),
            is_installed: false,
            executable_path: None,
            version: None,
            install_command: None,
        };

        assert_eq!(installation.language, "test_lang");
        assert!(!installation.is_installed);
        assert!(installation.executable_path.is_none());
        assert!(installation.version.is_none());
        assert!(installation.install_command.is_none());
    }

    #[test]
    fn test_installer_server_definitions_structure() {
        let installer = LspInstaller::new();

        for server in &installer.servers {
            assert!(!server.language.is_empty());
            assert!(!server.server_name.is_empty());
            assert!(!server.executable.is_empty());
            assert!(!server.check_args.is_empty());
        }
    }

    #[test]
    fn test_installer_language_count() {
        let installer = LspInstaller::new();
        assert!(installer.servers.len() >= 6); // At least: rust, typescript, python, go, cpp, java
    }

    #[test]
    fn test_installer_unique_languages() {
        let installer = LspInstaller::new();
        let mut languages = std::collections::HashSet::new();

        for server in &installer.servers {
            languages.insert(&server.language);
        }

        assert_eq!(languages.len(), installer.servers.len(), "All server languages should be unique");
    }

    #[test]
    fn test_get_install_instructions_empty_commands() {
        // Create a test installer with empty commands for a specific language
        let mut custom_installer = LspInstaller::new();
        custom_installer.servers.push(ServerDefinition {
            language: "empty_test".to_string(),
            server_name: "empty-server".to_string(),
            executable: "empty".to_string(),
            check_args: vec![],
            install_commands: vec![],
            package_managers: vec![],
        });

        let instructions = custom_installer.get_install_instructions("empty_test");
        assert!(instructions.is_none(), "Should return None when install_commands is empty");
    }

    #[test]
    fn test_get_install_instructions_nonexistent_language() {
        let installer = LspInstaller::new();
        let instructions = installer.get_install_instructions("nonexistent_language_xyz");
        assert!(instructions.is_none());
    }

    #[test]
    fn test_check_all_returns_all_servers() {
        let installer = LspInstaller::new();
        let installations = installer.check_all();

        assert_eq!(installations.len(), installer.servers.len());

        for installation in &installations {
            assert!(!installation.language.is_empty());
            assert!(!installation.server_name.is_empty());
        }
    }

    #[test]
    fn test_installer_all_servers_have_check_args() {
        let installer = LspInstaller::new();

        for server in &installer.servers {
            assert!(!server.check_args.is_empty(), 
                "{} should have check arguments", server.server_name);
        }
    }

    #[test]
    fn test_installer_all_servers_have_install_commands() {
        let installer = LspInstaller::new();

        for server in &installer.servers {
            assert!(!server.install_commands.is_empty(),
                "{} should have install commands", server.server_name);
        }
    }

    #[test]
    fn test_installer_all_servers_have_package_managers() {
        let installer = LspInstaller::new();

        for server in &installer.servers {
            assert!(!server.package_managers.is_empty(),
                "{} should have at least one package manager", server.server_name);
        }
    }

    #[test]
    fn test_installer_configured_languages() {
        let installer = LspInstaller::new();
        let languages: Vec<&str> = installer.servers.iter().map(|s| s.language.as_str()).collect();

        assert!(languages.contains(&"rust"), "Should support Rust");
        assert!(languages.contains(&"typescript"), "Should support TypeScript");
        assert!(languages.contains(&"python"), "Should support Python");
        assert!(languages.contains(&"go"), "Should support Go");
        assert!(languages.contains(&"cpp"), "Should support C++");
        assert!(languages.contains(&"java"), "Should support Java");
    }

    #[test]
    fn test_server_installation_clone() {
        let installation = ServerInstallation {
            language: "test_lang".to_string(),
            server_name: "test-server".to_string(),
            is_installed: true,
            executable_path: Some(PathBuf::from("/usr/bin/test-server")),
            version: Some("1.0.0".to_string()),
            install_command: Some("test install".to_string()),
        };

        let cloned = installation.clone();
        assert_eq!(installation.language, cloned.language);
        assert_eq!(installation.server_name, cloned.server_name);
        assert_eq!(installation.is_installed, cloned.is_installed);
    }

    #[test]
    fn test_check_installation_various_languages() {
        let installer = LspInstaller::new();
        
        for server in &installer.servers {
            let result = installer.check_installation(&server.language);
            assert!(result.is_ok(), "Check installation should not error for {}", server.language);
            
            let installation = result.unwrap();
            assert_eq!(installation.language, server.language);
            assert_eq!(installation.server_name, server.server_name);
        }
    }

    #[test]
    fn test_installer_consistency() {
        let installer1 = LspInstaller::new();
        let installer2 = LspInstaller::new();

        assert_eq!(installer1.servers.len(), installer2.servers.len());

        for (server1, server2) in installer1.servers.iter().zip(installer2.servers.iter()) {
            assert_eq!(server1.language, server2.language);
            assert_eq!(server1.server_name, server2.server_name);
        }
    }
}
