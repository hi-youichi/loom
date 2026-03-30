//! E2e tests for [[providers]] in config.toml.
//!
//! Each test:
//!  1. Writes a real config.toml to a tempdir
//!  2. Runs load_and_apply_with_report with LOOM_HOME pointed at that dir
//!  3. Asserts the correct env vars are set and the report is accurate
//!
//! Tests are serialised through LOCK because they mutate process-level env vars.

use config::{load_and_apply_with_report, ConfigSource};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

// ── helpers ──────────────────────────────────────────────────────────────────

struct EnvGuard {
    keys: Vec<String>,
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    fn new(keys: &[&str]) -> Self {
        let keys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        let saved = keys
            .iter()
            .map(|k| (k.clone(), std::env::var(k).ok()))
            .collect();
        Self { keys, saved }
    }

    fn clear(&self) {
        for k in &self.keys {
            std::env::remove_var(k);
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

struct LoomHomeGuard {
    prev: Option<String>,
}

impl LoomHomeGuard {
    fn set(path: &std::path::Path) -> Self {
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", path);
        Self { prev }
    }
}

impl Drop for LoomHomeGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }
}

fn write_config(dir: &std::path::Path, content: &str) {
    std::fs::write(dir.join("config.toml"), content).unwrap();
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Happy path: default provider is applied as env vars.
#[test]
fn e2e_default_provider_sets_env_vars() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "openai"

[[providers]]
name = "openai"
api_key = "sk-e2e-test"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MODEL", "LLM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "sk-e2e-test");
    assert_eq!(
        std::env::var("OPENAI_BASE_URL").unwrap(),
        "https://api.openai.com/v1"
    );
    assert_eq!(std::env::var("MODEL").unwrap(), "gpt-4o-mini");
    assert!(std::env::var("LLM_PROVIDER").is_err(), "no type → LLM_PROVIDER unset");

    assert_eq!(report.active_provider.as_deref(), Some("openai"));
    assert!(report
        .entries
        .iter()
        .all(|e| e.key != "OPENAI_API_KEY" || e.source == ConfigSource::Provider));
}

/// Optional `temperature` on [[providers]] maps to OPENAI_TEMPERATURE.
#[test]
fn e2e_provider_temperature_sets_openai_temperature() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "openai"

[[providers]]
name = "openai"
api_key = "sk-e2e-test"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
temperature = 0.35
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&[
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "MODEL",
        "LLM_PROVIDER",
        "OPENAI_TEMPERATURE",
    ]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("OPENAI_TEMPERATURE").unwrap(), "0.35");
    let te = report.entries.iter().find(|e| e.key == "OPENAI_TEMPERATURE");
    assert_eq!(te.map(|e| e.source), Some(ConfigSource::Provider));
}

/// Provider with type = "bigmodel" maps to LLM_PROVIDER=bigmodel.
#[test]
fn e2e_bigmodel_provider_sets_llm_provider() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "bigmodel"

[[providers]]
name = "bigmodel"
api_key = "bm-key-e2e"
base_url = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-4-flash"
type = "bigmodel"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MODEL", "LLM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "bm-key-e2e");
    assert_eq!(std::env::var("LLM_PROVIDER").unwrap(), "bigmodel");
    assert_eq!(std::env::var("MODEL").unwrap(), "glm-4-flash");
    assert_eq!(report.active_provider.as_deref(), Some("bigmodel"));
}

/// Multiple providers defined; LOOM_PROVIDER env var selects among them.
#[test]
fn e2e_loom_provider_env_var_selects_provider() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "fast"

[[providers]]
name = "fast"
model = "gpt-4o-mini"

[[providers]]
name = "powerful"
model = "gpt-4o"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "LOOM_PROVIDER"]);
    env.clear();
    std::env::set_var("LOOM_PROVIDER", "powerful");

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("MODEL").unwrap(), "gpt-4o");
    assert_eq!(report.active_provider.as_deref(), Some("powerful"));
}

/// .env file takes priority over [[providers]] settings.
#[test]
fn e2e_dotenv_overrides_provider() {
    let _lock = LOCK.lock().unwrap();
    let loom_home = tempfile::tempdir().unwrap();
    let proj_dir = tempfile::tempdir().unwrap();

    write_config(
        loom_home.path(),
        r#"
[default]
provider = "base"

[[providers]]
name = "base"
api_key = "from-provider"
model = "model-from-provider"
"#,
    );
    std::fs::write(proj_dir.path().join(".env"), "MODEL=model-from-dotenv\n").unwrap();

    let _home = LoomHomeGuard::set(loom_home.path());
    let env = EnvGuard::new(&["OPENAI_API_KEY", "MODEL", "LOOM_PROVIDER"]);
    env.clear();

    load_and_apply_with_report("loom", Some(proj_dir.path())).unwrap();

    // dotenv wins for MODEL, provider still sets OPENAI_API_KEY
    assert_eq!(std::env::var("MODEL").unwrap(), "model-from-dotenv");
    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "from-provider");
}

/// Process env takes highest priority.
#[test]
fn e2e_process_env_wins_over_provider_and_dotenv() {
    let _lock = LOCK.lock().unwrap();
    let loom_home = tempfile::tempdir().unwrap();
    let proj_dir = tempfile::tempdir().unwrap();

    write_config(
        loom_home.path(),
        r#"
[default]
provider = "base"

[[providers]]
name = "base"
api_key = "from-provider"
model = "from-provider-model"
"#,
    );
    std::fs::write(proj_dir.path().join(".env"), "MODEL=from-dotenv\n").unwrap();

    let _home = LoomHomeGuard::set(loom_home.path());
    let env = EnvGuard::new(&["OPENAI_API_KEY", "MODEL", "LOOM_PROVIDER"]);
    env.clear();
    std::env::set_var("MODEL", "from-process-env");

    load_and_apply_with_report("loom", Some(proj_dir.path())).unwrap();

    assert_eq!(std::env::var("MODEL").unwrap(), "from-process-env");
    // provider sets api_key (process env didn't have it)
    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "from-provider");
}

/// [[providers]] and [env] can coexist; provider overrides same keys from [env].
#[test]
fn e2e_provider_overrides_env_section() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "fast"

[env]
MODEL = "from-env-section"
SOME_OTHER_KEY = "stays"

[[providers]]
name = "fast"
model = "from-provider"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "SOME_OTHER_KEY", "LOOM_PROVIDER"]);
    env.clear();

    load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    // provider wins over [env] for MODEL
    assert_eq!(std::env::var("MODEL").unwrap(), "from-provider");
    // unrelated key from [env] is still applied
    assert_eq!(std::env::var("SOME_OTHER_KEY").unwrap(), "stays");
}

/// Partial provider (only model set, no api_key/base_url) only touches MODEL.
#[test]
fn e2e_partial_provider_only_sets_defined_fields() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "partial"

[[providers]]
name = "partial"
model = "only-model"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "OPENAI_API_KEY", "OPENAI_BASE_URL", "LLM_PROVIDER"]);
    env.clear();

    load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("MODEL").unwrap(), "only-model");
    assert!(std::env::var("OPENAI_API_KEY").is_err());
    assert!(std::env::var("OPENAI_BASE_URL").is_err());
    assert!(std::env::var("LLM_PROVIDER").is_err());
}

/// Unknown provider name in [default].provider → no provider env vars set, no crash.
#[test]
fn e2e_unknown_provider_name_is_noop() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "does-not-exist"

[[providers]]
name = "other"
model = "other-model"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "OPENAI_API_KEY", "LOOM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert!(std::env::var("MODEL").is_err());
    assert!(report.active_provider.is_none());
}

/// No [default].provider and no LOOM_PROVIDER → behaves exactly as before (no provider applied).
#[test]
fn e2e_no_default_provider_is_backward_compatible() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[env]
MODEL = "from-env-section"
OPENAI_API_KEY = "from-env-key"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "OPENAI_API_KEY", "LOOM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("MODEL").unwrap(), "from-env-section");
    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "from-env-key");
    assert!(report.active_provider.is_none());
    assert!(report
        .entries
        .iter()
        .all(|e| e.source != ConfigSource::Provider));
}

/// Provider name matching is case-insensitive.
#[test]
fn e2e_provider_name_case_insensitive() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[default]
provider = "OpenAI"

[[providers]]
name = "openai"
model = "gpt-4o"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "LOOM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("MODEL").unwrap(), "gpt-4o");
    assert!(report.active_provider.is_some());
}

/// LOOM_PROVIDER set in [env] section selects a provider.
#[test]
fn e2e_loom_provider_in_env_section_selects_provider() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[env]
LOOM_PROVIDER = "local"

[[providers]]
name = "local"
api_key = "local-key"
base_url = "http://localhost:11434/v1"
model = "llama3.2"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "MODEL", "LOOM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert_eq!(std::env::var("OPENAI_API_KEY").unwrap(), "local-key");
    assert_eq!(
        std::env::var("OPENAI_BASE_URL").unwrap(),
        "http://localhost:11434/v1"
    );
    assert_eq!(std::env::var("MODEL").unwrap(), "llama3.2");
    assert_eq!(report.active_provider.as_deref(), Some("local"));
}

/// ConfigLoadReport.active_provider is None when [[providers]] exists but no selection.
#[test]
fn e2e_active_provider_none_when_no_selection() {
    let _lock = LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        r#"
[[providers]]
name = "only-one"
model = "gpt-4o"
"#,
    );

    let _home = LoomHomeGuard::set(dir.path());
    let env = EnvGuard::new(&["MODEL", "LOOM_PROVIDER"]);
    env.clear();

    let report = load_and_apply_with_report("loom", None::<&std::path::Path>).unwrap();

    assert!(std::env::var("MODEL").is_err());
    assert!(report.active_provider.is_none());
}
