//! Context-window sizing.
//!
//! Agentic work needs room: system prompt + tool schemas alone take ~4K
//! tokens, and every file read or command output adds more. QSL therefore
//! enforces a minimum (`min_context_size`, default 32K) and, in auto mode
//! (`context_size = 0`, the default), uses the largest window that
//!   1. the model supports (`<arch>.context_length`),
//!   2. is at most `max_context_size` (default 256K), and
//!   3. fits in this machine's free RAM + GPU memory, using the KV-cache
//!      cost per token computed from the model's architecture.
//!
//! This keeps one default safe on an 8 GB laptop and generous on a server.

use serde_json::{Map, Value};

#[derive(Debug, Default, Clone, Copy)]
pub struct ModelMeta {
    pub max_ctx: Option<u32>,
    /// f16 KV-cache bytes per token of context.
    pub kv_bytes_per_token: Option<u64>,
    /// Model weights on disk (≈ memory needed for weights).
    pub weights_bytes: Option<u64>,
    /// Memory currently held by models loaded in the engine (freed on reload).
    pub resident_bytes: u64,
}

/// KV-cache bytes per token from GGUF metadata as reported by Ollama.
/// Handles GQA (`head_count_kv`), explicit key/value lengths, per-layer
/// arrays, and hybrid models where only every Nth layer uses attention
/// (`full_attention_interval`, e.g. Qwen3.5 / Qwen3-Next).
pub fn kv_bytes_per_token(info: &Map<String, Value>) -> Option<u64> {
    let arch = info.get("general.architecture")?.as_str()?;
    let num = |key: &str| -> Option<u64> {
        match info.get(&format!("{arch}.{key}"))? {
            Value::Number(n) => n.as_u64(),
            // Per-layer arrays: use the maximum (conservative).
            Value::Array(a) => a.iter().filter_map(Value::as_u64).max(),
            _ => None,
        }
    };
    let layers = num("block_count")?;
    let heads = num("attention.head_count")?;
    let kv_heads = num("attention.head_count_kv").unwrap_or(heads).max(1);
    let head_dim = num("embedding_length").map(|e| e / heads.max(1));
    let k = num("attention.key_length").or(head_dim)?;
    let v = num("attention.value_length").or(head_dim)?;
    let attn_layers = match num("full_attention_interval") {
        Some(n) if n > 1 => layers.div_ceil(n),
        _ => layers,
    };
    Some((k + v) * kv_heads * attn_layers * 2)
}

/// Memory available for the engine: `MemAvailable` plus free NVIDIA VRAM.
pub fn available_memory_bytes() -> Option<u64> {
    let mut total = None;
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        total = text
            .lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .map(|kb| kb * 1024);
    } else if cfg!(target_os = "macos") {
        // Unified memory: count half of physical RAM as usable.
        total = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map(|b| b / 2);
    }
    let vram = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse::<u64>().ok())
                .sum::<u64>()
                * 1024
                * 1024
        })
        .unwrap_or(0);
    total.map(|t| t + vram)
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub num_ctx: u32,
    /// Human-readable reason, shown in the banner / health check.
    pub reason: String,
    /// Set when the result is below the agentic minimum.
    pub warning: Option<String>,
}

pub struct Limits {
    /// 0 = auto.
    pub requested: u32,
    pub min: u32,
    pub max: u32,
}

const KV_OVERHEAD: f64 = 1.25; // compute buffers, fragmentation
const MEMORY_HEADROOM: f64 = 0.85; // leave room for the OS and other apps

fn round_down(n: u64) -> u32 {
    let step = 4096;
    ((n / step) * step).min(u32::MAX as u64) as u32
}

/// Decide `num_ctx` for an engine where the client chooses it (Ollama).
pub fn plan(limits: &Limits, meta: &ModelMeta, memory: Option<u64>) -> Plan {
    let model_max = meta.max_ctx.filter(|m| *m >= 2048);
    let mut reasons = Vec::new();
    let mut target = if limits.requested == 0 {
        reasons.push("auto".to_string());
        model_max.unwrap_or(limits.max).min(limits.max)
    } else if limits.requested < limits.min {
        reasons.push(format!(
            "raised from {} to the agentic minimum",
            limits.requested
        ));
        limits.min
    } else {
        reasons.push("configured".to_string());
        limits.requested
    };
    if let Some(m) = model_max {
        if target > m {
            target = m;
            reasons.push("model maximum".into());
        }
    }
    if let (Some(kv), Some(mem)) = (meta.kv_bytes_per_token, memory) {
        let usable = ((mem + meta.resident_bytes) as f64 * MEMORY_HEADROOM) as u64;
        let weights = meta.weights_bytes.unwrap_or(0);
        let per_tok = (kv as f64 * KV_OVERHEAD) as u64;
        let fit = usable.saturating_sub(weights) / per_tok.max(1);
        if (target as u64) > fit {
            target = round_down(fit).max(4096);
            reasons.push(format!(
                "limited by memory ({:.1} GiB usable)",
                usable as f64 / (1u64 << 30) as f64
            ));
        }
    }
    let warning = (target < limits.min).then(|| {
        if model_max.is_some_and(|m| m < limits.min) {
            format!(
                "this model supports only {target} tokens of context; agentic work needs >= {}. Prefer a long-context model (Qwen2.5/3, Llama 3.1+).",
                limits.min
            )
        } else {
            format!(
                "only {target} tokens of context fit in memory (agentic minimum {}). Free RAM/VRAM, use a smaller quantization, or enable KV-cache quantization (OLLAMA_KV_CACHE_TYPE=q8_0, OLLAMA_FLASH_ATTENTION=1).",
                limits.min
            )
        }
    });
    Plan {
        num_ctx: target.max(2048),
        reason: reasons.join(", "),
        warning,
    }
}

/// For servers that fix the context at load time (LM Studio, llama.cpp,
/// vLLM): adopt what the server reports, and warn if it is too small.
pub fn plan_server(limits: &Limits, provider: &str, detected: Option<u32>) -> Plan {
    match detected {
        Some(n) => Plan {
            num_ctx: n,
            reason: "reported by server".into(),
            warning: (n < limits.min).then(|| {
                let how = match provider {
                    "lmstudio" => "raise 'Context Length' when loading the model in LM Studio",
                    _ => "restart the server with a larger context (llama-server -c 65536, vLLM --max-model-len 65536)",
                };
                format!("the server loaded the model with only {n} tokens of context (agentic minimum {}): {how}.", limits.min)
            }),
        },
        None => {
            let n = if limits.requested == 0 { limits.min } else { limits.requested.max(limits.min) };
            Plan { num_ctx: n, reason: "assumed (server does not report it)".into(), warning: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const GIB: u64 = 1 << 30;

    fn limits(requested: u32) -> Limits {
        Limits {
            requested,
            min: 32_768,
            max: 262_144,
        }
    }

    #[test]
    fn kv_cost_for_hybrid_and_dense_models() {
        // Qwen3.5-9B: 32 layers, attention every 4th, 4 KV heads, 256 dims.
        let qwen35 = json!({
            "general.architecture": "qwen35", "qwen35.block_count": 32, "qwen35.attention.head_count": 16,
            "qwen35.attention.head_count_kv": 4, "qwen35.attention.key_length": 256,
            "qwen35.attention.value_length": 256, "qwen35.full_attention_interval": 4, "qwen35.embedding_length": 4096
        });
        assert_eq!(
            kv_bytes_per_token(qwen35.as_object().unwrap()),
            Some(32_768)
        );
        // Llama-3.1-8B: 32 layers, 8 KV heads, head dim 128 -> 128 KiB/token.
        let llama = json!({
            "general.architecture": "llama", "llama.block_count": 32, "llama.attention.head_count": 32,
            "llama.attention.head_count_kv": 8, "llama.embedding_length": 4096
        });
        assert_eq!(
            kv_bytes_per_token(llama.as_object().unwrap()),
            Some(131_072)
        );
    }

    #[test]
    fn auto_uses_model_max_when_memory_allows() {
        // Measured on an RTX 3050 laptop: 256K needs ~15 GB total; 20+ GB free.
        let meta = ModelMeta {
            max_ctx: Some(262_144),
            kv_bytes_per_token: Some(32_768),
            weights_bytes: Some(6 * GIB),
            resident_bytes: 0,
        };
        let p = plan(&limits(0), &meta, Some(26 * GIB));
        assert_eq!(p.num_ctx, 262_144);
        assert!(p.warning.is_none());
    }

    #[test]
    fn auto_shrinks_to_fit_small_machines() {
        // Dense 8B model on an 8 GB machine.
        let meta = ModelMeta {
            max_ctx: Some(131_072),
            kv_bytes_per_token: Some(131_072),
            weights_bytes: Some(5 * GIB),
            resident_bytes: 0,
        };
        let p = plan(&limits(0), &meta, Some(8 * GIB));
        assert!(p.num_ctx < 32_768 && p.num_ctx >= 4096, "{p:?}");
        assert!(p.warning.unwrap().contains("KV-cache quantization"));
        assert!(p.reason.contains("memory"));
    }

    #[test]
    fn explicit_small_values_are_raised_to_minimum() {
        let meta = ModelMeta {
            max_ctx: Some(131_072),
            ..Default::default()
        };
        let p = plan(&limits(16_384), &meta, None);
        assert_eq!(p.num_ctx, 32_768);
        assert!(p.reason.contains("minimum"));
    }

    #[test]
    fn short_context_models_warn() {
        let meta = ModelMeta {
            max_ctx: Some(8192),
            ..Default::default()
        };
        let p = plan(&limits(0), &meta, None);
        assert_eq!(p.num_ctx, 8192);
        assert!(p.warning.unwrap().contains("supports only"));
    }

    #[test]
    fn resident_model_memory_counts_as_available() {
        // The same model already loaded at 256K holds 15 GB; free shows only 14 GB.
        let meta = ModelMeta {
            max_ctx: Some(262_144),
            kv_bytes_per_token: Some(32_768),
            weights_bytes: Some(6 * GIB),
            resident_bytes: 15 * GIB,
        };
        assert_eq!(plan(&limits(0), &meta, Some(14 * GIB)).num_ctx, 262_144);
    }

    #[test]
    fn server_reported_context() {
        let p = plan_server(&limits(0), "lmstudio", Some(4096));
        assert_eq!(p.num_ctx, 4096);
        assert!(p.warning.unwrap().contains("LM Studio"));
        assert_eq!(plan_server(&limits(0), "custom", None).num_ctx, 32_768);
    }
}
