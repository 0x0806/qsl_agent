use serde::Deserialize;
use std::path::{Path, PathBuf};
use crate::error::{AgentError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub state_dir: PathBuf,
    pub provider: ProviderConfig,
    pub agent: AgentConfig,
    pub tools: ToolsConfig,
    pub health: HealthConfig,
    pub memory: MemoryConfig,
    pub cost: CostConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub tier: Tier,
    pub request_timeout_s: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier { Small, Medium, Large }

impl Tier {
    pub fn context_budget(&self) -> usize {
        match self { Tier::Small => 4_096, Tier::Medium => 32_768, Tier::Large => 131_072 }
    }
    pub fn compaction_threshold(&self) -> f32 { 0.85 }
    pub fn raw_turns_kept(&self) -> usize {
        match self { Tier::Small => 4, Tier::Medium => 8, Tier::Large => 16 }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub max_steps_per_turn: u32,
    pub max_turn_seconds: u64,
    pub system_prompt: String,
    pub temperature: f32,
    pub max_concurrent_tools: usize,
    pub tool_timeout_s: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsConfig {
    pub max_output_bytes: usize,
    pub command_timeout_s: u64,
    pub allow_shell: bool,
    pub allowed_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub heartbeat_interval_s: u64,
    pub stall_threshold_s: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    pub notes_file: PathBuf,
    pub bm25_top_k: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CostConfig {
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("QSL_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("config.toml"));
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AgentError::Config(format!("read {}: {e}", path.display())))?;
        let mut cfg: Config = toml::from_str(&text)
            .map_err(|e| AgentError::Config(format!("parse: {e}")))?;

        cfg.state_dir = expand_tilde(&cfg.state_dir);
        cfg.memory.notes_file = expand_tilde(&cfg.memory.notes_file);
        for p in &mut cfg.tools.allowed_paths { *p = expand_tilde(p); }

        cfg.validate()?;
        std::fs::create_dir_all(&cfg.state_dir)?;
        std::fs::create_dir_all(cfg.state_dir.join("sessions"))?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        let mut errs = Vec::new();
        if self.agent.max_steps_per_turn == 0 { errs.push("agent.max_steps_per_turn must be > 0".into()); }
        if self.agent.max_turn_seconds == 0 { errs.push("agent.max_turn_seconds must be > 0".into()); }
        if self.agent.max_concurrent_tools == 0 { errs.push("agent.max_concurrent_tools must be > 0".into()); }
        if self.agent.temperature < 0.0 || self.agent.temperature > 2.0 { errs.push("agent.temperature must be in [0, 2]".into()); }
        if self.tools.max_output_bytes == 0 { errs.push("tools.max_output_bytes must be > 0".into()); }
        if self.tools.allowed_paths.is_empty() { errs.push("tools.allowed_paths must not be empty".into()); }
        if !self.provider.base_url.starts_with("http") { errs.push("provider.base_url must start with http(s)://".into()); }
        if self.cost.input_usd_per_million < 0.0 || self.cost.output_usd_per_million < 0.0 { errs.push("cost prices must be >= 0".into()); }
        if self.health.heartbeat_interval_s == 0 { errs.push("health.heartbeat_interval_s must be > 0".into()); }

        if let Some(p) = self.memory.notes_file.parent() {
            if !p.as_os_str().is_empty() && std::fs::create_dir_all(p).is_err() {
                errs.push(format!("memory.notes_file parent not creatable: {}", p.display()));
            }
        }
        for p in &self.tools.allowed_paths {
            if !p.exists() { errs.push(format!("tools.allowed_paths entry does not exist: {}", p.display())); }
        }

        if errs.is_empty() { Ok(()) }
        else { Err(AgentError::Config(errs.join("; "))) }
    }
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(stripped) = p.strip_prefix("~") {
        if let Ok(home) = std::env::var("HOME") { return PathBuf::from(home).join(stripped); }
    }
    p.to_path_buf()
}
