//! Interactive REPL. Parses input, dispatches to AgentHandle, prints via Printer.
//!
//! Slash commands are forwarded to the agent as text (the agent's
//! `handle_line` handles `/health`, `/stats`, `/compact`, `/retry`, `/undo`,
//! `/export`, `/save`). Local-only: `/help`, `/quit`, `/skills`, `/skill`.

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, stdin};
use tracing::warn;

use crate::agent::AgentHandle;
use crate::error::Result;
use crate::skills::SkillRegistry;
use crate::ui::Printer;

pub async fn run(
    agent: AgentHandle,
    printer: Printer,
    skills: Option<Arc<SkillRegistry>>,
    shutdown: tokio_util::sync::CancellationToken,
) -> Result<()> {
    let mut lines = BufReader::new(stdin()).lines();

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let line = line.trim();
                if line.is_empty() { continue; }

                match parse_local(line, skills.as_deref()) {
                    Local::Quit => break,
                    Local::Help => printer.help(),
                    Local::Skills(names) => {
                        printer.info("Available skills:");
                        for n in names { println!("  {n}"); }
                    }
                    Local::Skill(Some(name)) => {
                        printer.info(&format!("skill '{name}' activated for next turn"));
                        // Forward to agent as a text hint — agent prepends it to the next user message
                        let _ = agent.chat(format!("/skill {name}")).await;
                    }
                    Local::Skill(None) => printer.warning("usage: /skill NAME"),
                    Local::Forward(text) => {
                        match agent.chat(text).await {
                            Ok(reply) => if !reply.is_empty() { printer.reply(&reply); },
                            Err(e) => printer.error(&e),
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

enum Local {
    Quit,
    Help,
    Skills(Vec<&'static str>),
    Skill(Option<String>),
    Forward(String),
}

fn parse_local(line: &str, skills: Option<&SkillRegistry>) -> Local {
    match line {
        "/quit" | "/exit" | ":q" => return Local::Quit,
        "/help" | "?"            => return Local::Help,
        _ => {}
    }

    if line == "/skills" {
        let names = skills.map(|s| s.names()).unwrap_or_default();
        return Local::Skills(names);
    }

    if let Some(rest) = line.strip_prefix("/skill ") {
        let name = rest.trim();
        if name.is_empty() { return Local::Skill(None); }
        return Local::Skill(Some(name.to_string()));
    }

    Local::Forward(line.to_string())
}

#[allow(dead_code)]
fn _warn_use() { warn!("cli module"); }
