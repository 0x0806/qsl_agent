//! `web_fetch`: HTTP GET with size limits and HTML-to-text conversion.

use futures_util::StreamExt;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

use super::{arg_u64, req_str, ToolCtx};
use crate::util::smart_clip;

const MAX_BYTES: usize = 3 * 1024 * 1024;

pub async fn web_fetch(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let url = req_str(args, &["url", "uri", "link"])?;
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http:// and https:// URLs are supported".into());
    }
    let max_chars = arg_u64(args, &["max_chars", "limit"])
        .map(|n| n as usize)
        .unwrap_or(ctx.max_output_chars)
        .clamp(500, 100_000);

    let req = ctx.http.get(url).timeout(Duration::from_secs(30)).header(
        "Accept",
        "text/html,application/json,text/plain;q=0.9,*/*;q=0.5",
    );
    let resp = tokio::select! {
        r = req.send() => r.map_err(|e| format!("fetch failed: {e}"))?,
        _ = ctx.interrupt.cancelled() => return Err("interrupted".into()),
    };
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let final_url = resp.url().to_string();
    if !(ctype.is_empty()
        || ctype.contains("text")
        || ctype.contains("json")
        || ctype.contains("xml")
        || ctype.contains("javascript"))
    {
        return Ok(format!(
            "HTTP {status} {final_url}\nContent-Type: {ctype} (binary content not shown)"
        ));
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read failed: {e}"))?;
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_BYTES {
            body.truncate(MAX_BYTES);
            break;
        }
    }
    let text = String::from_utf8_lossy(&body);
    let (title, rendered) = if ctype.contains("html") || text.trim_start().starts_with('<') {
        html_to_text(&text)
    } else {
        (None, text.into_owned())
    };
    let mut out = format!("HTTP {} {final_url}\n", status.as_u16());
    if let Some(t) = title {
        out.push_str(&format!("Title: {t}\n"));
    }
    out.push('\n');
    out.push_str(&smart_clip(rendered.trim(), max_chars));
    Ok(out)
}

/// Minimal, dependency-free HTML to readable text.
pub fn html_to_text(html: &str) -> (Option<String>, String) {
    static DROP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(script|style|noscript|svg|head|nav|footer|iframe|template)\b.*?</(script|style|noscript|svg|head|nav|footer|iframe|template)>|<!--.*?-->").expect("re")
    });
    static TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("re"));
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)<(br|/p|/div|/li|/tr|/h[1-6]|/section|/article|/pre|/table|hr)\b[^>]*>")
            .expect("re")
    });
    static LI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<li\b[^>]*>").expect("re"));
    static HEAD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)<h([1-6])\b[^>]*>").expect("re"));
    static LINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<a\b[^>]*href="(https?://[^"]+)"[^>]*>(.*?)</a>"#).expect("re")
    });
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").expect("re"));
    static BLANKS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\n[ \t]*\n(\s*\n)+").expect("re"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").expect("re"));

    let title = TITLE
        .captures(html)
        .map(|c| decode_entities(TAG.replace_all(&c[1], "").trim()))
        .filter(|t| !t.is_empty());
    let s = DROP.replace_all(html, " ");
    let s = LINK.replace_all(&s, "$2 ($1)");
    let s = HEAD.replace_all(&s, "\n\n# ");
    let s = LI.replace_all(&s, "\n- ");
    let s = BLOCK.replace_all(&s, "\n");
    let s = TAG.replace_all(&s, "");
    let s = decode_entities(&s);
    let s = SPACES.replace_all(&s, " ");
    let s: String = s.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let s = BLANKS.replace_all(&s, "\n\n");
    (title, s.trim().to_string())
}

fn decode_entities(s: &str) -> String {
    static NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&#(x?)([0-9a-fA-F]+);").expect("re"));
    let s = NUM.replace_all(s, |c: &regex::Captures| {
        let radix = if &c[1] == "x" { 16 } else { 10 };
        u32::from_str_radix(&c[2], radix)
            .ok()
            .and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_default()
    });
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_html() {
        let (title, text) = html_to_text(
            "<html><head><title>Doc &amp; Co</title><style>x{}</style></head><body><h1>Intro</h1><p>Hello&nbsp;<b>world</b></p>\
             <ul><li>one</li><li>two</li></ul><script>evil()</script><a href=\"https://x.org\">link</a></body></html>",
        );
        assert_eq!(title.as_deref(), Some("Doc & Co"));
        assert!(text.contains("# Intro"));
        assert!(text.contains("Hello world"));
        assert!(text.contains("- one"));
        assert!(text.contains("link (https://x.org)"));
        assert!(!text.contains("evil"));
    }
}
