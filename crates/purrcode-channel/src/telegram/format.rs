// SPDX-License-Identifier: GPL-3.0-or-later

//! Telegram HTML formatter and safe Markdown-to-HTML converter.
//!
//! Converts LLM Markdown output (fenced code blocks, inline code, headings,
//! bold, italic, strikethrough, blockquotes, links) into Telegram-safe HTML
//! with proper `&`, `<`, `>` escaping and automatic plain-text chunk splitting.

use crate::telegram::chunk::{split_message, MAX_MESSAGE_CHARS};

/// Escape HTML special characters for Telegram HTML parse mode.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Convert Markdown text to Telegram-compatible HTML.
/// Preserves fenced code blocks, inline code, headings, bold, italic,
/// strikethrough, blockquotes, and `[label](url)` links without emitting
/// unclosed tags.
pub fn markdown_to_telegram_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    let lines: Vec<&str> = md.split('\n').collect();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("```") {
            let lang = rest.trim();
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() {
                if lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                code_lines.push(lines[i]);
                i += 1;
            }
            let escaped = escape_html(&code_lines.join("\n"));
            if !lang.is_empty()
                && lang
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                out.push_str(&format!(
                    "<pre><code class=\"language-{lang}\">{escaped}</code></pre>"
                ));
            } else {
                out.push_str(&format!("<pre>{escaped}</pre>"));
            }
            if i < lines.len() {
                out.push('\n');
            }
            continue;
        }

        // Heading (# .. ######)
        if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            if hashes <= 6 {
                let after = &trimmed[hashes..];
                if let Some(title) = after.strip_prefix(' ') {
                    out.push_str("<b>");
                    out.push_str(&format_inline_html(title.trim()));
                    out.push_str("</b>");
                    i += 1;
                    if i < lines.len() {
                        out.push('\n');
                    }
                    continue;
                }
            }
        }

        // Blockquote (> ...)
        if let Some(quote) = trimmed.strip_prefix("> ") {
            out.push_str("<blockquote>");
            out.push_str(&format_inline_html(quote));
            out.push_str("</blockquote>");
            i += 1;
            if i < lines.len() {
                out.push('\n');
            }
            continue;
        }

        // Unordered list bullet normalization
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            out.push_str("• ");
            out.push_str(&format_inline_html(item));
        } else {
            out.push_str(&format_inline_html(line));
        }

        i += 1;
        if i < lines.len() {
            out.push('\n');
        }
    }

    out
}

/// Format inline Markdown spans (`code`, `**bold**`, `*italic*`, `~~strike~~`, `[label](url)`).
fn format_inline_html(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 16);
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0usize;

    while i < len {
        // Inline code: `...`
        if chars[i] == '`' {
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                let inner: String = chars[i + 1..end].iter().collect();
                out.push_str("<code>");
                out.push_str(&escape_html(&inner));
                out.push_str("</code>");
                i = end + 1;
                continue;
            }
        }

        // Bold: **...**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing_pair(&chars, i + 2, '*', '*') {
                let inner: String = chars[i + 2..end].iter().collect();
                out.push_str("<b>");
                out.push_str(&format_inline_html(&inner));
                out.push_str("</b>");
                i = end + 2;
                continue;
            }
        }

        // Strikethrough: ~~...~~
        if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            if let Some(end) = find_closing_pair(&chars, i + 2, '~', '~') {
                let inner: String = chars[i + 2..end].iter().collect();
                out.push_str("<s>");
                out.push_str(&format_inline_html(&inner));
                out.push_str("</s>");
                i = end + 2;
                continue;
            }
        }

        // Italic: *...* (single asterisk, not followed by space)
        if chars[i] == '*' && i + 1 < len && chars[i + 1] != ' ' {
            if let Some(end) = find_closing(&chars, i + 1, '*') {
                let inner: String = chars[i + 1..end].iter().collect();
                if !inner.is_empty() && !inner.ends_with(' ') {
                    out.push_str("<i>");
                    out.push_str(&format_inline_html(&inner));
                    out.push_str("</i>");
                    i = end + 1;
                    continue;
                }
            }
        }

        // Link: [label](https://...)
        if chars[i] == '[' {
            if let Some((label, url, next_idx)) = parse_md_link(&chars, i) {
                if url.starts_with("http://") || url.starts_with("https://") {
                    let safe_url = escape_html(&url).replace('"', "&quot;");
                    out.push_str(&format!(
                        "<a href=\"{safe_url}\">{}</a>",
                        format_inline_html(&label)
                    ));
                    i = next_idx;
                    continue;
                }
            }
        }

        match chars[i] {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
        i += 1;
    }

    out
}

fn find_closing(chars: &[char], start: usize, delim: char) -> Option<usize> {
    (start..chars.len()).find(|&idx| chars[idx] == delim)
}

fn find_closing_pair(chars: &[char], start: usize, a: char, b: char) -> Option<usize> {
    let mut idx = start;
    while idx + 1 < chars.len() {
        if chars[idx] == a && chars[idx + 1] == b {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

fn parse_md_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let close_bracket = find_closing(chars, start + 1, ']')?;
    if close_bracket + 1 >= chars.len() || chars[close_bracket + 1] != '(' {
        return None;
    }
    let close_paren = find_closing(chars, close_bracket + 2, ')')?;
    let label: String = chars[start + 1..close_bracket].iter().collect();
    let url: String = chars[close_bracket + 2..close_paren].iter().collect();
    Some((label, url, close_paren + 1))
}

/// Split raw Markdown into chunks and convert each chunk to Telegram HTML,
/// guaranteeing every chunk stays within Telegram's 4096-char limit (falling
/// back to escaped plain text if HTML expansion ever exceeds `MAX_MESSAGE_CHARS`).
pub fn split_markdown_as_html(raw_md: &str) -> Vec<String> {
    let raw_chunks = split_message(raw_md);
    let mut out = Vec::with_capacity(raw_chunks.len());
    for chunk in raw_chunks {
        let html = markdown_to_telegram_html(&chunk);
        if html.chars().count() <= MAX_MESSAGE_CHARS {
            out.push(html);
        } else {
            out.push(escape_html(&chunk));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_headings_bold_code_and_escapes_html() {
        let md = "# Title <test>\nUse **bold** and `x < 5 & y > 2`.\n```rust\nfn main() { println!(\"hi\"); }\n```";
        let html = markdown_to_telegram_html(md);
        assert!(html.contains("<b>Title &lt;test&gt;</b>"));
        assert!(html.contains("<b>bold</b>"));
        assert!(html.contains("<code>x &lt; 5 &amp; y &gt; 2</code>"));
        assert!(html.contains("<pre><code class=\"language-rust\">fn main()"));
    }

    #[test]
    fn converts_links_and_blockquotes() {
        let md = "> Quoted **line**\nCheck [PurrCode](https://example.com?a=1&b=2)";
        let html = markdown_to_telegram_html(md);
        assert!(html.contains("<blockquote>Quoted <b>line</b></blockquote>"));
        assert!(html.contains("<a href=\"https://example.com?a=1&amp;b=2\">PurrCode</a>"));
    }
}
