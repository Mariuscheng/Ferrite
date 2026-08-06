use ferrite::config::Config;
use std::path::PathBuf;
use std::time::Duration;

/// Build a unique temp-file path safe for parallel test execution.
fn temp_config_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join(format!(
        "ferrite-test-{}-{}.toml",
        std::process::id(),
        tag
    ))
}

fn cleanup_path(path: &PathBuf) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn default_values_are_sane() {
    let config = Config::default();
    assert_eq!(config.provider, "deepseek");
    assert_eq!(config.model, "deepseek-chat");
    assert_eq!(config.timeout_seconds, 120);
    assert_eq!(config.max_tool_iterations, 25);
    assert_eq!(config.reasoning_effort, "high");
    assert!(config.api_key.is_empty());
    assert!(config.shell.is_empty());
}

#[test]
fn validate_rejects_empty_api_key_for_non_ollama() {
    let config = Config::default();
    assert!(config.validate().is_err());
}

#[test]
fn validate_rejects_empty_endpoint() {
    let mut config = Config::default();
    config.api_key = "test-key".to_string();
    config.endpoint = String::new();
    assert!(config.validate().is_err());
}

#[test]
fn validate_rejects_empty_model() {
    let mut config = Config::default();
    config.api_key = "test-key".to_string();
    config.model = String::new();
    assert!(config.validate().is_err());
}

#[test]
fn validate_accepts_ollama_without_api_key() {
    let mut config = Config::default();
    config.provider = "ollama".to_string();
    config.endpoint = "http://localhost:11434".to_string();
    assert!(config.validate().is_ok());
}

#[test]
fn validate_accepts_valid_config() {
    let mut config = Config::default();
    config.api_key = "test-key".to_string();
    assert!(config.validate().is_ok());
}

#[test]
fn timeout_clamps_to_minimum_one_second() {
    let mut config = Config::default();
    config.timeout_seconds = 0;
    assert_eq!(config.timeout(), Duration::from_secs(1));
}

#[test]
fn timeout_clamps_to_maximum_six_hundred_seconds() {
    let mut config = Config::default();
    config.timeout_seconds = 9999;
    assert_eq!(config.timeout(), Duration::from_secs(600));
}

#[test]
fn timeout_uses_configured_value() {
    let mut config = Config::default();
    config.timeout_seconds = 42;
    assert_eq!(config.timeout(), Duration::from_secs(42));
}

#[test]
fn effective_shell_uses_custom_template_when_set() {
    let mut config = Config::default();
    config.shell = "bash -c {cmd}".to_string();
    assert_eq!(config.effective_shell(), "bash -c {cmd}");
}

#[test]
fn effective_shell_auto_detects_when_empty() {
    let config = Config::default();
    assert!(config.effective_shell().contains("{cmd}"));
}

#[test]
fn load_from_path_reads_toml_and_backfills_defaults() {
    let path = temp_config_path("load");
    cleanup_path(&path);
    std::fs::write(
        &path,
        r#"
provider = "ollama"
api_key = ""
model = "llama3"
endpoint = "http://localhost:11434"
timeout_seconds = 60
agent_name = "test-agent"
max_context_tokens = 32000
temperature = 0.2
"#,
    )
    .expect("write config");

    let config = Config::load_from_path(&path).expect("load config");
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.model, "llama3");
    // Values from the file are preserved.
    assert_eq!(config.endpoint, "http://localhost:11434");
    assert_eq!(config.timeout_seconds, 60);
    // Missing optional field is backfilled from defaults.
    assert_eq!(config.max_tool_iterations, Config::default().max_tool_iterations);
    cleanup_path(&path);
}

#[test]
fn toml_serialization_roundtrips_all_fields() {
    let config = Config {
        provider: "deepseek".to_string(),
        api_key: "secret".to_string(),
        model: "deepseek-chat".to_string(),
        endpoint: "https://api.deepseek.com".to_string(),
        timeout_seconds: 60,
        agent_name: "test-agent".to_string(),
        max_context_tokens: 32000,
        temperature: 0.5,
        reasoning: true,
        reasoning_effort: "low".to_string(),
        max_tool_iterations: 10,
        shell: "pwsh -c {cmd}".to_string(),
    };
    let toml_str = toml::to_string_pretty(&config).expect("serialize");
    let loaded: Config = toml::from_str(&toml_str).expect("deserialize");
    assert_eq!(loaded.provider, config.provider);
    assert_eq!(loaded.api_key, config.api_key);
    assert_eq!(loaded.model, config.model);
    assert_eq!(loaded.endpoint, config.endpoint);
    assert_eq!(loaded.timeout_seconds, config.timeout_seconds);
    assert_eq!(loaded.agent_name, config.agent_name);
    assert_eq!(loaded.max_context_tokens, config.max_context_tokens);
    assert_eq!(loaded.temperature, config.temperature);
    assert_eq!(loaded.reasoning, config.reasoning);
    assert_eq!(loaded.reasoning_effort, config.reasoning_effort);
    assert_eq!(loaded.max_tool_iterations, config.max_tool_iterations);
    assert_eq!(loaded.shell, config.shell);
}

#[test]
fn load_from_missing_path_can_be_handled() {
    let result = Config::load_from_path(&PathBuf::from("nonexistent-config.toml"));
    assert!(result.is_err());
}