//! Terminal output formatting. Zero-dep ANSI colors.

use crate::agent::{HealthSnapshot, SessionStats};
use crate::error::AgentError;

#[derive(Clone, Copy)]
pub struct Printer {
    pub color: bool,
    pub verbose: bool,
}

impl Printer {
    pub fn new(color: bool, verbose: bool) -> Self {
        Self { color, verbose }
    }

    fn style(&self, code: &str, text: &str) -> String {
        if self.color { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
    }

    pub fn reply(&self, text: &str) {
        println!("{text}");
    }

    pub fn info(&self, text: &str) {
        println!("{}", self.style("36", text));
    }

    pub fn warning(&self, text: &str) {
        println!("{}", self.style("33", text));
    }

    pub fn error(&self, e: &AgentError) {
        eprintln!("{} {}", self.style("31", "error:"), e);
    }

    pub fn tool_call(&self, name: &str, args: &serde_json::Value) {
        if self.verbose {
            let compact = compact_json(args);
            println!("{}", self.style("90", &format!("→ {name}({compact})")));
        }
    }

    pub fn tool_result(&self, name: &str, ok: bool) {
        if self.verbose {
            let (sym, col) = if ok { ("✓", "32") } else { ("✗", "31") };
            println!("{}", self.style(col, &format!("  {sym} {name}")));
        }
    }

    pub fn health(&self, h: &HealthSnapshot) {
        let status = if h.stalled { self.style("31", "STALLED") } else { self.style("32", "OK") };
        println!(
            "session={} status={} idle={}s steps={} ctx={:.1}% tokens={}/{} cost=${:.4}",
            h.session_id, status, h.idle_seconds, h.steps, h.context_pct,
            h.tokens_in, h.tokens_out, h.cost_usd,
        );
    }

    pub fn stats(&self, s: &SessionStats) {
        println!("session    : {}", s.session_id);
        println!("turn       : {}", s.turn_idx);
        println!("messages   : {}", s.message_count);
        println!("context    : {} tokens ({:.1}%)", s.context_tokens, s.context_pct);
        println!("tokens     : {} in / {} out", s.tokens_in, s.tokens_out);
        println!("cost       : ${:.4}", s.cost_usd);
        println!("snapshots  : {}", s.snapshots_available);
        println!("tools      : {}", s.tools_available.join(", "));
    }

    pub fn help(&self) {
        println!("{}", self.style("1", "Commands:"));
        let rows = [
            ("/help",              "show this"),
            ("/health",            "idle, steps, context %, cost"),
            ("/stats",             "detailed session stats (JSON)"),
            ("/compact",           "force context compaction"),
            ("/retry",             "re-run last turn"),
            ("/undo N",            "rewind N turns"),
            ("/export md|json",    "dump session to stdout"),
            ("/save PATH",         "write session JSON to file"),
            ("/skills",            "list available skills"),
            ("/skill NAME",        "activate a skill for the next turn"),
            ("/quit",              "exit"),
        ];
        let w = rows.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
        for (cmd, desc) in rows {
            println!("  {:<w$}  {}", self.style("36", cmd), desc, w = w);
        }
    }
}

fn compact_json(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() <= 80 { s } else { format!("{}…", &s[..79]) }
}
