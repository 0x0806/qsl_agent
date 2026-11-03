//! Context-window management: token estimates, history repair, and choosing
//! where to compact without breaking tool-call / tool-result pairing.

use crate::provider::{Message, Role};
use crate::util::smart_clip;

pub fn estimate(history: &[Message]) -> usize {
    history.iter().map(Message::estimated_tokens).sum()
}

/// Repair history so every assistant tool call has exactly one result and no
/// result is orphaned. Needed after interrupts, crashes and compaction;
/// OpenAI-compatible servers reject unmatched pairs outright.
pub fn sanitize(history: &mut Vec<Message>) {
    let mut out: Vec<Message> = Vec::with_capacity(history.len());
    let mut i = 0;
    while i < history.len() {
        let m = &history[i];
        match m.role {
            Role::Assistant if !m.tool_calls.is_empty() => {
                let calls = m.tool_calls.clone();
                out.push(m.clone());
                i += 1;
                let mut results: Vec<Message> = Vec::new();
                while i < history.len() && history[i].role == Role::Tool {
                    results.push(history[i].clone());
                    i += 1;
                }
                for tc in &calls {
                    match results
                        .iter()
                        .position(|r| r.tool_call_id.as_deref() == Some(tc.id.as_str()))
                    {
                        Some(pos) => out.push(results.remove(pos)),
                        None => out.push(Message::tool(
                            &tc.id,
                            &tc.name,
                            "[no result: the call was interrupted before it ran]",
                        )),
                    }
                }
            }
            Role::Tool => {
                // Orphan result (its assistant call is gone) — drop it.
                i += 1;
            }
            Role::Assistant if m.content.trim().is_empty() => {
                i += 1;
            }
            _ => {
                out.push(m.clone());
                i += 1;
            }
        }
    }
    *history = out;
}

/// Pick the index where the preserved tail starts. Everything in
/// `1..split` gets summarised. Returns `None` when nothing can be compacted.
pub fn compaction_split(history: &[Message], keep_tail_tokens: usize) -> Option<usize> {
    if history.len() < 3 {
        return None;
    }
    let mut tail_tokens = 0usize;
    let mut best: Option<usize> = None;
    for i in (2..history.len()).rev() {
        tail_tokens += history[i].estimated_tokens();
        let boundary = history[i].role != Role::Tool;
        if boundary && history.len() - i >= 2 {
            if tail_tokens > keep_tail_tokens && best.is_some() {
                break;
            }
            best = Some(i);
            if tail_tokens > keep_tail_tokens {
                break;
            }
        }
    }
    best.filter(|&s| s > 1)
}

/// Render messages as compact text for the summariser.
pub fn transcript_for_summary(messages: &[Message]) -> String {
    let mut s = String::new();
    for m in messages {
        match m.role {
            Role::User => s.push_str(&format!(
                "[user]\n{}\n\n",
                smart_clip(m.content.trim(), 4000)
            )),
            Role::Assistant => {
                if !m.content.trim().is_empty() {
                    s.push_str(&format!(
                        "[assistant]\n{}\n\n",
                        smart_clip(m.content.trim(), 3000)
                    ));
                }
                for tc in &m.tool_calls {
                    s.push_str(&format!(
                        "[assistant called {}] {}\n\n",
                        tc.name,
                        smart_clip(&tc.arguments, 600)
                    ));
                }
            }
            Role::Tool => s.push_str(&format!(
                "[result of {}]\n{}\n\n",
                m.name.clone().unwrap_or_default(),
                smart_clip(m.content.trim(), 1500)
            )),
            Role::System => s.push_str(&format!(
                "[system note]\n{}\n\n",
                smart_clip(m.content.trim(), 1000)
            )),
        }
    }
    s
}

/// Last-resort shrink when summarisation is impossible: clip large tool
/// outputs everywhere except the most recent `protect` messages.
pub fn clip_tool_outputs(history: &mut [Message], max_chars: usize, protect: usize) -> usize {
    let n = history.len();
    let mut saved = 0;
    for m in history.iter_mut().take(n.saturating_sub(protect)) {
        if m.role == Role::Tool && m.content.len() > max_chars {
            let before = m.content.len();
            m.content = smart_clip(&m.content, max_chars);
            saved += before.saturating_sub(m.content.len());
        }
    }
    saved
}

pub const SUMMARY_PROMPT: &str = "You are compressing the earlier part of a conversation between a user and an autonomous \
coding/ops agent so the agent can continue the task with a smaller context.\n\
Write a dense summary in Markdown bullet points (max ~450 words) covering:\n\
- The user's goals and explicit requirements/preferences\n\
- Decisions made and their reasons\n\
- Files read, created or modified (exact paths) and what changed\n\
- Commands run and their important results (errors, test outcomes, versions)\n\
- Problems hit and how they were resolved\n\
- Current state of the work and the concrete next steps\n\
Keep exact identifiers (paths, function names, URLs, numbers). Do not invent anything. Output only the summary.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolCall;

    fn call(id: &str) -> Message {
        let mut m = Message::assistant("");
        m.tool_calls = vec![ToolCall {
            id: id.into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        }];
        m
    }

    #[test]
    fn sanitize_fills_missing_and_drops_orphans() {
        let mut h = vec![
            Message::system("s"),
            Message::user("u"),
            Message::tool("ghost", "x", "orphan"),
            call("a"),
            Message::user("next"),
            Message::assistant(""),
        ];
        sanitize(&mut h);
        let roles: Vec<Role> = h.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![
                Role::System,
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::User
            ]
        );
        assert!(h[3].content.contains("interrupted"));
        assert_eq!(h[3].tool_call_id.as_deref(), Some("a"));
    }

    #[test]
    fn sanitize_reorders_results_to_match_calls() {
        let mut a = Message::assistant("");
        a.tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "x".into(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "2".into(),
                name: "y".into(),
                arguments: "{}".into(),
            },
        ];
        let mut h = vec![
            Message::system("s"),
            Message::user("u"),
            a,
            Message::tool("2", "y", "B"),
            Message::tool("1", "x", "A"),
        ];
        sanitize(&mut h);
        assert_eq!(h[3].content, "A");
        assert_eq!(h[4].content, "B");
    }

    #[test]
    fn split_never_starts_on_tool_result() {
        let mut h = vec![Message::system("s"), Message::user("x".repeat(4000))];
        for i in 0..10 {
            h.push(call(&i.to_string()));
            h.push(Message::tool(&i.to_string(), "read_file", "y".repeat(2000)));
        }
        h.push(Message::assistant("done"));
        let split = compaction_split(&h, 1500).unwrap();
        assert!(split > 1);
        assert_ne!(h[split].role, Role::Tool);
        assert!(h.len() - split >= 2);
    }

    #[test]
    fn nothing_to_compact_for_short_history() {
        let h = vec![Message::system("s"), Message::user("u")];
        assert!(compaction_split(&h, 10).is_none());
    }

    #[test]
    fn clip_outputs_protects_recent() {
        let mut h = vec![
            Message::tool("1", "x", "z".repeat(10_000)),
            Message::tool("2", "x", "z".repeat(10_000)),
        ];
        let saved = clip_tool_outputs(&mut h, 1000, 1);
        assert!(saved > 8000);
        assert_eq!(h[1].content.len(), 10_000);
    }
}
