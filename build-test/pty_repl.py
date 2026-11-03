#!/usr/bin/env python3
"""Interactive REPL test suite: drives qsl-agent through a real pseudo-terminal
against the mock LLM.  Usage: pty_repl.py <qsl-agent binary> <mock port>"""
import os, pty, re, select, shutil, signal, subprocess, sys, tempfile, time

BIN, PORT = sys.argv[1], int(sys.argv[2])
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\r")
home = tempfile.mkdtemp(prefix="qsl-pty-")
cfg = f"""provider = "custom"
custom_url = "http://127.0.0.1:{PORT}/v1"
model = "mock-coder:7b"
obsidian_vault_path = "{home}/vault"
state_dir = "{home}/state"
max_retries = 0
"""
open(f"{home}/config.toml", "w").write(cfg)
env = dict(os.environ, HOME=home, XDG_CONFIG_HOME=f"{home}/.config", XDG_STATE_HOME=f"{home}/.local/state",
           QSL_CONFIG=f"{home}/config.toml", TERM="xterm-256color", COLUMNS="100", LINES="40")
env.pop("NO_COLOR", None)

pid, fd = pty.fork()
if pid == 0:
    os.chdir(home)
    os.execve(BIN, [BIN], env)

buf = ""
def read(timeout=0.3):
    global buf
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                data = os.read(fd, 65536).decode(errors="replace")
            except OSError:
                return
            buf += data
            end = max(end, time.time() + 0.15)

def expect(pattern, timeout=8):
    global buf
    end = time.time() + timeout
    while time.time() < end:
        read(0.2)
        clean = ANSI.sub("", buf)
        m = re.search(pattern, clean)
        if m:
            out = clean[: m.end()]
            buf = ""
            global last_out
            last_out = out
            return out
    print("TIMEOUT waiting for", repr(pattern), "\n----- screen -----\n", ANSI.sub("", buf)[-3000:])
    os.kill(pid, signal.SIGKILL)
    sys.exit(1)

def send(s):
    os.write(fd, s.encode())

results = []
last_out = ""
def check(name, cond):
    results.append((name, cond))
    print(("PASS " if cond else "FAIL ") + name)
    if not cond:
        print("   >>> saw:", repr(last_out[-600:]))

out = expect(r"qsl-agent > $")
check("banner shows title", "QSL AGENT — AUTONOMOUS LOCAL INTELLIGENCE" in out)
check("banner shows model", "mock-coder:7b" in out)
check("banner shows session", "Session:" in out)

send("ping\r")
out = expect(r"qsl-agent > $")
check("streamed RESPONSE frame", "[RESPONSE]" in out and "Hello from the mock model" in out)
check("status line", re.search(r"\[\d+\.\ds \| 1 step", out) is not None)

target = f"{home}/work/note.txt"
send(f"[mock:write] {target}\r")
out = expect(r"permission \[y/a/n/d\]")
check("permission review shown", "PERMISSION REVIEW" in out and "write_file" in out and "Target:" in out)
send("y\r")
out = expect(r"qsl-agent > $")
check("file written after approval", os.path.exists(target))
check("result card", "[RESULT: write_file]" in out and "[ACTION: write_file]" not in out)

send("/rollback\r")
out = expect(r"qsl-agent > $")
check("rollback removes created file", not os.path.exists(target) and "Rolled back 1 file" in out)

send(f"[mock:write] {target}\r")
expect(r"permission \[y/a/n/d\]")
send("n\r")
out = expect(r"qsl-agent > $")
check("deny is respected", not os.path.exists(target) and "DENIED" in out)

send("[mock:risky]\r")
out = expect(r"permission \[y/a/n/d\]")
check("risky command flagged", "Risk:" in out and "deletes files" in out)
send("\r")
out = expect(r"qsl-agent > $")
check("enter defaults to deny", "DENIED=true" in out)

send("[mock:think]\r")
out = expect(r"qsl-agent > $")
check("reasoning frame", "REASONING" in out and "secret planning" in out and "The answer is 42." in out)

send("/sk\t")
read(0.5)
send("\x15")  # clear line (Ctrl+U)
read(0.2)
buf = ""
send("/skills\r")
out = expect(r"qsl-agent > $")
check("skills table", "Skill Name" in out and "rust-cargo" in out)

send("/sessions\r")
out = expect(r"qsl-agent > $")
check("sessions listed", "(current)" in out)

send("/usage\r")
out = expect(r"qsl-agent > $")
check("usage stats", "tool calls" in out)

send("/tools\r")
out = expect(r"qsl-agent > $")
check("tools listed", "execute_command" in out and "delegate_task" in out)

send("multi line \\\r")
read(0.3)
send("second line\r")
out = expect(r"qsl-agent > $")
check("multi-line input", "multi line \nsecond line" in out or "second line" in out)

send("/bogus\r")
out = expect(r"qsl-agent > $")
check("unknown command handled", "Unknown command" in out)

send("/help\r")
out = expect(r"qsl-agent > $")
check("help lists commands", "/rollback" in out and "/resume" in out)

send("[mock:sleep]\r")
expect(r"permission \[y/a/n/d\]")
send("y\r")
expect(r"Approved")
time.sleep(1.0)
t0 = time.time()
send("\x03")
out = expect(r"qsl-agent > $", timeout=6)
check("ctrl+c interrupts turn quickly", time.time() - t0 < 4 and "Interrupting" in out and "interrupted" in out.lower())
r = subprocess.run(["pgrep", "-f", "sleep 30 && echo finished"], capture_output=True)
check("interrupted command was killed", r.returncode != 0)

send("ping again\r")
out = expect(r"qsl-agent > $")
check("agent usable after interrupt", "said=ping again" in out)

t2 = f"{home}/work/a.txt"
send(f"[mock:write] {t2}\r")
expect(r"permission \[y/a/n/d\]")
send("a\r")
expect(r"qsl-agent > $")
send(f"[mock:write] {home}/work/b.txt\r")
out = expect(r"qsl-agent > $")
check("session approval skips prompt", "PERMISSION REVIEW" not in out and os.path.exists(f"{home}/work/b.txt"))

send("/exit\r")
read(1.0)
_, status = os.waitpid(pid, 0)
check("clean exit", os.WEXITSTATUS(status) == 0)

shutil.rmtree(home, ignore_errors=True)
failed = [n for n, ok in results if not ok]
print(f"\n{len(results) - len(failed)}/{len(results)} PTY checks passed")
sys.exit(1 if failed else 0)
