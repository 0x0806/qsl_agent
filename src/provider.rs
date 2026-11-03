use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Model parameter tier classification for optimal hardware and context tuning
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelTier {
    /// Models <= 9B parameters (e.g. 3B, 7B, 8B, 9B): High-speed, compact KV cache
    Small,
    /// Models 10B - 34B parameters (e.g. 12B, 14B, 27B, 32B): Balanced reasoning powerhouse
    Medium,
    /// Models >= 35B parameters (e.g. 70B, 72B, 120B+): Deep enterprise reasoning, high VRAM
    Large,
}

impl ModelTier {
    pub fn from_model_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("70b")
            || lower.contains("72b")
            || lower.contains("67b")
            || lower.contains("104b")
            || lower.contains("110b")
            || lower.contains("120b")
            || lower.contains("236b")
            || lower.contains("405b")
            || lower.contains("large")
        {
            ModelTier::Large
        } else if lower.contains("12b")
            || lower.contains("13b")
            || lower.contains("14b")
            || lower.contains("27b")
            || lower.contains("32b")
            || lower.contains("33b")
            || lower.contains("34b")
            || lower.contains("35b")
            || lower.contains("medium")
        {
            ModelTier::Medium
        } else {
            ModelTier::Small
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            ModelTier::Small => "Small (<=9B) [High-Speed / Compact KV Cache]",
            ModelTier::Medium => "Medium (10B-34B) [Balanced Workstation / 16K-32K Context]",
            ModelTier::Large => "Large (>=35B) [Enterprise Reasoning / 32K+ Context & Adaptive Headroom]",
        }
    }

    /// Optimal generation token headroom for adaptive context window sizing
    pub fn optimal_headroom(&self, requested_max: u32) -> u32 {
        match self {
            ModelTier::Small => requested_max.min(2048).max(1024),
            ModelTier::Medium => requested_max.min(4096).max(2048),
            ModelTier::Large => requested_max.min(8192).max(4096),
        }
    }
}

/// A single message in a chat conversation (OpenAI-compatible format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Client for communicating with a local LLM server (Ollama, LM Studio, or any OpenAI-compatible endpoint)
pub struct LocalLlmClient {
    pub endpoint: String,
    pub client: Client,
    pub provider_name: String,
}

impl LocalLlmClient {
    pub fn new(endpoint: &str, provider_name: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(1200)) // 20 min max to accommodate 70B models on CPU/GPU
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client,
            provider_name: provider_name.to_string(),
        }
    }

    /// List all available models from the local inference server
    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let base_url = if self.endpoint.ends_with("/v1") {
            self.endpoint.trim_end_matches("/v1").to_string()
        } else {
            self.endpoint.clone()
        };

        // Try Ollama native /api/tags first
        if self.provider_name.to_lowercase() == "ollama" {
            let ollama_tags_url = format!("{}/api/tags", base_url);
            if let Ok(res) = self
                .client
                .get(&ollama_tags_url)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(data) = res.json::<Value>().await {
                        if let Some(models) = data.get("models").and_then(|m| m.as_array()) {
                            let names: Vec<String> = models
                                .iter()
                                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                                .map(|s| s.to_string())
                                .collect();
                            if !names.is_empty() {
                                return Ok(names);
                            }
                        }
                    }
                }
            }
        }

        // Standard OpenAI /v1/models fallback
        let models_url = format!("{}/models", self.endpoint);
        let res = self
            .client
            .get(&models_url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("Failed to reach local endpoint at {}: {}", models_url, e))?;

        if !res.status().is_success() {
            return Err(format!(
                "Failed to query models from {}: HTTP {}",
                models_url,
                res.status()
            ));
        }

        let json_val = res
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse models JSON: {}", e))?;

        let mut model_ids = Vec::new();
        if let Some(data) = json_val.get("data").and_then(|d| d.as_array()) {
            for item in data {
                if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                    model_ids.push(id.to_string());
                }
            }
        }

        Ok(model_ids)
    }

    /// Auto-detect the first available model from the local inference server
    pub async fn auto_detect_model(&self) -> Option<String> {
        match self.list_models().await {
            Ok(models) if !models.is_empty() => Some(models[0].clone()),
            _ => None,
        }
    }

    /// Check if the local inference server is reachable and healthy
    pub async fn health_check(&self) -> Result<String, String> {
        let base_url = if self.endpoint.ends_with("/v1") {
            self.endpoint.trim_end_matches("/v1").to_string()
        } else {
            self.endpoint.clone()
        };

        // Try Ollama health
        let health_url = format!("{}/", base_url);
        match self
            .client
            .get(&health_url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => {
                let text = res.text().await.unwrap_or_default();
                Ok(format!(
                    "Provider '{}' is reachable at {}. Response: {}",
                    self.provider_name,
                    base_url,
                    text.trim()
                ))
            }
            Ok(res) => Err(format!("Provider returned HTTP {}", res.status())),
            Err(e) => Err(format!("Cannot reach provider at {}: {}", base_url, e)),
        }
    }

    /// Send a chat completion request to the local LLM with adaptive context and tier optimization
    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        temperature: f32,
        max_tokens: u32,
        context_size: u32,
    ) -> Result<ChatMessage, String> {
        let chat_url = if self.endpoint.ends_with("/v1") {
            format!("{}/chat/completions", self.endpoint)
        } else {
            format!("{}/v1/chat/completions", self.endpoint)
        };

        let tier = ModelTier::from_model_name(model);
        let total_chars: usize = messages
            .iter()
            .map(|m| m.content.as_deref().unwrap_or("").len())
            .sum();
        let prompt_tokens = (total_chars / 4) as u32;

        let headroom = tier.optimal_headroom(max_tokens);
        // Dynamic KV cache bounds: prevent CUDA OOM on 70B models while ensuring ample generation headroom
        let adaptive_ctx = (prompt_tokens + headroom).clamp(4096, context_size);

        let mut payload = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
            "options": {
                "num_ctx": adaptive_ctx,
                "num_predict": max_tokens,
                "temperature": temperature
            }
        });

        if let Some(t) = tools {
            if !t.is_empty() {
                payload["tools"] = json!(t);
                payload["tool_choice"] = json!("auto");
            }
        }

        let response = self
            .client
            .post(&chat_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    format!(
                        "Connection refused to local AI engine at '{}'. Is your local server (Ollama/LM Studio) running?\n  → Start Ollama: ollama serve\n  → Start LM Studio: open LM Studio and enable the local server",
                        chat_url
                    )
                } else if e.is_timeout() {
                    format!(
                        "Request timed out to local AI engine at '{}'. If running a large model (70B+), inference may be compute-constrained.",
                        chat_url
                    )
                } else {
                    format!("Connection error to local AI engine ({}): {}", chat_url, e)
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Local AI model error [HTTP {}]: {}", status, text));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse model response: {}", e))?;

        let choice = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| "No choices returned by local model".to_string())?;

        let message_val = choice
            .get("message")
            .ok_or_else(|| "No message object in choice".to_string())?;

        let message: ChatMessage = serde_json::from_value(message_val.clone())
            .map_err(|e| format!("Failed to deserialize ChatMessage: {}", e))?;

        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_tier_classification() {
        assert_eq!(ModelTier::from_model_name("llama3.2:3b"), ModelTier::Small);
        assert_eq!(ModelTier::from_model_name("huihui-qwen35-9b-256k:latest"), ModelTier::Small);
        assert_eq!(ModelTier::from_model_name("qwen2.5:7b"), ModelTier::Small);
        assert_eq!(ModelTier::from_model_name("qwen2.5-coder:14b"), ModelTier::Medium);
        assert_eq!(ModelTier::from_model_name("qwen2.5:32b"), ModelTier::Medium);
        assert_eq!(ModelTier::from_model_name("llama3.3:70b"), ModelTier::Large);
        assert_eq!(ModelTier::from_model_name("deepseek-r1:70b"), ModelTier::Large);
        assert_eq!(ModelTier::from_model_name("qwen2.5:72b-instruct-q4_k_m"), ModelTier::Large);
    }

    #[test]
    fn test_tier_headroom() {
        assert_eq!(ModelTier::Small.optimal_headroom(4096), 2048);
        assert_eq!(ModelTier::Medium.optimal_headroom(4096), 4096);
        assert_eq!(ModelTier::Large.optimal_headroom(8192), 8192);
    }
}
