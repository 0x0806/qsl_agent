use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use moka::future::Cache;
use serde_json::Value;

use crate::error::{AgentError, Result};
use crate::security::SecurityGuard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolClass { ReadOnly, Mutating }

pub struct ToolOutcome { pub output: String, pub ok: bool }

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn class(&self) -> ToolClass;
    fn schema(&self) -> Value;
    fn validate(&self, _args: &Value) -> Result<()> { Ok(()) }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutcome>;
}

pub struct ToolContext {
    pub security: Arc<SecurityGuard>,
    pub max_output: usize,
    pub command_timeout_s: u64,
    pub read_cache: Cache<(String, u64), String>,
}

impl ToolContext {
    pub fn new(security: Arc<SecurityGuard>, max_output: usize, timeout_s: u64) -> Self {
        let read_cache = Cache::builder()
            .max_capacity(64 * 1024 * 1024)
            .time_to_live(Duration::from_secs(5))
            .build();
        Self { security, max_output, command_timeout_s: timeout_s, read_cache }
    }
    pub fn clone_for(&self) -> Self {
        Self {
            security: self.security.clone(),
            max_output: self.max_output,
            command_timeout_s: self.command_timeout_s,
            read_cache: self.read_cache.clone(),
        }
    }
}

pub struct ToolRegistry {
    tools: HashMap<&'static str, Arc<dyn Tool>>,
    schemas: Vec<Value>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new(), schemas: Vec::new() } }
    pub fn register(&mut self, t: Arc<dyn Tool>) {
        self.schemas.push(t.schema());
        self.tools.insert(t.name(), t);
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> { self.tools.get(name).cloned() }
    pub fn schemas(&self) -> Vec<Value> { self.schemas.clone() }
    pub fn names(&self) -> Vec<&'static str> { self.tools.keys().copied().collect() }
}

pub fn smart_clip(s: &str, max: usize) -> String {
    if s.len() <= max { return s.to_string(); }
    if max < 256 { return format!("{}…", &s[..max.saturating_sub(1)]); }
    let head_n = max * 6 / 10;
    let tail_n = max.saturating_sub(head_n + 60);
    let head_end = floor_char_boundary(s, head_n);
    let tail_start = floor_char_boundary(s, s.len().saturating_sub(tail_n));
    format!(
        "{}\n\n[… {} bytes elided …]\n\n{}",
        &s[..head_end],
        tail_start - head_end,
        &s[tail_start..],
    )
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() { return s.len(); }
    while idx > 0 && !s.is_char_boundary(idx) { idx -= 1; }
    idx
}

pub fn default_registry(sec: &Arc<SecurityGuard>, _cfg: &crate::config::Config) -> Result<ToolRegistry> {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(read_file::ReadFile));
    r.register(Arc::new(write_file::WriteFile));
    r.register(Arc::new(execute::ExecuteCommand));
    Ok(r)
}

pub mod read_file;
pub mod write_file;
pub mod execute;

pub use read_file::ReadFile;
pub use write_file::WriteFile;
pub use execute::ExecuteCommand;

#[allow(dead_code)]
fn _use_err(_: AgentError) {}
