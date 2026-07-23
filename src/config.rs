use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub timeout_seconds: u64,
    pub agent_name: String,
    pub max_context_tokens: usize,
    pub temperature: f32,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
    /// Shell command template used by `execute_command`.
    /// Use `{cmd}` as the placeholder for the user command.
    /// If empty, auto-detected from OS:
    ///   Windows → `cmd /C {cmd}`
    ///   others  → `sh -c {cmd}`
    #[serde(default)]
    pub shell: String,
}

fn default_reasoning_effort() -> String {
    "high".into()
}

fn default_max_tool_iterations() -> usize {
    25
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "deepseek".into(),
            api_key: String::new(),
            model: "deepseek-chat".into(),
            endpoint: "https://api.deepseek.com".into(),
            timeout_seconds: 120,
            agent_name: "ai-coding-assistant".into(),
            max_context_tokens: 128000,
            temperature: 0.3,
            reasoning: false,
            reasoning_effort: "high".into(),
            max_tool_iterations: 25,
            shell: String::new(),
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ferrite")
    }

    pub fn config_path(name: &str) -> PathBuf {
        Self::config_dir().join(format!("{}.toml", name))
    }

    pub fn default_config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub async fn load(name: &str) -> Result<Self> {
        let path = Self::config_path(name);
        Self::load_from_path(&path)
    }

    pub async fn load_default() -> Result<Self> {
        let path = Self::default_config_path();

        if !path.exists() {
            tracing::warn!(
                "Config file {:?} not found, using defaults (no API key set)",
                path
            );
            return Ok(Self::default());
        }

        Self::load("config").await
    }

    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("Failed to read config: {:?}", path))?;
        let mut config: Self =
            toml::from_str(&text).with_context(|| "Failed to parse TOML config")?;

        // Backfill missing fields from defaults so old/incomplete config files won't
        // crash sidecar startup.
        let defaults = Self::default();
        if config.provider.trim().is_empty() {
            config.provider = defaults.provider;
        }
        if config.model.trim().is_empty() {
            config.model = defaults.model;
        }
        if config.endpoint.trim().is_empty() {
            config.endpoint = defaults.endpoint;
        }
        if config.max_tool_iterations == 0 {
            config.max_tool_iterations = defaults.max_tool_iterations;
        }
        Ok(config)
    }

    pub fn save(&self, name: &str) -> Result<()> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config dir: {:?}", dir))?;

        let path = Self::config_path(name);
        let toml_str =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize config")?;
        fs::write(&path, toml_str)
            .with_context(|| format!("Failed to write config to {:?}", path))?;

        tracing::info!("Config saved to {:?}", path);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.provider.to_lowercase() != "ollama" && self.api_key.is_empty() {
            anyhow::bail!("API key is not set. Please configure it via the VS Code settings.");
        }
        if self.endpoint.is_empty() {
            anyhow::bail!("API endpoint is not set.");
        }
        if self.model.is_empty() {
            anyhow::bail!("Model name is not set.");
        }
        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds.clamp(1, 600))
    }

    /// Return the shell command template to use for execute_command.
    /// When `shell` is empty, auto-detect from OS.
    pub fn effective_shell(&self) -> &str {
        if !self.shell.trim().is_empty() {
            return self.shell.as_str();
        }
        if cfg!(target_os = "windows") {
            "cmd /C {cmd}"
        } else {
            "sh -c {cmd}"
        }
    }
}
