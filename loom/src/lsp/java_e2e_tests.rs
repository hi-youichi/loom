//! End-to-end integration tests for Java LSP support
//!
//! These tests verify the complete flow from file detection through
//! configuration loading to installation readiness.

#[cfg(test)]
mod java_e2e_tests {
    use crate::lsp::LspManager;
    use crate::lsp::installer::LspInstaller;
    use std::path::Path;
    use env_config::{LspConfig, get_default_lsp_servers, load_default_lsp_config};

    #[test]
    fn test_java_e2e_file_to_config_flow() {
        // 完整的流程测试：从文件检测到配置加载
        
        // 1. 模拟检测到一个Java文件
        let java_file = Path::new("src/main/java/com/example/Application.java");
        
        // 2. 获取配置并验证它包含Java配置
        let configs = get_default_lsp_servers();
        let manager = LspManager::from_configs(configs.clone());
        let detected_language = manager.detect_language(java_file);
        
        assert_eq!(detected_language, Some("java".to_string()),
            "Java file should be detected correctly in e2e flow");
        
        // 3. 验证Java配置的完整性
        let java_config = configs.iter()
            .find(|c| c.language == "java")
            .expect("Java config should exist in configs");
        
        assert_eq!(java_config.command, "jdtls");
        assert!(java_config.file_patterns.contains(&"*.java".to_string()));
        assert_eq!(java_config.startup_timeout_ms, 30_000);
    }

    #[test]
    fn test_java_e2e_config_serialization() {
        // 测试配置的完整序列化/反序列化流程
        
        // 1. 获取默认配置
        let config = LspConfig::default();
        
        // 2. 验证Java配置存在
        assert!(config.servers.iter().any(|s| s.language == "java"),
            "Default config should contain Java server");
        
        // 3. 验证配置可以被正确处理（通过load_default_lsp_config）
        let loaded_config = load_default_lsp_config()
            .expect("Default config should load successfully");
        
        // 4. 验证加载的配置包含Java
        assert!(loaded_config.servers.iter().any(|s| s.language == "java"),
            "Loaded config should contain Java server");
        
        // 5. 验证两个配置的Java部分一致
        let original_java = config.servers.iter().find(|s| s.language == "java").unwrap();
        let loaded_java = loaded_config.servers.iter().find(|s| s.language == "java").unwrap();
        
        assert_eq!(original_java.language, loaded_java.language);
        assert_eq!(original_java.command, loaded_java.command);
        assert_eq!(original_java.file_patterns, loaded_java.file_patterns);
        assert_eq!(original_java.startup_timeout_ms, loaded_java.startup_timeout_ms);
    }

    #[test]
    fn test_java_e2e_installer_integration() {
        // 测试安装器与配置的集成
        
        // 1. 创建安装器
        let installer = LspInstaller::new();
        
        // 2. 验证Java在安装器中存在
        let install_result = installer.check_installation("java");
        assert!(install_result.is_ok(), "Java should be supported by installer");
        
        let installation = install_result.unwrap();
        assert_eq!(installation.language, "java");
        assert_eq!(installation.server_name, "eclipse-jdtls");
        
        // 3. 验证安装说明可用
        let instructions = installer.get_install_instructions("java");
        assert!(instructions.is_some(), "Java should have install instructions");
        
        let instructions = instructions.unwrap();
        assert!(instructions.len() > 0, "Install instructions should not be empty");
        
        // 4. 验证安装说明包含相关的包管理器
        assert!(instructions.contains("brew") || instructions.contains("pip") || instructions.contains("choco"),
            "Install instructions should mention package managers");
    }

    #[test]
    fn test_java_e2e_config_loading_flow() {
        // 测试配置加载的完整流程
        
        // 1. 加载默认配置
        let loaded_config = load_default_lsp_config()
            .expect("Default LSP config should load successfully");
        
        // 2. 验证配置包含Java
        assert!(!loaded_config.servers.is_empty(), "Config should have servers");
        assert!(loaded_config.servers.iter().any(|s| s.language == "java"),
            "Loaded config should contain Java server");
        
        // 3. 验证可以使用这个配置创建LSP Manager
        let manager = LspManager::from_configs(loaded_config.servers.clone());
        let java_detected = manager.detect_language(Path::new("Test.java"));
        
        assert_eq!(java_detected, Some("java".to_string()),
            "Loaded config should enable Java file detection");
    }

    #[test]
    fn test_java_e2e_multi_file_scenario() {
        // 测试多文件场景下的Java支持
        
        let manager = LspManager::from_configs(get_default_lsp_servers());
        
        // 模拟一个典型的Java项目结构
        let project_files = vec![
            "src/main/java/com/example/Main.java",
            "src/main/java/com/example/controller/UserController.java",
            "src/main/java/com/example/service/UserService.java",
            "src/main/java/com/example/model/User.java",
            "src/test/java/com/example/UserTest.java",
            "pom.xml",  // Maven文件 (不应该被识别为Java)
            "build.gradle", // Gradle文件 (不应该被识别为Java)
        ];
        
        let mut java_count = 0;
        let mut non_java_count = 0;
        
        for file_path in project_files {
            let detected = manager.detect_language(Path::new(file_path));
            match file_path {
                path if path.ends_with(".java") => {
                    assert_eq!(detected, Some("java".to_string()),
                        "Java file '{}' should be detected as Java", path);
                    java_count += 1;
                }
                _ => {
                    assert_ne!(detected, Some("java".to_string()),
                        "Non-Java file '{}' should not be detected as Java", file_path);
                    non_java_count += 1;
                }
            }
        }
        
        assert!(java_count > 0, "Should have detected at least one Java file");
        assert!(non_java_count > 0, "Should have correctly rejected non-Java files");
    }

    #[test]
    fn test_java_e2e_error_handling() {
        // 测试错误处理场景
        
        let manager = LspManager::from_configs(get_default_lsp_servers());
        
        // 1. 测试无效文件路径
        let invalid_paths = vec![
            "",           // 空路径
            "noextension", // 无扩展名
            ".hidden",    // 隐藏文件无扩展名
            "file.unknownext", // 未知扩展名
        ];
        
        for invalid_path in invalid_paths {
            let detected = manager.detect_language(Path::new(invalid_path));
            // 这些路径可能无法识别，但不应该导致panic
            let _ = detected;
        }
        
        // 2. 测试安装器对不支持的语言的处理
        let installer = LspInstaller::new();
        let unsupported_result = installer.check_installation("nonexistent_language");
        assert!(unsupported_result.is_err(), 
            "Installer should return error for unsupported language");
        
        // 3. 测试获取不支持语言的安装说明
        let no_instructions = installer.get_install_instructions("nonexistent_language");
        assert!(no_instructions.is_none(), 
            "Installer should return None for unsupported language");
    }

    #[test]
    fn test_java_e2e_performance_characteristics() {
        // 测试性能特征
        
        // 1. 测试配置加载性能
        let start = std::time::Instant::now();
        let _config = LspConfig::default();
        let config_load_time = start.elapsed();
        
        assert!(config_load_time.as_millis() < 100, 
            "Config loading should be fast (< 100ms), took: {:?}", config_load_time);
        
        // 2. 测试LSP Manager创建性能
        let start = std::time::Instant::now();
        let _manager = LspManager::from_configs(get_default_lsp_servers());
        let manager_creation_time = start.elapsed();
        
        assert!(manager_creation_time.as_millis() < 50, 
            "Manager creation should be fast (< 50ms), took: {:?}", manager_creation_time);
        
        // 3. 测试语言检测性能
        let manager = LspManager::from_configs(get_default_lsp_servers());
        let test_files = vec![
            "Test.java", "Application.java", "controller/UserController.java",
            "service/EmailService.java", "model/Person.java",
        ];
        
        let start = std::time::Instant::now();
        for file in &test_files {
            let _detected = manager.detect_language(Path::new(file));
        }
        let detection_time = start.elapsed();
        
        assert!(detection_time.as_millis() < 10, 
            "Language detection should be very fast (< 10ms for {} files), took: {:?}", 
            test_files.len(), detection_time);
    }

    #[test]
    fn test_java_e2e_configuration_completeness() {
        // 测试Java配置的完整性和一致性
        
        let config = LspConfig::default();
        let java_config = config.servers.iter()
            .find(|s| s.language == "java")
            .expect("Java config should exist");
        
        // 验证所有必需字段都存在且有效
        assert!(!java_config.language.is_empty(), "Language should not be empty");
        assert!(!java_config.command.is_empty(), "Command should not be empty");
        assert!(!java_config.file_patterns.is_empty(), "File patterns should not be empty");
        assert!(java_config.startup_timeout_ms > 0, "Startup timeout should be positive");
        
        // 验证Java特定的配置值
        assert_eq!(java_config.language, "java");
        assert_eq!(java_config.command, "jdtls");
        assert!(java_config.file_patterns.contains(&"*.java".to_string()));
        assert_eq!(java_config.startup_timeout_ms, 30_000);
        
        // 验证可选配置的存在
        assert!(java_config.auto_install.is_some(), "Java should have auto-install config");
        assert!(java_config.initialization_options.is_some(), "Java should have initialization options");
        
        // 验证自动安装配置的完整性
        let auto_install = java_config.auto_install.as_ref().unwrap();
        assert!(auto_install.enabled, "Java auto-install should be enabled");
        assert!(!auto_install.command.is_empty(), "Auto-install command should not be empty");
        assert!(auto_install.verify_command.is_some(), "Auto-install should have verify command");
    }
}