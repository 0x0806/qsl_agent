//! `execute_command`: runs `bash -c` in its own process group so timeouts
//! and Ctrl+C terminate the whole process tree, not just the shell.

use serde_json::Value;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::{arg_str, arg_u64, req_str, ToolCtx};
use crate::util::smart_clip;

/// Kills the process group on drop (covers interrupt and timeout paths).
struct GroupGuard(Option<i32>);

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if let Some(pgid) = self.0.take() {
            #[cfg(unix)]
            // SAFETY: killpg only sends a signal; a stale pgid yields ESRCH.
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
}

fn shell_program() -> &'static str {
    if std::path::Path::new("/bin/bash").exists() || std::path::Path::new("/usr/bin/bash").exists()
    {
        "bash"
    } else {
        "sh"
    }
}

const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

async fn read_capped<R: tokio::io::AsyncRead + Unpin>(mut r: R) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    let mut overflow = false;
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.len() < CAPTURE_LIMIT {
                    out.extend_from_slice(&buf[..n.min(CAPTURE_LIMIT - out.len())]);
                } else {
                    overflow = true;
                }
            }
        }
    }
    (out, overflow)
}

pub async fn execute_command(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let cmd = req_str(args, &["command", "cmd", "script", "shell"])?;
    let timeout_s = arg_u64(args, &["timeout", "timeout_secs"])
        .unwrap_or(ctx.command_timeout_secs)
        .clamp(1, 3600);
    let workdir = match arg_str(args, &["workdir", "cwd", "dir", "directory"]) {
        Some(w) if !w.trim().is_empty() => {
            let p = ctx.security.check_read(&w)?;
            if !p.is_dir() {
                return Err(format!("workdir '{}' is not a directory", p.display()));
            }
            Some(p)
        }
        _ => None,
    };

    let mut command = Command::new(shell_program());
    command
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    if let Some(w) = &workdir {
        command.current_dir(w);
    }
    #[cfg(unix)]
    command.process_group(0);

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to start shell: {e}"))?;
    let _guard = GroupGuard(child.id().map(|p| p as i32));
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let out_task = tokio::spawn(read_capped(stdout));
    let err_task = tokio::spawn(read_capped(stderr));

    let status = tokio::select! {
        s = child.wait() => s.map_err(|e| format!("wait failed: {e}"))?,
        _ = tokio::time::sleep(Duration::from_secs(timeout_s)) => {
            drop(_guard);
            let _ = child.kill().await;
            let (o, _) = out_task.await.unwrap_or_default();
            let partial = String::from_utf8_lossy(&o);
            return Err(format!(
                "command timed out after {timeout_s}s and was killed. Partial output:\n{}\nHint: pass a larger `timeout`, run it in the background (`nohup ... &`), or narrow the command.",
                smart_clip(partial.trim(), 4000)
            ));
        }
        _ = ctx.interrupt.cancelled() => {
            return Err("command interrupted by user".into());
        }
    };
    let (out, o_over) = out_task.await.unwrap_or_default();
    let (err, e_over) = err_task.await.unwrap_or_default();
    let stdout = String::from_utf8_lossy(&out);
    let stderr = String::from_utf8_lossy(&err);
    let code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "signal".into());
    let elapsed = started.elapsed().as_secs_f32();

    let budget = ctx.max_output_chars;
    let mut result = format!("Exit Code: {code} ({elapsed:.1}s)\n");
    let stdout_t = stdout.trim_end();
    let stderr_t = stderr.trim_end();
    if stderr_t.is_empty() {
        result.push_str(&smart_clip(stdout_t, budget));
    } else if stdout_t.is_empty() {
        result.push_str("STDERR:\n");
        result.push_str(&smart_clip(stderr_t, budget));
    } else {
        result.push_str("STDOUT:\n");
        result.push_str(&smart_clip(stdout_t, budget * 7 / 10));
        result.push_str("\nSTDERR:\n");
        result.push_str(&smart_clip(stderr_t, budget * 3 / 10));
    }
    if o_over || e_over {
        result.push_str("\n[output exceeded 4 MB capture limit; remainder discarded]");
    }
    if stdout_t.is_empty() && stderr_t.is_empty() {
        result.push_str("(no output)");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tests_support::Harness;
    use serde_json::json;

    #[tokio::test]
    async fn runs_and_reports_exit_code() {
        let h = Harness::new();
        let r = execute_command(
            &json!({"command": "echo hello; echo oops >&2; exit 3"}),
            &h.ctx(),
        )
        .await
        .unwrap();
        assert!(r.starts_with("Exit Code: 3"));
        assert!(r.contains("hello") && r.contains("oops"));
    }

    #[tokio::test]
    async fn timeout_kills_process_tree() {
        let h = Harness::new();
        let marker = h.dir.path().join("survivor");
        let cmd = format!("(sleep 2; touch {}) & sleep 30", marker.display());
        let started = Instant::now();
        let r = execute_command(&json!({"command": cmd, "timeout": 1}), &h.ctx()).await;
        assert!(r.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(
            !marker.exists(),
            "background child must be killed with the group"
        );
    }

    #[tokio::test]
    async fn workdir_is_respected() {
        let h = Harness::new();
        let r = execute_command(
            &json!({"cmd": "pwd", "workdir": h.dir.path().to_string_lossy()}),
            &h.ctx(),
        )
        .await
        .unwrap();
        assert!(r.contains(&*h.dir.path().canonicalize().unwrap().to_string_lossy()));
    }

    #[tokio::test]
    async fn interrupt_stops_command() {
        let h = Harness::new();
        let i = h.interrupt.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            i.trigger();
        });
        let r = execute_command(&json!({"command": "sleep 20"}), &h.ctx()).await;
        assert!(r.unwrap_err().contains("interrupted"));
    }
}
