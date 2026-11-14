// SPDX-License-Identifier: GPL-3.0-or-later

//! Lightweight Markdown-to-Ratatui `Line` renderer for the TUI transcript.

use super::theme::TuiPalette;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Render Markdown text into styled Ratatui lines with a left prefix on the
/// first line (`first_prefix`) and continuation prefix (`cont_prefix`).
pub fn render_markdown_lines(
    md: &str,
    first_prefix: Span<'static>,
    cont_prefix: &str,
    palette: &TuiPalette,
) -> Vec<Line<'static>> {
    let mut lines_out = Vec::new();
    let raw_lines: Vec<&str> = md.split('\n').collect();
    let mut i = 0usize;
    let mut is_first_line = true;

    let mut push_prefixed = |mut spans: Vec<Span<'static>>| {
        if is_first_line {
            is_first_line = false;
            let mut full = vec![first_prefix.clone()];
            full.append(&mut spans);
            lines_out.push(Line::from(full));
        } else {
            let mut full = vec![Span::raw(cont_prefix.to_string())];
            full.append(&mut spans);
            lines_out.push(Line::from(full));
        }
    };

    while i < raw_lines.len() {
        let line = raw_lines[i];
        let trimmed = line.trim_start();

        // Fenced code block
        if let Some(lang) = trimmed.strip_prefix("```") {
            let lang_label = lang.trim();
            let header = if lang_label.is_empty() {
                "┌─ code ───────────────".to_string()
            } else {
                format!("┌─ {lang_label} ───────────────")
            };
            push_prefixed(vec![Span::styled(
                header,
                Style::default().fg(palette.border),
            )]);
            i += 1;
            while i < raw_lines.len() {
                if raw_lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                push_prefixed(vec![
                    Span::styled("│ ", Style::default().fg(palette.border)),
                    Span::styled(
                        raw_lines[i].to_string(),
                        Style::default().fg(palette.code_fg).bg(palette.code_bg),
                    ),
                ]);
                i += 1;
            }
            push_prefixed(vec![Span::styled(
                "└──────────────────────".to_string(),
                Style::default().fg(palette.border),
            )]);
            continue;
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            push_prefixed(vec![Span::styled(
                "────────────────────────────".to_string(),
                Style::default().fg(palette.border),
            )]);
            i += 1;
            continue;
        }

        // Headings
        if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            if hashes <= 6 {
                if let Some(title) = trimmed[hashes..].strip_prefix(' ') {
                    let color = if hashes == 1 {
                        palette.accent
                    } else {
                        palette.accent_secondary
                    };
                    push_prefixed(vec![Span::styled(
                        title.trim().to_string(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )]);
                    i += 1;
                    continue;
                }
            }
        }

        // Blockquotes
        if let Some(quote) = trimmed.strip_prefix("> ") {
            let mut spans = vec![Span::styled("┃ ", Style::default().fg(palette.quote))];
            spans.extend(parse_inline_spans(
                quote,
                palette,
                Style::default().fg(palette.quote),
            ));
            push_prefixed(spans);
            i += 1;
            continue;
        }

        // Unordered list items
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let mut spans = vec![Span::styled("• ", Style::default().fg(palette.accent))];
            spans.extend(parse_inline_spans(
                item,
                palette,
                Style::default().fg(palette.text_primary),
            ));
            push_prefixed(spans);
            i += 1;
            continue;
        }

        // Normal paragraph line
        if trimmed.is_empty() {
            push_prefixed(vec![Span::raw(String::new())]);
        } else {
            let spans =
                parse_inline_spans(line, palette, Style::default().fg(palette.text_primary));
            push_prefixed(spans);
        }
        i += 1;
    }

    if lines_out.is_empty() {
        lines_out.push(Line::from(vec![first_prefix]));
    }
    lines_out
}

fn parse_inline_spans(text: &str, palette: &TuiPalette, base_style: Style) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0usize;
    let mut spans = Vec::new();
    let mut plain = String::new();

    let flush_plain = |buf: &mut String, out: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            out.push(Span::styled(std::mem::take(buf), base_style));
        }
    };

    while i < len {
        // Inline code
        if chars[i] == '`' {
            if let Some(end) = (i + 1..len).find(|&idx| chars[idx] == '`') {
                flush_plain(&mut plain, &mut spans);
                let code: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    format!(" {code} "),
                    Style::default().fg(palette.code_fg).bg(palette.code_bg),
                ));
                i = end + 1;
                continue;
            }
        }

        // Bold **...**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = (i + 2..len.saturating_sub(1))
                .find(|&idx| chars[idx] == '*' && chars[idx + 1] == '*')
            {
                flush_plain(&mut plain, &mut spans);
                let inner: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(inner, base_style.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
        }

        // Italic *...*
        if chars[i] == '*' && i + 1 < len && chars[i + 1] != ' ' {
            if let Some(end) = (i + 1..len).find(|&idx| chars[idx] == '*') {
                let inner: String = chars[i + 1..end].iter().collect();
                if !inner.is_empty() && !inner.ends_with(' ') {
                    flush_plain(&mut plain, &mut spans);
                    spans.push(Span::styled(
                        inner,
                        base_style.add_modifier(Modifier::ITALIC),
                    ));
                    i = end + 1;
                    continue;
                }
            }
        }

        plain.push(chars[i]);
        i += 1;
    }

    flush_plain(&mut plain, &mut spans);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_config::UiConfig;

    #[test]
    fn renders_headings_code_and_lists() {
        let palette = TuiPalette::from_ui_config(&UiConfig::default());
        let lines = render_markdown_lines(
            "# Heading\n- **item** with `code`\n```rust\nlet x = 1;\n```",
            Span::raw("🐾 "),
            "   ",
            &palette,
        );
        assert!(lines.len() >= 5);
        let all_text: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("Heading"));
        assert!(all_text.contains("item"));
        assert!(all_text.contains("let x = 1;"));
    }
}
