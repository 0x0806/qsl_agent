use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Global configuration for QSL Agent, persisted to ~/.config/qsl-agent/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub ollama_url: String,
    pub lmstudio_url: String,
    pub custom_url: Option<String>,
    pub model: String,
    pub obsidian_vault_path: PathBuf,
    pub temperature: f32,
    pub max_tokens: u32,
    pub context_size: u32,
    pub auto_approve_commands: bool,
    pub max_react_steps: u32,
    pub command_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        let default_vault = Self::detect_obsidian_vault();

        Self {
            provider: "ollama".to_string(),
            ollama_url: "http://127.0.0.1:11434".to_string(),
            lmstudio_url: "http://127.0.0.1:1234/v1".to_string(),
            custom_url: None,
            model: String::new(), // Empty triggers auto-detect
            obsidian_vault_path: default_vault,
            temperature: 0.2,
            max_tokens: 4096,
            context_size: 16384,
            auto_approve_commands: false,
            max_react_steps: 35,
            command_timeout_secs: 30,
        }
    }
}

impl Config {
    /// Dynamically discover any existing Obsidian vault on the local system
    pub fn detect_obsidian_vault() -> PathBuf {
        let detected = Self::list_detected_vaults();
        if let Some(first) = detected.into_iter().next() {
            return first;
        }

        // Clean default fallback if no existing vault is discovered on the machine
        dirs::document_dir()
            .map(|d| d.join("Obsidian_Vault"))
            .or_else(|| dirs::home_dir().map(|h| h.join("Obsidian_Vault")))
            .unwrap_or_else(|| PathBuf::from("./obsidian_vault"))
    }

    /// Scan system for all active Obsidian vaults
    pub fn list_detected_vaults() -> Vec<PathBuf> {
        let mut vaults = Vec::new();

        // 1. Environment variable OBSIDIAN_VAULT
        if let Ok(env_val) = std::env::var("OBSIDIAN_VAULT") {
            let p = PathBuf::from(env_val);
            if p.exists() && !vaults.contains(&p) {
                vaults.push(p);
            }
        }

        // 2. Official Obsidian app registry (obsidian.json)
        let config_roots = vec![
            dirs::config_dir().map(|d| d.join("obsidian").join("obsidian.json")),
            dirs::home_dir().map(|h| h.join("Library").join("Application Support").join("obsidian").join("obsidian.json")),
            dirs::config_dir().map(|d| d.join("Obsidian").join("obsidian.json")),
        ];

        for opt in config_roots.into_iter().flatten() {
            if opt.exists() {
                if let Ok(content) = fs::read_to_string(&opt) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(v_map) = val.get("vaults").and_then(|v| v.as_object()) {
                            for (_k, item) in v_map {
                                if let Some(path_str) = item.get("path").and_then(|p| p.as_str()) {
                                    let p = PathBuf::from(path_str);
                                    if p.exists() && p.join(".obsidian").exists() && !vaults.contains(&p) {
                                        vaults.push(p);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Scan Documents directory and immediate subdirectories
        if let Some(docs) = dirs::document_dir() {
            if docs.exists() {
                if docs.join(".obsidian").exists() && !vaults.contains(&docs) {
                    vaults.push(docs.clone());
                }
                if let Ok(entries) = fs::read_dir(&docs) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_dir() && path.join(".obsidian").exists() && !vaults.contains(&path) {
                            vaults.push(path);
                        }
                    }
                }
            }
        }

        // 4. Scan common home directory folders (Obsidian, Notes, Vault, etc.)
        if let Some(home) = dirs::home_dir() {
            let candidates = vec![
                home.join("Obsidian"),
                home.join("obsidian"),
                home.join("Notes"),
                home.join("notes"),
                home.join("Vault"),
                home.join("vault"),
            ];
            for cand in candidates {
                if cand.exists() && cand.join(".obsidian").exists() && !vaults.contains(&cand) {
                    vaults.push(cand);
                }
            }

            // Check immediate subdirectories of home
            if let Ok(entries) = fs::read_dir(&home) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_dir() && path.join(".obsidian").exists() && !vaults.contains(&path) {
                        vaults.push(path);
                    }
                }
            }
        }

        // 5. Current working directory
        let cwd = PathBuf::from(".");
        if cwd.join(".obsidian").exists() {
            if let Ok(canon) = fs::canonicalize(&cwd) {
                if !vaults.contains(&canon) {
                    vaults.push(canon);
                }
            }
        }

        vaults
    }

    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("qsl-agent")
    }

    pub fn config_path() -> PathBuf {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);
        dir.join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = toml::from_str::<Config>(&content) {
                    // If vault path is empty or does not exist on this machine, dynamically detect!
                    if cfg.obsidian_vault_path.as_os_str().is_empty() || !cfg.obsidian_vault_path.exists() {
                        cfg.obsidian_vault_path = Self::detect_obsidian_vault();
                    }
                    return cfg;
                }
                // Config parse error — backup corrupted file and recreate
                let backup = path.with_extension("toml.bak");
                let _ = fs::copy(&path, &backup);
                eprintln!(
                    "Warning: config.toml was corrupted. Backed up to {}. Using defaults.",
                    backup.display()
                );
            }
        }
        let default_cfg = Self::default();
        let _ = default_cfg.save();
        default_cfg
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;
        let header = "# =============================================================================\n\
                      # QSL Agent Configuration (2026 Edition)\n\
                      # Developed by Quantum Secular Labs (https://quantumseclabs.com/)\n\
                      # Autonomous, 100% Local & Sovereign Intelligence Engine\n\
                      # =============================================================================\n\n";
        fs::write(path, format!("{}{}", header, serialized))
    }

    pub fn get_active_endpoint(&self) -> String {
        match self.provider.to_lowercase().as_str() {
            "ollama" => format!("{}/v1", self.ollama_url.trim_end_matches('/')),
            "lmstudio" => self.lmstudio_url.trim_end_matches('/').to_string(),
            "custom" => self
                .custom_url
                .clone()
                .unwrap_or_else(|| format!("{}/v1", self.ollama_url.trim_end_matches('/'))),
            _ => format!("{}/v1", self.ollama_url.trim_end_matches('/')),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_dynamic_detection() {
        let vault = Config::detect_obsidian_vault();
        assert!(!vault.as_os_str().is_empty());
    }

    #[test]
    fn test_list_detected_vaults() {
        let vaults = Config::list_detected_vaults();
        // Should find existing vaults on the host machine if any exist
        if !vaults.is_empty() {
            for v in vaults {
                assert!(v.exists(), "Detected vault must exist on filesystem");
            }
        }
    }
}
