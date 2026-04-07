//! Integration tests for LSP functionality.
//!
//! These tests require actual language servers to be installed.

#[cfg(test)]
mod tests {
    use crate::lsp::LspManager;
    use env_config::LspServerConfig;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn abs_path(relative: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn create_test_config() -> LspServerConfig {
        LspServerConfig {
            language: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
            file_patterns: vec!["*.rs".to_string()],
            initialization_options: None,
            root_uri: None,
            env: std::collections::HashMap::new(),
            startup_timeout_ms: 30000,
            auto_install: None,
        }
    }

    async fn create_test_manager() -> Arc<RwLock<LspManager>> {
        let config = create_test_config();
        let manager = LspManager::from_configs(vec![config]);
        Arc::new(RwLock::new(manager))
    }

    #[tokio::test]
    async fn test_rust_analyzer_completion() {
        let manager = create_test_manager().await;
        let manager = manager.read().await;

        let test_file = abs_path("src/test.rs");
        let content = r#"
fn main() {
    println!("Hello, world!");
}
"#;

        manager.open_document(&test_file, content).await.unwrap();

        let result = manager
            .completion(&test_file, 2, 5)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rust_analyzer_diagnostics() {
        let manager = create_test_manager().await;
        let manager = manager.read().await;

        let test_file = abs_path("src/error.rs");
        let content = r#"
fn main() {
    let x = undefined_variable;
}
"#;

        manager.open_document(&test_file, content).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    #[tokio::test]
    async fn test_rust_analyzer_goto_definition() {
        let manager = create_test_manager().await;
        let manager = manager.read().await;

        let test_file = abs_path("src/definition.rs");
        let content = r#"
fn helper_function() -> i32 {
    42
}

fn main() {
    let result = helper_function();
}
"#;

        manager.open_document(&test_file, content).await.unwrap();

        let result = manager
            .goto_definition(&test_file, 6, 18)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rust_analyzer_hover() {
        let manager = create_test_manager().await;
        let manager = manager.read().await;

        let test_file = abs_path("src/hover.rs");
        let content = r#"
fn main() {
    let x = 42;
}
"#;

        manager.open_document(&test_file, content).await.unwrap();

        let result = manager.hover(&test_file, 2, 8).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let config = create_test_config();
        let manager = LspManager::from_configs(vec![config.clone()]);
        
        let active = manager.active_servers();
        assert!(active.is_empty());

        manager.shutdown_all().await;
        
        let active = manager.active_servers();
        assert!(active.is_empty());
    }
}
