use std::sync::LazyLock;
use metrics::{counter, gauge, histogram};

static STARTED: LazyLock<()> = LazyLock::new(|| {
    counter!("qsl_starts_total").increment(1);
});

pub fn init() {
    LazyLock::force(&STARTED);
}

pub fn turn_completed(turn_idx: u64, latency_ms: f64, tokens_in: u32, tokens_out: u32) {
    counter!("qsl_turns_total").increment(1);
    histogram!("qsl_turn_latency_ms").record(latency_ms);
    counter!("qsl_tokens_in_total").increment(tokens_in as u64);
    counter!("qsl_tokens_out_total").increment(tokens_out as u64);
    gauge!("qsl_last_turn_idx").set(turn_idx as f64);
}

pub fn tool_executed(name: &str, ok: bool, latency_ms: f64) {
    counter!("qsl_tools_total", "name" => name.to_string(), "ok" => ok.to_string()).increment(1);
    histogram!("qsl_tool_latency_ms", "name" => name.to_string()).record(latency_ms);
}

pub fn context_utilization(pct: f32) {
    gauge!("qsl_context_utilization_pct").set(pct as f64);
}

pub fn circuit_open(provider: &str) {
    counter!("qsl_circuit_open_total", "provider" => provider.to_string()).increment(1);
}
