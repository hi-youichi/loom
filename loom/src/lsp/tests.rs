//! Unit tests for LSP module

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::lsp::LspManager;
    use std::path::Path;

    #[test]
    fn test_lsp_manager_creation() {
        // Test that LspManager can be created from configs (sync)
        let manager = LspManager::from_configs(env_config::get_default_lsp_servers());
        assert!(manager.detect_language(Path::new("test.rs")).is_some());
    }

    #[test]
    fn test_detect_language() {
        let manager = LspManager::from_configs(env_config::get_default_lsp_servers());
        
        let test_cases = vec![
            ("src/main.rs", "rust"),
            ("src/lib.ts", "typescript"),
            ("app.jsx", "javascript"),
            ("script.py", "python"),
            ("main.go", "go"),
        ];

        for (file_path, expected_lang) in test_cases {
            // Language detection should work based on file extension
            let detected = manager.detect_language(Path::new(file_path));
            assert_eq!(detected, Some(expected_lang.to_string()));
        }
    }
}
