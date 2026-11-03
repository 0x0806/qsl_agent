use std::sync::atomic::{AtomicU64, Ordering};

pub struct CostTracker {
    pub input_price_per_m: f64,
    pub output_price_per_m: f64,
    input: AtomicU64,
    output: AtomicU64,
}

impl CostTracker {
    pub fn new(input_price_per_m: f64, output_price_per_m: f64) -> Self {
        Self {
            input_price_per_m,
            output_price_per_m,
            input: AtomicU64::new(0),
            output: AtomicU64::new(0),
        }
    }
    pub fn record(&self, i: u32, o: u32) {
        self.input.fetch_add(i as u64, Ordering::Relaxed);
        self.output.fetch_add(o as u64, Ordering::Relaxed);
        metrics::counter!("qsl_tokens_total", "direction" => "input").increment(i as u64);
        metrics::counter!("qsl_tokens_total", "direction" => "output").increment(o as u64);
    }
    pub fn totals(&self) -> (u64, u64) {
        (self.input.load(Ordering::Relaxed), self.output.load(Ordering::Relaxed))
    }
    pub fn estimated_usd(&self) -> f64 {
        let (i, o) = self.totals();
        let usd = (i as f64 * self.input_price_per_m + o as f64 * self.output_price_per_m) / 1_000_000.0;
        metrics::gauge!("qsl_cost_usd").set(usd);
        usd
    }
}
