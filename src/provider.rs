use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

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
            .timeout(Duration::from_secs(600)) // 10 min max for very large generations
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
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

        // Fallback: OpenAI-compatible /v1/models
        let models_url = if self.endpoint.ends_with("/v1") {
            format!("{}/models", self.endpoint)
        } else {
            format!("{}/v1/models", self.endpoint)
        };

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
                Ok(format!("Provider '{}' is reachable at {}. Response: {}", self.provider_name, base_url, text.trim()))
            }
            Ok(res) => Err(format!("Provider returned HTTP {}", res.status())),
            Err(e) => Err(format!("Cannot reach provider at {}: {}", base_url, e)),
        }
    }

    /// Send a chat completion request to the local LLM
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

        let total_chars: usize = messages
            .iter()
            .map(|m| m.content.as_deref().unwrap_or("").len())
            .sum();
        let estimated_tokens = (total_chars / 4) as u32;
        // Allocate context adaptively to speed up Ollama prompt evaluation
        let adaptive_ctx = (estimated_tokens + max_tokens.min(2048))
            .max(4096)
            .min(context_size);

        let mut payload = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
            "options": {
                "num_ctx": adaptive_ctx
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
                        "Request timed out to local AI engine at '{}'. The model may be loading or the context is too large.",
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
