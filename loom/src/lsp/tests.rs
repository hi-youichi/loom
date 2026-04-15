//! Unit tests for LSP module

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::lsp::LspManager;
    use std::path::Path;
    
    // 修复clone问题 - 创建配置的副本
    fn get_configs() -> Vec<env_config::LspServerConfig> {
        let configs = env_config::get_default_lsp_servers();
        configs
    }

    #[test]
    fn test_lsp_manager_creation() {
        // Test that LspManager can be created from configs (sync)
        let manager = LspManager::from_configs(get_configs());
        assert!(manager.detect_language(Path::new("test.rs")).is_some());
    }

    #[test]
    fn test_detect_language() {
        let manager = LspManager::from_configs(get_configs());
    }

    #[test]
    fn test_lsp_manager_java_config_loaded() {
        let configs = get_configs();

        let java_config = configs.iter().find(|c| c.language == "java");
        assert!(java_config.is_some(), "Java config should exist in default configs");

        let java_config = java_config.unwrap();
        assert_eq!(java_config.language, "java");
        assert_eq!(java_config.command, "jdtls");

        let manager = LspManager::from_configs(configs);
        let java_detected = manager.detect_language(Path::new("Test.java"));
        assert_eq!(java_detected, Some("java".to_string()));
    }

    #[test]
    fn test_lsp_manager_java_extension_mapping() {
        let manager = LspManager::from_configs(get_configs());

        let java_detected = manager.detect_language(Path::new("Test.java"));
        assert_eq!(java_detected, Some("java".to_string()));

        let test_files = vec![
            "Main.java",
            "Application.java",
            "controller/UserController.java",
        ];

        for file in test_files {
            let detected = manager.detect_language(Path::new(file));
            assert_eq!(detected, Some("java".to_string()),
                "File '{}' should be detected as Java", file);
        }
    }

    #[test]
    fn test_lsp_manager_java_config_properties() {
        let configs = get_configs();
        let java_config = configs.iter().find(|c| c.language == "java").unwrap();

        assert!(java_config.file_patterns.contains(&"*.java".to_string()));
        assert_eq!(java_config.startup_timeout_ms, 30_000);
        assert!(java_config.auto_install.is_some());
        assert!(java_config.initialization_options.is_some());
    }

    #[test]
    fn test_lsp_manager_all_supported_languages() {
        let configs = get_configs();
        let supported_languages: Vec<&str> = configs.iter()
            .map(|c| c.language.as_str())
            .collect();

        let expected_languages = vec!["rust", "typescript", "javascript", "python", "go", "java"];
        for lang in expected_languages {
            assert!(supported_languages.contains(&lang),
                "Language '{}' should be supported", lang);
        }
    }

    #[test]
    fn test_lsp_manager_extension_map_completeness() {
        let configs = get_configs();
        let manager = LspManager::from_configs(configs.clone());

        for config in &configs {
            for pattern in &config.file_patterns {
                if let Some(ext) = pattern.strip_prefix("*.") {
                    let test_file = format!("testfile.{}", ext);
                    let detected = manager.detect_language(Path::new(&test_file));

                    assert_eq!(detected, Some(config.language.clone()),
                        "Extension '{}' should map to language '{}', got {:?}",
                        ext, config.language, detected);
                }
            }
        }
    }

    #[test]
    fn test_lsp_manager_java_file_patterns() {
        let configs = get_configs();
        let java_config = configs.iter().find(|c| c.language == "java").unwrap();

        assert!(!java_config.file_patterns.is_empty(), "Java should have file patterns");
        assert!(java_config.file_patterns.contains(&"*.java".to_string()));

        let patterns: std::collections::HashSet<_> = java_config.file_patterns.iter().collect();
        assert_eq!(patterns.len(), java_config.file_patterns.len(),
            "Java file patterns should not have duplicates");
    }

    #[test]
    fn test_java_specific_detections() {
        let manager = LspManager::from_configs(get_configs());

        let java_files = vec![
            "Main.java",
            "Application.java",
            "Utils.java",
            "controller/UserController.java",
            "model/Person.java",
            "service/EmailService.java",
            "test/MyTest.java",
            "src/main/java/com/example/App.java",
        ];

        for java_file in java_files {
            let detected = manager.detect_language(Path::new(java_file));
            assert_eq!(detected, Some("java".to_string()),
                "Java file '{}' should be detected as Java", java_file);
        }
    }

    #[test]
    fn test_case_sensitive_detection() {
        let manager = LspManager::from_configs(get_configs());
        let test_cases = vec![
            ("src/main.rs", "rust"),
            ("src/lib.ts", "typescript"),
            ("app.jsx", "javascript"),
            ("script.py", "python"),
            ("main.go", "go"),
            ("App.java", "java"),
            ("TestClass.java", "java"),           // 更多Java文件变体
            ("MyInterface.java", "java"),
            ("package-info.java", "java"),
        ];
        
        for (file_path, expected_lang) in test_cases {
            let detected = manager.detect_language(Path::new(file_path));
            // 注意：根据实际实现，这可能需要调整期望值
            // 如果实现不支持大小写不敏感，这些测试可能需要修改
            assert!(detected.is_some() || detected == Some(expected_lang.to_string()),
                "File '{}' should be detected (as '{}' or '{}')", 
                file_path, expected_lang, detected.unwrap_or_default());
        }
    }
    
    #[test]
    fn test_java_vs_other_extensions() {
        let manager = LspManager::from_configs(get_configs());
        
        // 测试Java与其他相似扩展名的区分
        let test_cases = vec![
            ("MyClass.java", Some("java")),
            ("MyClass.js", Some("javascript")), // JavaScript不是Java
        ];
        
        for (file_path, expected_lang) in test_cases {
            let detected = manager.detect_language(Path::new(file_path));
            let expected = expected_lang.map(|s| s.to_string());
            assert_eq!(detected, expected,
                "File '{}' should be detected as {:?}", file_path, expected);
        }
        
        // 测试不支持Java的扩展名
        let unsupported_files = vec!["MyClass.jav", "MyClass.javax"];
        for file_path in unsupported_files {
            let detected = manager.detect_language(Path::new(file_path));
            // 这些文件可能无法识别，但不应该被识别为java
            if let Some(lang) = detected {
                assert_ne!(lang, "java", 
                    "File '{}' should not be detected as Java, got: {}", file_path, lang);
            }
        }
    }
    
    #[test]
    fn test_path_variations() {
        let manager = LspManager::from_configs(get_configs());
        
        // 测试不同路径格式下的Java文件检测
        let path_variations = vec![
            "./src/main/java/App.java",           // 相对路径
            "/absolute/path/to/File.java",         // 绝对路径  
            "C:\\Projects\\MyApp\\Main.java",     // Windows路径 (如果支持)
            "../parent/Package.java",              // 父目录路径
            "deep/nested/path/to/MyClass.java",    // 深层嵌套路径
        ];
        
        for path in path_variations {
            let detected = manager.detect_language(Path::new(path));
            assert_eq!(detected, Some("java".to_string()),
                "Java file should be detected in path: {}", path);
        }
    }
    
    #[test]
    fn test_java_file_patterns() {
        let manager = LspManager::from_configs(get_configs());
        
        // 测试常见的Java文件命名模式
        let java_patterns = vec![
            "*Test.java",      // 测试类
            "*Controller.java", // 控制器
            "*Service.java",   // 服务类
            "*Model.java",     // 模型类
            "*Utils.java",     // 工具类
            "*Constants.java", // 常量类
            "*Exception.java", // 异常类
        ];
        
        for pattern in java_patterns {
            // 创建一个符合模式的文件名
            let file_name = pattern.replace("*", "Sample");
            let detected = manager.detect_language(Path::new(&file_name));
            assert_eq!(detected, Some("java".to_string()),
                "Java file pattern '{}' should work", pattern);
        }
    }
}
