use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use crate::config::Tier;
use crate::error::{AgentError, Result};
use crate::provider::{ChatMessage, Provider, Role};
use crate::memory::Memory;

pub struct ContextManager {
    budget: usize,
    threshold: f32,
    raw_turns: usize,
    summary: Option<String>,
    tier: Tier,
}

impl ContextManager {
    pub fn new(tier: Tier) -> Self {
        Self {
            budget: tier.context_budget(),
            threshold: tier.compaction_threshold(),
            raw_turns: tier.raw_turns_kept(),
            summary: None,
            tier,
        }
    }

    pub fn tokens(messages: &[ChatMessage]) -> usize {
        messages.iter().map(|m| m.token_count() as usize).sum()
    }

    pub fn utilization(&self, messages: &[ChatMessage]) -> f32 {
        Self::tokens(messages) as f32 / self.budget as f32
    }

    pub fn needs_compaction(&self, messages: &[ChatMessage]) -> bool {
        self.utilization(messages) >= self.threshold
    }

    pub fn assert_well_formed(&self, messages: &[ChatMessage]) -> Result<()> {
        let mut open_calls = std::collections::HashSet::new();
        for m in messages {
            if let Some(tcs) = &m.tool_calls {
                for tc in tcs { open_calls.insert(tc.id.clone()); }
            }
            if let Some(id) = &m.tool_call_id { open_calls.remove(id); }
        }
        if !open_calls.is_empty() {
            return Err(AgentError::ContextOverflow {
                used: Self::tokens(messages),
                limit: self.budget,
            });
        }
        Ok(())
    }

    pub fn assert_fits(&self, messages: &[ChatMessage]) -> Result<()> {
        let used = Self::tokens(messages);
        if used > self.budget {
            return Err(AgentError::ContextOverflow { used, limit: self.budget });
        }
        Ok(())
    }

    pub async fn compact(
        &mut self,
        messages: &mut Vec<ChatMessage>,
        memory: &Memory,
        provider: &Provider,
    ) -> Result<()> {
        if messages.len() <= self.raw_turns { return Ok(()); }

        let split_at = messages.len().saturating_sub(self.raw_turns);
        let split_at = adjust_to_turn_boundary(messages, split_at);

        let to_summarize: Vec<ChatMessage> = messages.drain(..split_at).collect();
        let text = to_summarize.iter()
            .map(|m| format!("[{:?}] {}", m.role, m.content))
            .collect::<Vec<_>>().join("\n");

        let new_summary = match summarize_with_model(&text, provider, self.tier).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "summarizer failed; falling back to truncation");
                truncate_chars(&text, 2_000)
            }
        };
        self.summary = Some(format!(
            "{}\n---\n{}",
            self.summary.as_deref().unwrap_or(""),
            new_summary,
        ));

        messages.insert(0, ChatMessage::system(format!(
            "[Rolling summary of earlier turns]\n{}",
            self.summary.as_deref().unwrap()
        )));

        memory.append_compaction_event(messages.len(), Self::tokens(messages)).await?;
        Ok(())
    }
}

async fn summarize_with_model(text: &str, provider: &Provider, tier: Tier) -> Result<String> {
    let max_chars = match tier {
        Tier::Small  => 4_000,
        Tier::Medium => 8_000,
        Tier::Large  => 16_000,
    };
    let prompt = format!(
        "Summarize the following conversation excerpt in <= {max_chars} chars. \
         Preserve: user intent, decisions, tool outcomes, open questions. \
         Drop: politeness, redundancy, raw tool output details.\n\n---\n{text}"
    );
    let req = crate::provider::ChatRequest {
        messages: vec![
            ChatMessage::system("You are a conversation summarizer. Be terse and factual."),
            ChatMessage::user(prompt),
        ],
        tools: vec![],
        temperature: 0.1,
        stream: false,
    };
    let (msg, _, _, _) = provider.chat(req).await?;
    Ok(truncate_chars(&msg.content, max_chars))
}

fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { s.chars().take(n).collect::<String>() + "…" }
}

fn adjust_to_turn_boundary(messages: &[ChatMessage], mut idx: usize) -> usize {
    while idx > 0 {
        let m = &messages[idx];
        if m.tool_calls.is_none() && m.tool_call_id.is_none()
            && matches!(m.role, Role::User | Role::Assistant)
        {
            return idx;
        }
        idx -= 1;
    }
    0
}

pub fn dedup_tool_results(messages: &mut [ChatMessage]) -> usize {
    let mut seen: HashMap<u64, usize> = HashMap::new();
    let mut replaced = 0;
    for i in 0..messages.len() {
        if messages[i].tool_call_id.is_some() && messages[i].content.len() > 400 {
            let h = hash_str(&messages[i].content);
            if let Some(&prev) = seen.get(&h) {
                let orig_len = messages[i].content.len();
                messages[i].content = format!(
                    "[duplicate of tool result at msg #{prev} — {orig_len} bytes elided]");
                replaced += 1;
            } else {
                seen.insert(h, i);
            }
        }
    }
    replaced
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
