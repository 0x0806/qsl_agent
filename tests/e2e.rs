//! End-to-end tests: run the real `qsl-agent` binary against the scripted
//! mock server, in an isolated HOME, over both wire protocols.

#[path = "common/mock_llm.rs"]
mod mock_llm;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Env {
    home: tempfile::TempDir,
    port: u16,
    state: mock_llm::MockState,
}

impl Env {
    async fn new() -> Self {
        let (port, state) = mock_llm::start(0).await;
        Self {
            home: tempfile::tempdir().unwrap(),
            port,
            state,
        }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.home.path().join(p)
    }

    /// Write a config for the given protocol ("openai" or "ollama").
    fn configure(&self, api: &str, extra: &str) {
        let (provider, url_key) = if api == "ollama" {
            ("ollama", "ollama_url")
        } else {
            ("custom", "custom_url")
        };
        let url = if api == "ollama" {
            format!("http://127.0.0.1:{}", self.port)
        } else {
            format!("http://127.0.0.1:{}/v1", self.port)
        };
        let cfg = format!(
            "provider = \"{provider}\"\n{url_key} = \"{url}\"\nmodel = \"{}\"\nobsidian_vault_path = \"{}\"\nstate_dir = \"{}\"\nmax_retries = 0\n{extra}\n",
            mock_llm::MODEL,
            self.path("vault").display(),
            self.path("state").display()
        );
        std::fs::write(self.path("config.toml"), cfg).unwrap();
    }

    async fn run(&self, args: &[&str]) -> Output {
        let bin = env!("CARGO_BIN_EXE_qsl-agent");
        let home = self.home.path().to_path_buf();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        tokio::task::spawn_blocking(move || {
            Command::new(bin)
                .args(&args)
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("XDG_DATA_HOME", home.join(".local/share"))
                .env("QSL_CONFIG", home.join("config.toml"))
                .env_remove("OBSIDIAN_VAULT")
                .env_remove("QSL_MODEL")
                .env_remove("QSL_PROVIDER")
                .env_remove("QSL_ENDPOINT")
                .env("NO_COLOR", "1")
                .current_dir(&home)
                .stdin(Stdio::null())
                .output()
                .expect("run qsl-agent")
        })
        .await
        .unwrap()
    }

    async fn json(&self, prompt: &str, extra_args: &[&str]) -> Value {
        let mut args = vec!["--json"];
        args.extend_from_slice(extra_args);
        args.push(prompt);
        let out = self.run(&args).await;
        let stdout = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str(stdout.lines().last().unwrap_or("")).unwrap_or_else(|e| {
            panic!(
                "bad JSON ({e}). stdout:\n{stdout}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }
}

fn reply(v: &Value) -> &str {
    assert_eq!(v["ok"], true, "turn failed: {v}");
    v["reply"].as_str().unwrap()
}

async fn both_protocols<F, Fut>(f: F)
where
    F: Fn(Env) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    for api in ["openai", "ollama"] {
        let env = Env::new().await;
        env.configure(api, "");
        f(env).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_chat_and_usage() {
    both_protocols(|env| async move {
        let v = env.json("ping", &[]).await;
        assert!(reply(&v).contains("Hello from the mock model"));
        assert_eq!(v["steps"], 1);
        assert!(
            v["tokens_in"].as_u64().unwrap() > 0,
            "usage must be captured: {v}"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn write_file_tool_round_trip() {
    both_protocols(|env| async move {
        let target = env.path("out/hello.txt");
        let v = env
            .json(&format!("[mock:write] create {}", target.display()), &[])
            .await;
        assert!(reply(&v).contains("TOOL_RESULT: Created"), "{v}");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "hello from qsl\n"
        );
        assert_eq!(v["tool_calls"], 1);
        // the tool result went back to the model with the right wire format
        let reqs = env.state.requests.lock().unwrap();
        let last = reqs.last().unwrap();
        let msgs = last["messages"].as_array().unwrap();
        assert!(
            msgs.iter().any(|m| m["role"] == "tool"),
            "tool message sent back"
        );
        assert!(
            last["tools"].as_array().is_some_and(|t| t.len() > 10),
            "tool schemas sent"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn parallel_read_only_tools() {
    let env = Env::new().await;
    env.configure("openai", "");
    let dir = env.path("proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "the needle is here\n").unwrap();
    let v = env
        .json(&format!("[mock:parallel] {}", dir.display()), &[])
        .await;
    assert_eq!(reply(&v), "RESULTS=2 NEEDLE=true");
}

#[tokio::test(flavor = "multi_thread")]
async fn hard_blocked_command_never_runs() {
    let env = Env::new().await;
    env.configure("openai", "");
    let v = env.json("[mock:danger]", &["--yolo"]).await;
    assert_eq!(
        reply(&v),
        "BLOCKED=true",
        "deny tier applies even in yolo mode"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn risky_command_denied_without_interactive_approval() {
    let env = Env::new().await;
    env.configure("openai", "");
    let v = env.json("[mock:risky]", &[]).await;
    assert_eq!(reply(&v), "DENIED=true");
}

#[tokio::test(flavor = "multi_thread")]
async fn shell_commands_execute() {
    both_protocols(|env| async move {
        let v = env.json("[mock:shell]", &[]).await;
        assert_eq!(reply(&v), "SHELL=true");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn think_blocks_are_stripped_from_answer() {
    both_protocols(|env| async move {
        let v = env.json("[mock:think]", &[]).await;
        assert_eq!(reply(&v), "The answer is 42.");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn repetition_guard_and_budget_synthesis() {
    let env = Env::new().await;
    env.configure("openai", "max_react_steps = 4");
    let v = env.json("[mock:loop]", &[]).await;
    assert_eq!(v["ok"], true, "{v}");
    // 4 steps + 15 auto-extension, then forced synthesis without tools
    let reqs = env.state.requests.lock().unwrap();
    assert!(
        reqs.last().unwrap().get("tools").is_none(),
        "final synthesis call must not offer tools"
    );
    let all: String = reqs.last().unwrap()["messages"].to_string();
    assert!(
        all.contains("[BLOCKED] identical"),
        "repeated calls are blocked"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn falls_back_to_text_tool_protocol() {
    both_protocols(|env| async move {
        let v = env.json("[mock:notools]", &[]).await;
        assert_eq!(reply(&v), "TEXT_PROTOCOL_OK=true");
        let reqs = env.state.requests.lock().unwrap();
        let sys = reqs.last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(sys.contains("Tool Calling Protocol"));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_tool_arguments_are_repaired() {
    let env = Env::new().await;
    env.configure("openai", "");
    let v = env.json("[mock:badargs]", &[]).await;
    assert_eq!(reply(&v), "ARGS_REPAIRED=true");
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_persists_across_runs_and_sessions_resume() {
    let env = Env::new().await;
    env.configure("openai", "");
    let v = env.json("[mock:memory]", &[]).await;
    assert_eq!(reply(&v), "MEMORY_SAVED");
    let mem = std::fs::read_to_string(env.path("vault/QSL_Agent/Memories.md")).unwrap();
    assert!(mem.contains("favourite language is Rust"));
    let v = env.json("[mock:recall]", &[]).await;
    assert_eq!(
        reply(&v),
        "RECALL=true",
        "memories are injected into the system prompt"
    );

    // --continue resumes the previous session's history
    let first = env.json("hello there", &[]).await;
    let sid = first["session_id"].as_str().unwrap().to_string();
    let v = env.json("second message", &["--continue"]).await;
    assert_eq!(v["session_id"], sid.as_str());
    assert!(
        reply(&v).contains("turns=2"),
        "resumed history is sent: {v}"
    );

    let out = env.run(&["--list-sessions"]).await;
    assert!(String::from_utf8_lossy(&out.stdout).contains(&sid));
    let daily = std::fs::read_dir(env.path("vault/QSL_Agent/Daily"))
        .unwrap()
        .count();
    assert_eq!(daily, 1, "daily journal written");
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_native_options_are_sent() {
    let env = Env::new().await;
    env.configure("ollama", "context_size = 32768");
    let v = env.json("ping", &[]).await;
    assert!(reply(&v).contains("Hello"));
    let reqs = env.state.requests.lock().unwrap();
    let opts = &reqs.last().unwrap()["options"];
    assert_eq!(
        opts["num_ctx"], 8192,
        "num_ctx clamps to the model's context_length from /api/show"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_utilities() {
    let env = Env::new().await;
    env.configure("openai", "");
    let out = env.run(&["--version"]).await;
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
    let out = env.run(&["--list-models"]).await;
    assert!(String::from_utf8_lossy(&out.stdout).contains(mock_llm::MODEL));
    let out = env.run(&["--health-check"]).await;
    assert!(
        out.status.success(),
        "health check: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = env.run(&["--show-config"]).await;
    assert!(String::from_utf8_lossy(&out.stdout).contains("max_react_steps"));
    let out = env.run(&["--approval", "bogus", "x"]).await;
    assert_eq!(out.status.code(), Some(2));
    let out = env.run(&["-q", "plain question"]).await;
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Hello from the mock model"));
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_engine_reports_error() {
    let env = Env::new().await;
    let cfg = format!(
        "provider = \"custom\"\ncustom_url = \"http://127.0.0.1:9/v1\"\nmodel = \"x\"\nobsidian_vault_path = \"{}\"\nstate_dir = \"{}\"\nmax_retries = 0\n",
        env.path("vault").display(),
        env.path("state").display()
    );
    std::fs::write(env.path("config.toml"), cfg).unwrap();
    let v = env.json("ping", &[]).await;
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("cannot reach"), "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_api_serves_chat() {
    let env = Env::new().await;
    env.configure("openai", "");
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let bin = env!("CARGO_BIN_EXE_qsl-agent");
    let home: &Path = env.home.path();
    let mut child = tokio::process::Command::new(bin)
        .args(["--serve", &format!("127.0.0.1:{port}")])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("QSL_CONFIG", home.join("config.toml"))
        .env("QSL_API_TOKEN", "s3cret")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..50 {
        if client.get(format!("{url}/health")).send().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ready, "server did not start");
    let unauth = client
        .post(format!("{url}/v1/chat"))
        .json(&serde_json::json!({"message": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);
    let r: Value = client
        .post(format!("{url}/v1/chat"))
        .bearer_auth("s3cret")
        .json(&serde_json::json!({"message": "hi"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert!(r["reply"]
        .as_str()
        .unwrap()
        .contains("Hello from the mock model"));
    child.kill().await.ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn long_sessions_compact_but_keep_current_request() {
    let env = Env::new().await;
    env.configure(
        "openai",
        "context_size = 8192\nmin_context_size = 8192\nmax_tokens = 1024",
    );
    let filler = "lorem ipsum dolor sit amet ".repeat(120);
    let mut sid = String::new();
    for i in 0..5 {
        let prompt = format!("turn-{i} {filler}");
        let args: Vec<&str> = if i == 0 { vec![] } else { vec!["--continue"] };
        let v = env.json(&prompt, &args).await;
        let r = reply(&v).to_string();
        assert!(
            r.contains(&format!("said=turn-{i}")),
            "current request must survive compaction: {r}"
        );
        if i == 0 {
            sid = v["session_id"].as_str().unwrap().to_string();
        }
        assert_eq!(v["session_id"], sid.as_str());
    }
    let summaries_before = {
        let reqs = env.state.requests.lock().unwrap();
        assert!(
            reqs.iter().any(|r| r["messages"][0]["content"]
                .as_str()
                .unwrap_or("")
                .starts_with("You are compressing")),
            "a summarisation request was made"
        );
        let sys = reqs.last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            sys.contains("Summary of Earlier Conversation"),
            "summary is carried in the system prompt"
        );
        reqs.len()
    };

    // A resumed session must restore the summary (a large window ensures
    // no new compaction happens in this run).
    env.configure("openai", "context_size = 131072\nmax_tokens = 1024");
    let v = env.json("short follow-up", &["--continue"]).await;
    assert!(reply(&v).contains("said=short follow-up"));
    let reqs = env.state.requests.lock().unwrap();
    let new: Vec<&Value> = reqs[summaries_before..].iter().collect();
    assert!(new.iter().all(|r| !r["messages"][0]["content"]
        .as_str()
        .unwrap_or("")
        .starts_with("You are compressing")));
    assert!(
        new.last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Summary of Earlier Conversation"),
        "summary restored on resume"
    );
}
