use std::fs;
use std::path::Path;

#[allow(dead_code)]
pub fn create_agent_config(home: &Path, tier: &str) {
    let agents_dir = home.join(".loom/agents");
    fs::create_dir_all(&agents_dir).expect("Failed to create agents directory");

    let default_dir = agents_dir.join("default");
    fs::create_dir_all(&default_dir).expect("Failed to create default agent directory");

    let config_path = default_dir.join("config.yaml");
    let config = format!("tier: {}", tier);
    fs::write(config_path, config).expect("Failed to write agent config");
}

#[allow(dead_code)]
pub fn create_subagent_config(home: &Path, agent_name: &str, tier: &str) {
    let agent_dir = home.join(".loom/agents").join(agent_name);
    fs::create_dir_all(&agent_dir).expect("Failed to create agent directory");

    let config_path = agent_dir.join("config.yaml");
    let config = format!("tier: {}", tier);
    fs::write(config_path, config).expect("Failed to write subagent config");
}
#[allow(dead_code)]
pub fn read_last_model_file(home: &Path) -> String {
    let last_model_path = home.join("last-model");
    fs::read_to_string(last_model_path)
        .expect("Failed to read last-model file")
        .trim()
        .to_string()
}
#[allow(dead_code)]
pub fn write_last_model_file(home: &Path, model: &str) {
    let last_model_path = home.join("last-model");
    fs::write(last_model_path, model).expect("Failed to write last-model file");
}
#[allow(dead_code)]
pub fn create_test_agents_config(home: &Path) {
    let _agents_dir = home.join(".loom/agents");

    // Create product-manager agent with low tier
    create_subagent_config(home, "product-manager", "low");

    // Create test-engineer agent with medium tier
    create_subagent_config(home, "test-engineer", "medium");

    // Create default agent with medium tier
    create_agent_config(home, "medium");
}
