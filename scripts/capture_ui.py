#!/usr/bin/env python3
"""Capture real terminal screenshots of qsl-agent.

Drives the real binary in a pseudo-terminal against the scripted mock LLM
(`cargo run --example mock_llm -- PORT`), feeds the raw output to a terminal
emulator (pyte), renders the resulting screen as HTML in IBM Plex Mono and
screenshots it with headless Chromium. Nothing is mocked up by hand.

  pip install pyte
  python3 scripts/capture_ui.py <qsl-agent binary> <mock port> <out dir> [--fonts DIR]

--fonts: folder with ibm-plex-mono-latin-{400,600}-normal.woff2 (e.g. the site's
node_modules/@fontsource/ibm-plex-mono/files). Without it the system monospace is used.
"""
import html, os, pty, re, select, shutil, signal, struct, subprocess, sys, tempfile, time, fcntl, termios
import pyte

BIN, PORT, OUT = os.path.abspath(sys.argv[1]), int(sys.argv[2]), os.path.abspath(sys.argv[3])
FONTS = sys.argv[sys.argv.index("--fonts") + 1] if "--fonts" in sys.argv else None
os.makedirs(OUT, exist_ok=True)

BG, FG = "#141414", "#efece6"
NAMED = {"black": "#2b2b2b", "red": "#ef6b5b", "green": "#7fc97f", "brown": "#e5b567", "blue": "#6fa8dc",
         "magenta": "#c792ea", "cyan": "#6fd0d0", "white": "#d8d4cb", "default": FG,
         "brightblack": "#7f7a71", "brightred": "#ff8a7a", "brightgreen": "#9be29b", "brightbrown": "#f3d27a",
         "brightblue": "#8fc1f0", "brightmagenta": "#ddb0ff", "brightcyan": "#8fe3e3", "brightwhite": "#ffffff"}

def colour(c, default):
    if c == "default":
        return default
    if re.fullmatch(r"[0-9a-fA-F]{6}", c):
        return "#" + c
    return NAMED.get(c, default)

class Session:
    def __init__(self, cols=100, rows=60, env_extra=None, vault_skills=True):
        self.cols, self.rows = cols, rows
        self.home = tempfile.mkdtemp(prefix="qsl-cap-")
        cfg = (f'provider = "custom"\ncustom_url = "http://127.0.0.1:{PORT}/v1"\nmodel = "mock-coder:7b"\n'
               f'obsidian_vault_path = "{self.home}/vault"\nstate_dir = "{self.home}/state"\nmax_retries = 0\n')
        os.makedirs(f"{self.home}/work", exist_ok=True)
        open(f"{self.home}/config.toml", "w").write(cfg)
        env = dict(os.environ, HOME=self.home, XDG_CONFIG_HOME=f"{self.home}/.config",
                   XDG_STATE_HOME=f"{self.home}/.local/state", QSL_CONFIG=f"{self.home}/config.toml",
                   TERM="xterm-256color", COLORTERM="truecolor", COLUMNS=str(cols), LINES=str(rows))
        env.pop("NO_COLOR", None)
        env.update(env_extra or {})
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(f"{self.home}/work")
            os.execve(BIN, [BIN], env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.screen = pyte.Screen(cols, 400)
        self.stream = pyte.ByteStream(self.screen)
        self.raw = b""
        self.mark = 0

    def read(self, t=0.4):
        end = time.time() + t
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if r:
                try:
                    d = os.read(self.fd, 65536)
                except OSError:
                    return
                self.raw += d
                self.stream.feed(d)
                end = max(end, time.time() + 0.2)

    def text(self):
        return "\n".join(self.screen.display)

    def expect(self, pat, timeout=15):
        end = time.time() + timeout
        while time.time() < end:
            self.read(0.3)
            if re.search(pat, "\n".join(l.rstrip() for l in self.screen.display[self.mark:]), re.M):
                return
        sys.exit(f"timeout waiting for {pat!r}\n" + "\n".join(l.rstrip() for l in self.screen.display if l.strip()) + "\nRAW " + repr(self.raw[-600:]))

    def send(self, s):
        os.write(self.fd, s.encode())

    def set_mark(self):
        self.mark = self.screen.cursor.y

    def snapshot(self, name, start=None, title="qsl-agent"):
        self.read(0.3)
        start = self.mark if start is None else start
        rows = list(range(start, self.screen.cursor.y + 1))
        # trim trailing blank rows
        def blank(y):
            return not any(self.screen.buffer[y][x].data.strip() for x in range(self.cols))
        while rows and blank(rows[-1]) and rows[-1] != self.screen.cursor.y:
            rows.pop()
        lines = []
        for y in rows:
            line, run, cur = [], "", None
            for x in range(self.cols):
                ch = self.screen.buffer[y][x]
                style = (colour(ch.fg, FG), colour(ch.bg, BG) if ch.bg != "default" else None, ch.bold, ch.italics, ch.reverse)
                if cur is not None and style != cur:
                    line.append(span(cur, run)); run = ""
                cur = style
                run += ch.data
            line.append(span(cur, run))
            lines.append("".join(line).rstrip() or "&nbsp;")
        render(name, title, lines, self.cols)

    def close(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except OSError:
            pass
        shutil.rmtree(self.home, ignore_errors=True)

def span(style, text):
    fg, bg, bold, ital, rev = style
    if rev:
        fg, bg = bg or BG, fg
    css = f"color:{fg};" + (f"background:{bg};" if bg else "") + ("font-weight:600;" if bold else "") + ("font-style:italic;" if ital else "")
    # Block glyphs are drawn as exact boxes (a real terminal paints them cell-perfect; fonts leave hairline seams).
    out = []
    for ch in text:
        if ch in "\u2588\u2580\u2584":
            grad = {"\u2588": fg, "\u2580": f"linear-gradient({fg} 50%,transparent 50%)", "\u2584": f"linear-gradient(transparent 50%,{fg} 50%)"}[ch]
            out.append(f'<span style="display:inline-block;width:1ch;height:1.3em;vertical-align:top;background:{grad}"></span>')
        else:
            out.append(f'<span style="{css}">{html.escape(ch)}</span>')
    return "".join(out)

def render(name, title, lines, cols):
    font_css = ""
    if FONTS:
        for w in (400, 600):
            font_css += (f"@font-face{{font-family:'IBM Plex Mono';font-weight:{w};"
                         f"src:url('file://{os.path.abspath(FONTS)}/ibm-plex-mono-latin-{w}-normal.woff2') format('woff2');}}")
    fs, lh = 15, 1.3
    cw = fs * 0.6
    width = int(cols * cw) + 56
    height = int(len(lines) * fs * lh) + 56 + 38
    doc = f"""<!doctype html><meta charset=utf-8><style>{font_css}
html,body{{margin:0;background:#0b0b0b}}
.win{{margin:0;background:{BG};width:{width}px;box-sizing:border-box;border:1px solid #2a2a2a}}
.bar{{height:38px;display:flex;align-items:center;gap:8px;padding:0 16px;background:#1d1d1d;color:#8d887f;font:13px 'IBM Plex Mono',monospace}}
.bar i{{width:12px;height:12px;border-radius:50%;background:#3a3a3a;display:block}}
pre{{margin:0;padding:24px 28px;font:400 {fs}px/{lh} 'IBM Plex Mono',monospace;color:{FG};white-space:pre;font-variant-ligatures:none}}
</style><div class=win><div class=bar><i></i><i></i><i></i><span style="margin-left:12px">{html.escape(title)}</span></div><pre>{chr(10).join(lines)}</pre></div>"""
    path = os.path.join(OUT, name + ".html")
    open(path, "w").write(doc)
    png = os.path.join(OUT, name + ".png")
    subprocess.run(["chromium", "--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
                    "--force-device-scale-factor=2", f"--window-size={width},{height}", f"--screenshot={png}", "file://" + os.path.abspath(path)],
                   check=True, capture_output=True)
    os.remove(path)
    print("wrote", png)

def main():
    s = Session(cols=100, env_extra={"OPENAI_API_KEY": "sk-demo-not-a-real-key"})
    s.expect(r"qsl-agent >$"); s.snapshot("cli-banner", start=0)
    s.set_mark(); s.send("/about\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-about")
    s.set_mark(); s.send("/skills\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-skills")
    s.set_mark(); s.send("[mock:skill]\r"); s.expect(r"SKILL_OK=true[\s\S]*qsl-agent >$"); s.snapshot("cli-audit-skill")
    s.set_mark(); s.send("/provider\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-providers")
    s.set_mark(); s.send("/provider openai\r"); s.expect(r"anything else cancels >"); s.snapshot("cli-hosted-notice")
    s.send("n\r"); s.expect(r"qsl-agent >$")
    target = f"{s.home}/work/notes.md"
    s.set_mark(); s.send(f"[mock:write] {target}\r"); s.expect(r"permission \[y/a/n/d\]"); s.snapshot("cli-approval")
    s.send("y\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-action-result")
    s.set_mark(); s.send("[mock:danger]\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-blocked")
    s.set_mark(); s.send("/help\r"); s.expect(r"qsl-agent >$"); s.snapshot("cli-help")
    s.close()
    n = Session(cols=60, env_extra={})
    n.expect(r"qsl-agent >$"); n.snapshot("cli-banner-narrow", start=0); n.close()
    p = Session(cols=100, env_extra={"NO_COLOR": "1"})
    p.expect(r"qsl-agent >$"); p.snapshot("cli-banner-nocolor", start=0); p.close()

if __name__ == "__main__":
    main()
