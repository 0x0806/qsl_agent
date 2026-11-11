//! Configuration persisted to `~/.config/qsl-agent/config.toml`.
//!
//! The file format is a flat superset of the v0.2 format, so existing user
//! configs keep working. Every field has a default (`#[serde(default)]`), so
//! partial files are fine and new options never break old installs.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::util::expand_tilde;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    /// Prompt on mutating tools: y = once, a = session, n = deny, d = deny for session.
    #[default]
    Ask,
    /// Auto-approve mutating tools; dangerous shell commands still prompt.
    Always,
    /// Prompt on every mutating call; session-wide approval disabled.
    Strict,
    /// Approve everything except hard-blocked commands. Use only in sandboxes.
    Yolo,
}

impl ApprovalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "ask" => Some(Self::Ask),
            "always" | "auto" => Some(Self::Always),
            "strict" => Some(Self::Strict),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Self::Ask => "Ask [Interactive Review: y/a/n/d]",
            Self::Always => "Always [Autonomous; dangerous commands still reviewed]",
            Self::Strict => "Strict [Prompt on every mutating call]",
            Self::Yolo => "Yolo [Everything except hard-blocked commands]",
        }
    }
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Strict => "strict",
            Self::Yolo => "yolo",
        };
        f.write_str(s)
    }
}

/// Wire protocol used to talk to the inference server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiStyle {
    /// Ollama native API for the `ollama` provider, OpenAI-compatible elsewhere.
    #[default]
    Auto,
    /// `/v1/chat/completions` (LM Studio, llama.cpp, vLLM, OpenRouter, ...).
    Openai,
    /// Ollama native `/api/chat` (honours `num_ctx`, streams `thinking`).
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // ---- v0.2-compatible keys -------------------------------------------
    pub provider: String,
    pub ollama_url: String,
    pub lmstudio_url: String,
    pub custom_url: Option<String>,
    pub model: String,
    pub obsidian_vault_path: PathBuf,
    pub temperature: f32,
    pub max_tokens: u32,
    /// Context window (num_ctx). 0 = auto: largest that the model supports
    /// and this machine's memory fits, within min/max below.
    pub context_size: u32,
    /// Legacy switch; `approval_mode` takes precedence when set.
    pub auto_approve_commands: bool,
    pub max_react_steps: u32,
    pub command_timeout_secs: u64,

    // ---- v0.5 keys ---------------------------------------------------------
    /// Floor for the context window; agentic work needs room for tool output.
    pub min_context_size: u32,
    /// Ceiling for auto mode (larger windows cost memory and speed).
    pub max_context_size: u32,
    pub approval_mode: Option<ApprovalMode>,
    /// Bearer token for remote OpenAI-compatible endpoints (or `QSL_API_KEY`).
    pub api_key: Option<String>,
    pub api: ApiStyle,
    /// Hosted engines (by host) whose data-sharing disclaimer the user accepted.
    pub cloud_consent: Vec<String>,
    /// The key as written in the config file; env and flag keys are never saved.
    #[serde(skip)]
    pub file_api_key: Option<String>,
    /// Hosts approved for this run only (`--accept-cloud`); never saved.
    #[serde(skip)]
    pub session_consent: Vec<String>,
    /// Stream tokens to the terminal as they are generated.
    pub stream: bool,
    /// Display `<think>` / reasoning output from reasoning models.
    pub show_reasoning: bool,
    /// Seconds to wait for the server between streamed chunks.
    pub request_timeout_secs: u64,
    /// Retries for transient provider failures (connect errors, 429, 5xx).
    pub max_retries: u32,
    /// Fraction of `context_size` at which history is summarised.
    pub compaction_threshold: f32,
    /// Per-tool-result output cap sent back to the model.
    pub max_tool_output_chars: usize,
    /// Allow the `web_fetch` tool (plain HTTP GET; inference stays local).
    pub allow_web: bool,
    /// When non-empty, file tools may only touch paths under these roots.
    pub allowed_roots: Vec<PathBuf>,
    /// Load AGENTS.md / QSL.md / .qsl/instructions.md from the working dir.
    pub project_context: bool,
    /// Run independent read-only tool calls concurrently.
    pub parallel_tools: bool,
    /// Persist conversations to `<state_dir>/sessions`.
    pub save_sessions: bool,
    /// Override for sessions, checkpoints and logs (default: XDG state dir).
    pub state_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "ollama".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
            lmstudio_url: "http://127.0.0.1:1234/v1".into(),
            custom_url: None,
            model: String::new(),
            obsidian_vault_path: PathBuf::new(),
            temperature: 0.2,
            max_tokens: 4096,
            context_size: 0,
            min_context_size: 32_768,
            max_context_size: 262_144,
            auto_approve_commands: false,
            max_react_steps: 35,
            command_timeout_secs: 60,
            approval_mode: None,
            api_key: None,
            cloud_consent: Vec::new(),
            file_api_key: None,
            session_consent: Vec::new(),
            api: ApiStyle::Auto,
            stream: true,
            show_reasoning: true,
            request_timeout_secs: 600,
            max_retries: 3,
            compaction_threshold: 0.75,
            max_tool_output_chars: 12_000,
            allow_web: true,
            allowed_roots: Vec::new(),
            project_context: true,
            parallel_tools: true,
            save_sessions: true,
            state_dir: None,
        }
    }
}

/// Outcome of loading the config file, so the caller can report problems
/// through the UI instead of the loader printing on its own.
pub struct Loaded {
    pub config: Config,
    pub path: PathBuf,
    pub warning: Option<String>,
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("qsl-agent")
    }

    pub fn default_path() -> PathBuf {
        if let Ok(p) = std::env::var("QSL_CONFIG") {
            if !p.trim().is_empty() {
                return expand_tilde(&p);
            }
        }
        Self::config_dir().join("config.toml")
    }

    /// Load from `path` (or the default location). A missing file is created
    /// with defaults; a corrupt file is backed up and replaced by defaults.
    pub fn load(path: Option<&Path>) -> Loaded {
        let path = path
            .map(Path::to_path_buf)
            .unwrap_or_else(Self::default_path);
        let mut warning = None;

        let mut cfg = match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(c) => c,
                Err(e) => {
                    let backup = path.with_extension("toml.bak");
                    let _ = fs::copy(&path, &backup);
                    warning = Some(format!(
                        "config {} is invalid ({}). Backed up to {} and using defaults.",
                        path.display(),
                        e.message().trim(),
                        backup.display()
                    ));
                    let c = Config::default();
                    let _ = c.save_to(&path);
                    c
                }
            },
            Err(_) => {
                let c = Config::default();
                let _ = c.save_to(&path);
                c
            }
        };

        cfg.file_api_key = cfg.api_key.clone();
        cfg.apply_env();
        cfg.normalize();
        Loaded {
            config: cfg,
            path,
            warning,
        }
    }

    /// Environment overrides (highest precedence below CLI flags).
    fn apply_env(&mut self) {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        if let Some(v) = get("QSL_PROVIDER") {
            self.provider = v;
        }
        if let Some(v) = get("QSL_MODEL") {
            self.model = v;
        }
        if let Some(v) = get("QSL_ENDPOINT") {
            self.custom_url = Some(v);
            self.provider = "custom".into();
        }
        if let Some(v) = get("QSL_API_KEY") {
            self.api_key = Some(v);
        }
        if let Some(v) = get("QSL_APPROVAL").and_then(|v| ApprovalMode::parse(&v)) {
            self.approval_mode = Some(v);
        }
    }

    /// Clamp values into sane ranges and resolve derived paths.
    pub fn normalize(&mut self) {
        self.provider = self.provider.trim().to_lowercase();
        if !crate::frontier::ALL_PROVIDERS.contains(&self.provider.as_str()) {
            self.provider = "ollama".into();
        }
        self.temperature = self.temperature.clamp(0.0, 2.0);
        self.max_tokens = self.max_tokens.clamp(256, 131_072);
        self.min_context_size = self.min_context_size.clamp(2048, 1_048_576);
        self.max_context_size = self
            .max_context_size
            .clamp(self.min_context_size, 2_097_152);
        if self.context_size != 0 {
            self.context_size = self.context_size.clamp(2048, 2_097_152);
        }
        self.max_react_steps = self.max_react_steps.clamp(1, 500);
        self.command_timeout_secs = self.command_timeout_secs.clamp(1, 86_400);
        self.request_timeout_secs = self.request_timeout_secs.clamp(10, 86_400);
        self.max_retries = self.max_retries.min(10);
        if !(0.3..=0.95).contains(&self.compaction_threshold) {
            self.compaction_threshold = 0.75;
        }
        self.max_tool_output_chars = self.max_tool_output_chars.clamp(1_000, 1_000_000);

        let vault = self.obsidian_vault_path.to_string_lossy().to_string();
        if vault.trim().is_empty() {
            self.obsidian_vault_path = detect_obsidian_vault();
        } else {
            self.obsidian_vault_path = expand_tilde(vault.trim());
        }
        self.allowed_roots = self
            .allowed_roots
            .iter()
            .map(|p| expand_tilde(&p.to_string_lossy()))
            .collect();
        if let Some(sd) = &self.state_dir {
            self.state_dir = Some(expand_tilde(&sd.to_string_lossy()));
        }
    }

    pub fn effective_approval(&self) -> ApprovalMode {
        self.approval_mode.unwrap_or(if self.auto_approve_commands {
            ApprovalMode::Always
        } else {
            ApprovalMode::Ask
        })
    }

    pub fn set_approval(&mut self, mode: ApprovalMode) {
        self.approval_mode = Some(mode);
        self.auto_approve_commands = matches!(mode, ApprovalMode::Always | ApprovalMode::Yolo);
    }

    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone().unwrap_or_else(|| {
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("qsl-agent")
        })
    }

    /// Configured server URL for the active provider.
    pub fn base_url(&self) -> String {
        match self.provider.as_str() {
            "lmstudio" => self.lmstudio_url.clone(),
            "custom" => self
                .custom_url
                .clone()
                .unwrap_or_else(|| self.ollama_url.clone()),
            p => match crate::frontier::preset(p) {
                Some(preset) => preset.base_url.to_string(),
                None => self.ollama_url.clone(),
            },
        }
    }

    /// Host that receives the data.
    pub fn endpoint_host(&self) -> String {
        crate::frontier::host_of(&self.base_url())
    }

    /// True when prompts are sent to a machine that is not local or on the LAN.
    pub fn is_remote(&self) -> bool {
        !crate::frontier::is_local_host(&self.endpoint_host())
    }

    /// True when the user has accepted the data-sharing disclaimer for this host.
    pub fn has_cloud_consent(&self) -> bool {
        let host = self.endpoint_host();
        !self.is_remote()
            || self.cloud_consent.contains(&host)
            || self.session_consent.contains(&host)
    }

    /// API key for the active provider: explicit key, else the preset's env var.
    pub fn resolved_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| {
                crate::frontier::preset(&self.provider)
                    .and_then(|p| std::env::var(p.key_env).ok())
                    .filter(|k| !k.trim().is_empty())
            })
    }

    pub fn api_style(&self) -> ApiStyle {
        match self.api {
            ApiStyle::Auto if self.provider == "ollama" => ApiStyle::Ollama,
            ApiStyle::Auto => ApiStyle::Openai,
            other => other,
        }
    }

    /// Base URL of the server root (no `/v1`), used for Ollama native calls.
    pub fn server_root(&self) -> String {
        let url = self.base_url();
        let url = url.trim().trim_end_matches('/').to_string();
        let url = url.trim_end_matches("/chat/completions").to_string();
        match url.rsplit_once('/') {
            Some((head, last)) if is_version_segment(last) && head.contains("://") => {
                head.to_string()
            }
            _ => url,
        }
    }

    /// OpenAI-compatible base URL ending in a version segment (e.g. `/v1`).
    pub fn openai_base(&self) -> String {
        let url = self.base_url();
        let url = url.trim().trim_end_matches('/');
        let url = url.trim_end_matches("/chat/completions");
        let last = url.rsplit('/').next().unwrap_or("");
        if is_version_segment(last) || last == "openai" {
            url.to_string()
        } else {
            format!("{url}/v1")
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        self.save_to(path)
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut persisted = self.clone();
        persisted.api_key = self.file_api_key.clone();
        let body = toml::to_string_pretty(&persisted).map_err(std::io::Error::other)?;
        let header = "# =============================================================================\n\
                      # QSL Agent Configuration (v0.5)\n\
                      # Developed by Quantum Secular Labs (https://quantumseclabs.com/)\n\
                      # Every key is optional; see config.example.toml in the repository.\n\
                      # =============================================================================\n\n";
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, format!("{header}{body}"))?;
        fs::rename(&tmp, path)
    }

    /// Render the config for display, masking secrets.
    pub fn display_toml(&self) -> String {
        let mut c = self.clone();
        if c.api_key.is_some() {
            c.api_key = Some("********".into());
        }
        toml::to_string_pretty(&c).unwrap_or_default()
    }
}

fn is_version_segment(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

/// Dynamically discover an existing Obsidian vault, falling back to
/// `~/Documents/Obsidian_Vault`.
pub fn detect_obsidian_vault() -> PathBuf {
    if let Some(first) = list_detected_vaults().into_iter().next() {
        return first;
    }
    dirs::document_dir()
        .map(|d| d.join("Obsidian_Vault"))
        .or_else(|| dirs::home_dir().map(|h| h.join("Obsidian_Vault")))
        .unwrap_or_else(|| PathBuf::from("./obsidian_vault"))
}

/// Scan the system for Obsidian vaults: `$OBSIDIAN_VAULT`, the Obsidian app
/// registry, `~/Documents` and its children, common home folders, home's
/// direct children, and the working directory.
pub fn list_detected_vaults() -> Vec<PathBuf> {
    fn add(vaults: &mut Vec<PathBuf>, p: PathBuf, require_marker: bool) {
        if p.is_dir() && (!require_marker || p.join(".obsidian").exists()) && !vaults.contains(&p) {
            vaults.push(p);
        }
    }
    fn add_children(vaults: &mut Vec<PathBuf>, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut found: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        found.sort();
        for p in found {
            add(vaults, p, true);
        }
    }

    let mut vaults: Vec<PathBuf> = Vec::new();

    if let Ok(env_val) = std::env::var("OBSIDIAN_VAULT") {
        if !env_val.trim().is_empty() {
            add(&mut vaults, expand_tilde(env_val.trim()), false);
        }
    }

    let registries = [
        dirs::config_dir().map(|d| d.join("obsidian/obsidian.json")),
        dirs::config_dir().map(|d| d.join("Obsidian/obsidian.json")),
        dirs::home_dir().map(|h| h.join("Library/Application Support/obsidian/obsidian.json")),
        dirs::home_dir()
            .map(|h| h.join(".var/app/md.obsidian.Obsidian/config/obsidian/obsidian.json")),
    ];
    for reg in registries.into_iter().flatten() {
        let Ok(content) = fs::read_to_string(&reg) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if let Some(map) = val.get("vaults").and_then(|v| v.as_object()) {
            for item in map.values() {
                if let Some(p) = item.get("path").and_then(|p| p.as_str()) {
                    add(&mut vaults, PathBuf::from(p), true);
                }
            }
        }
    }

    if let Some(docs) = dirs::document_dir() {
        add(&mut vaults, docs.clone(), true);
        add_children(&mut vaults, &docs);
    }
    if let Some(home) = dirs::home_dir() {
        for name in ["Obsidian", "obsidian", "Notes", "notes", "Vault", "vault"] {
            add(&mut vaults, home.join(name), true);
        }
        add_children(&mut vaults, &home);
    }
    if let Ok(cwd) = std::env::current_dir() {
        add(&mut vaults, cwd, true);
    }
    vaults
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_v02_config_parses() {
        let legacy = r#"
provider = "lmstudio"
ollama_url = "http://127.0.0.1:11434"
lmstudio_url = "http://127.0.0.1:1234/v1"
model = "qwen2.5-coder:7b"
obsidian_vault_path = "/tmp/vault"
temperature = 0.3
max_tokens = 4096
context_size = 16384
auto_approve_commands = true
max_react_steps = 35
command_timeout_secs = 30
"#;
        let mut c: Config = toml::from_str(legacy).unwrap();
        c.normalize();
        assert_eq!(c.provider, "lmstudio");
        assert_eq!(c.effective_approval(), ApprovalMode::Always);
        assert!(c.stream, "new keys take defaults");
        assert_eq!(c.openai_base(), "http://127.0.0.1:1234/v1");
        assert_eq!(c.api_style(), ApiStyle::Openai);
    }

    #[test]
    fn example_config_parses() {
        let c: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        assert_eq!(c.approval_mode, Some(ApprovalMode::Ask));
        assert_eq!(c.api, ApiStyle::Auto);
        assert_eq!(c.max_react_steps, Config::default().max_react_steps);
    }

    #[test]
    fn partial_and_unknown_keys_are_tolerated() {
        let c: Config = toml::from_str("model = \"x\"\n[agent]\nfoo = 1\n").unwrap();
        assert_eq!(c.model, "x");
        assert_eq!(c.max_react_steps, 35);
    }

    #[test]
    fn endpoint_normalization() {
        let mut c = Config::default();
        assert_eq!(c.server_root(), "http://127.0.0.1:11434");
        assert_eq!(c.openai_base(), "http://127.0.0.1:11434/v1");
        c.provider = "custom".into();
        c.custom_url = Some("http://127.0.0.1:8080/v1/chat/completions".into());
        assert_eq!(c.openai_base(), "http://127.0.0.1:8080/v1");
        c.custom_url = Some("https://openrouter.ai/api/v1/".into());
        assert_eq!(c.openai_base(), "https://openrouter.ai/api/v1");
        c.custom_url = Some("http://localhost:8000".into());
        assert_eq!(c.openai_base(), "http://localhost:8000/v1");
    }

    #[test]
    fn values_are_clamped() {
        let mut c = Config {
            temperature: 9.0,
            max_react_steps: 0,
            compaction_threshold: 2.0,
            ..Config::default()
        };
        c.obsidian_vault_path = PathBuf::from("/tmp/v");
        c.normalize();
        assert_eq!(c.temperature, 2.0);
        assert_eq!(c.max_react_steps, 1);
        assert_eq!(c.compaction_threshold, 0.75);
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config {
            model: "m".into(),
            ..Config::default()
        };
        c.obsidian_vault_path = dir.path().join("vault");
        c.save(&path).unwrap();
        let loaded = Config::load(Some(&path));
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.config.model, "m");
    }

    #[test]
    fn corrupt_config_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is = = not toml").unwrap();
        let loaded = Config::load(Some(&path));
        assert!(loaded.warning.is_some());
        assert!(path.with_extension("toml.bak").exists());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn hosted_presets_are_remote_and_need_consent() {
        let mut c = Config::default();
        assert!(!c.is_remote() && c.has_cloud_consent());
        c.provider = "openai".into();
        assert!(c.is_remote() && !c.has_cloud_consent());
        assert_eq!(c.endpoint_host(), "api.openai.com");
        c.cloud_consent.push("api.openai.com".into());
        assert!(c.has_cloud_consent());
        c.cloud_consent.clear();
        c.session_consent.push("api.openai.com".into());
        assert!(c.has_cloud_consent());
        c.provider = "custom".into();
        c.custom_url = Some("https://llm.example.com/v1".into());
        assert!(c.is_remote());
        c.custom_url = Some("http://192.168.1.20:8000/v1".into());
        assert!(!c.is_remote());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn preset_urls_and_keys() {
        let mut c = Config::default();
        c.provider = "gemini".into();
        assert_eq!(
            c.openai_base(),
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        c.provider = "openai".into();
        assert_eq!(c.openai_base(), "https://api.openai.com/v1");
        c.api_key = Some("explicit".into());
        assert_eq!(c.resolved_api_key().as_deref(), Some("explicit"));
        c.normalize();
        assert_eq!(
            c.provider, "openai",
            "hosted provider names survive normalize"
        );
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn env_and_flag_keys_are_never_saved() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("config.toml");
        let mut c = Config::default();
        c.api_key = Some("sk-from-env".into());
        c.file_api_key = None;
        c.cloud_consent.push("api.openai.com".into());
        c.session_consent.push("example.com".into());
        c.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sk-from-env") && !text.contains("example.com"));
        assert!(text.contains("api.openai.com"));
    }
}
