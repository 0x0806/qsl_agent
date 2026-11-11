//! Optional frontier (hosted) model providers, and the consent that goes with
//! them. QSL Agent is local-first: nothing leaves the machine unless the user
//! switches to one of these presets (or points `custom` at a remote server) and
//! explicitly accepts that their data will be sent to that company.

/// A hosted model service reachable through an OpenAI-compatible chat API.
pub struct Preset {
    /// Value of the `provider` setting.
    pub id: &'static str,
    /// Product name users know.
    pub product: &'static str,
    /// Company that receives the data.
    pub company: &'static str,
    pub base_url: &'static str,
    /// Environment variable holding the API key.
    pub key_env: &'static str,
    pub note: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "openai",
        product: "ChatGPT models (OpenAI API)",
        company: "OpenAI",
        base_url: "https://api.openai.com/v1",
        key_env: "OPENAI_API_KEY",
        note: "OpenAI states that data sent to its API is not used to train its models unless you opt in, and that abuse-monitoring logs are kept for up to 30 days by default; zero data retention needs OpenAI's approval.",
    },
    Preset {
        id: "anthropic",
        product: "Claude models (Anthropic API)",
        company: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        key_env: "ANTHROPIC_API_KEY",
        note: "Anthropic states that commercial API inputs and outputs are not used for training by default and are deleted within 30 days (longer if flagged for policy violations, or by agreement). It describes its OpenAI-compatible endpoint, which this uses, as primarily for testing and comparison rather than long-term production use; some request fields are ignored.",
    },
    Preset {
        id: "gemini",
        product: "Gemini models (Google AI API)",
        company: "Google",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        key_env: "GEMINI_API_KEY",
        note: "Google's terms depend on the quota behind your key. Unpaid quota: content may be used to improve Google products and machine learning, and human reviewers may read it; Google says not to submit sensitive, confidential or personal information. Paid quota: not used to improve Google products, but logged for a limited period to detect abuse. Google marks its OpenAI compatibility as beta.",
    },
    Preset {
        id: "openrouter",
        product: "OpenRouter (many vendors' models)",
        company: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        key_env: "OPENROUTER_API_KEY",
        note: "A router: your data goes to OpenRouter and onward to whichever vendor serves the model you pick, and that vendor's own retention and training policy applies. OpenRouter's account privacy settings can restrict routing to providers that do not train on data.",
    },
];

/// Providers a user can select by name.
pub const ALL_PROVIDERS: &[&str] = &[
    "ollama",
    "lmstudio",
    "custom",
    "openai",
    "anthropic",
    "gemini",
    "openrouter",
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Host part of a URL (`https://api.openai.com/v1` -> `api.openai.com`).
pub fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.to_lowercase()
}

/// True when `host` is this machine or a private/LAN address.
pub fn is_local_host(host: &str) -> bool {
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return true;
    }
    if host == "::1"
        || host.starts_with("fe80:")
        || host.starts_with("fc")
        || host.starts_with("fd")
    {
        return host.contains(':');
    }
    let parts: Vec<u8> = host.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() == 4 && host.split('.').count() == 4 {
        return match (parts[0], parts[1]) {
            (127, _) | (10, _) | (192, 168) | (169, 254) => true,
            (172, b) => (16..=31).contains(&b),
            _ => false,
        };
    }
    false
}

/// The disclaimer shown before any data is sent to a hosted service.
pub fn disclaimer(company: &str, host: &str, product: &str, note: &str) -> Vec<String> {
    let mut v = vec![
        format!("You are about to send your data to {company} ({host}) - {product}."),
        "From then on, what you type, the contents of files the agent reads, command output and tool results are sent over the internet to that service.".to_string(),
        "It is handled under that company's own terms, privacy policy and retention rules, which QSL Agent and Quantum Secular Labs do not control. Usage may be billed to your account with them.".to_string(),
        "QSL Agent masks common secrets in what it shows and stores and, for hosted engines, in tool output sent to the model, but masking is best effort. Do not use a hosted engine with code, credentials or personal data you are not allowed to share.".to_string(),
        "The local-first guarantee (prompts go only to a server you run) does not apply while a hosted engine is selected. Switch back with /provider ollama.".to_string(),
    ];
    if !note.is_empty() {
        v.push(format!("Vendor statements (from their published documentation, checked 2026-11-11, and subject to change): {note}"));
        v.push("These are summaries, not guarantees. Read the vendor's current terms and your agreement with them before relying on any of it.".to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host_of("http://127.0.0.1:11434/api/chat"), "127.0.0.1");
        assert_eq!(host_of("http://user:pw@Example.com:8080/x"), "example.com");
        assert_eq!(host_of("http://[::1]:8080/v1"), "::1");
    }

    #[test]
    fn local_vs_remote_hosts() {
        for h in [
            "localhost",
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.5",
            "172.16.0.1",
            "172.31.255.1",
            "::1",
            "box.local",
        ] {
            assert!(is_local_host(h), "{h} should be local");
        }
        for h in [
            "api.openai.com",
            "8.8.8.8",
            "172.32.0.1",
            "172.15.0.1",
            "192.169.0.1",
            "fcdn.example.com",
            "example.com",
        ] {
            assert!(!is_local_host(h), "{h} should be remote");
        }
    }

    #[test]
    fn presets_are_consistent() {
        for p in PRESETS {
            assert!(p.base_url.starts_with("https://"));
            assert!(ALL_PROVIDERS.contains(&p.id));
            assert!(p.key_env.ends_with("_API_KEY"));
        }
        assert!(preset("anthropic").is_some() && preset("ollama").is_none());
    }

    #[test]
    fn disclaimer_names_the_company_and_data() {
        let d = disclaimer("OpenAI", "api.openai.com", "ChatGPT models", "").join(" ");
        assert!(
            d.contains("OpenAI")
                && d.contains("api.openai.com")
                && d.contains("sent over the internet")
        );
    }
}
