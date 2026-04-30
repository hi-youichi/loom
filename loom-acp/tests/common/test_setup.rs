use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;

#[allow(dead_code)]
pub fn setup_test_home() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    
    let loom_dir = temp_dir.path().join(".loom");
    fs::create_dir_all(&loom_dir).expect("Failed to create .loom dir");
    
    let agents_dir = loom_dir.join("agents");
    fs::create_dir_all(&agents_dir).expect("Failed to create agents dir");
    
    std::env::set_var("LOOM_HOME", temp_dir.path());
    
    temp_dir
}

pub fn cleanup_test_home(_temp_dir: &TempDir) {
    std::env::remove_var("LOOM_HOME");
    // TempDir will be automatically cleaned up when it goes out of scope
}

pub struct TestEnvironment {
    pub temp_dir: TempDir,
    pub loom_home: PathBuf,
}

impl TestEnvironment {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let temp_dir = setup_test_home();
        let loom_home = temp_dir.path().to_path_buf();
        
        Self { temp_dir, loom_home }
    }
    
    #[allow(dead_code)]
    pub fn agents_dir(&self) -> PathBuf {
        self.loom_home.join(".loom/agents")
    }
    
    #[allow(dead_code)]
    pub fn last_model_path(&self) -> PathBuf {
        self.loom_home.join("last-model")
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        cleanup_test_home(&self.temp_dir);
    }
}