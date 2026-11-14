// SPDX-License-Identifier: GPL-3.0-or-later

use super::app::{AskPromptRequest, MessageItem, ToolStatus, TuiApp};
use super::markdown::render_markdown_lines;
use super::theme::TuiPalette;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn render(frame: &mut Frame, app: &TuiApp) {
    let palette = TuiPalette::from_ui_config(&app.ui_config);
    let completions = app.slash_completions();
    let show_completions =
        !completions.is_empty() && app.pending_approval.is_none() && app.pending_ask.is_none();

    let input_line_count = app.input.lines().count().max(1);
    let trailing_newline = usize::from(app.input.ends_with('\n'));
    let input_height = ((input_line_count + trailing_newline).clamp(1, 5) + 2) as u16;

    if let Some(ref approval) = app.pending_approval {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(10),
                Constraint::Length(1),
                Constraint::Length(input_height),
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app, &palette);
        render_transcript(frame, chunks[1], app, &palette);
        render_approval_card(frame, chunks[2], approval, &palette);
        render_subagent_bar(frame, chunks[3], app, &palette);
        render_input(frame, chunks[4], app, &palette);
    } else if let Some(ref ask_req) = app.pending_ask {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(8),
                Constraint::Length(1),
                Constraint::Length(input_height),
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app, &palette);
        render_transcript(frame, chunks[1], app, &palette);
        render_ask_card(frame, chunks[2], ask_req, &palette);
        render_subagent_bar(frame, chunks[3], app, &palette);
        render_input(frame, chunks[4], app, &palette);
    } else if show_completions {
        let comp_height = (completions.len().min(5) + 2) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(comp_height),
                Constraint::Length(1),
                Constraint::Length(input_height),
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app, &palette);
        render_transcript(frame, chunks[1], app, &palette);
        render_completions(frame, chunks[2], &completions, &palette);
        render_subagent_bar(frame, chunks[3], app, &palette);
        render_input(frame, chunks[4], app, &palette);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(1),
                Constraint::Length(input_height),
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app, &palette);
        render_transcript(frame, chunks[1], app, &palette);
        render_subagent_bar(frame, chunks[2], app, &palette);
        render_input(frame, chunks[3], app, &palette);
    }
}

fn render_completions(
    frame: &mut Frame,
    area: Rect,
    completions: &[(&'static str, &'static str)],
    palette: &TuiPalette,
) {
    let lines: Vec<Line<'static>> = completions
        .iter()
        .take(5)
        .enumerate()
        .map(|(idx, (cmd, desc))| {
            let prefix = if idx == 0 { "▸ " } else { "  " };
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(palette.accent)),
                Span::styled(
                    format!("{:<14}", cmd),
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(palette.text_muted)),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.border))
        .title(Span::styled(
            " Commands (Tab to complete) ",
            Style::default().fg(palette.text_muted),
        ));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_approval_card(
    frame: &mut Frame,
    area: Rect,
    approval: &super::app::ApprovalRequest,
    palette: &TuiPalette,
) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Tool: ", Style::default().fg(palette.text_muted)),
        Span::styled(
            &approval.tool_name,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ── ", Style::default().fg(palette.border)),
        Span::styled(
            &approval.description,
            Style::default().fg(palette.text_primary),
        ),
    ]));

    if let Some(ref diff) = approval.diff_preview {
        for line in diff.lines().take(4) {
            let color = if line.starts_with('+') {
                palette.success
            } else if line.starts_with('-') {
                palette.error
            } else {
                palette.text_muted
            };
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(color),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Actions: ", Style::default().fg(palette.text_muted)),
        Span::styled(
            "[a]/[y] Approve & Apply  ",
            Style::default()
                .fg(palette.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "[A] Always (session)  ",
            Style::default()
                .fg(palette.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "[t]/[n] Reject / Deny    ",
            Style::default()
                .fg(palette.error)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("[e] Edit", Style::default().fg(palette.accent)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.accent))
        .title(Span::styled(
            " ⚠️ Approval Required ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_ask_card(frame: &mut Frame, area: Rect, ask: &AskPromptRequest, palette: &TuiPalette) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Question: ", Style::default().fg(palette.text_muted)),
        Span::styled(
            &ask.question,
            Style::default()
                .fg(palette.text_primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    for (idx, opt) in ask.options.iter().enumerate() {
        let selected = idx == ask.selected_idx;
        let marker = if selected { "▸ " } else { "  " };
        let label_style = if selected {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text_primary)
        };
        let mut spans = vec![
            Span::styled(marker, Style::default().fg(palette.accent)),
            Span::styled(format!("[{}] {}", idx + 1, opt.label), label_style),
        ];
        if let Some(ref desc) = opt.description {
            spans.push(Span::styled(
                format!(" — {}", desc),
                Style::default().fg(palette.text_muted),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        "Use ↑/↓ + Enter (or 1-9) to pick, Esc to cancel",
        Style::default().fg(palette.text_muted),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.accent))
        .title(Span::styled(
            " 🐾 PurrCode asks ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_header(frame: &mut Frame, area: Rect, app: &TuiApp, palette: &TuiPalette) {
    let mut spans = vec![Span::styled(
        format!("🐾 {} ", app.ui_config.companion_name),
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    )];

    let fields = if app.ui_config.statusline_fields.is_empty() {
        vec![
            "model".to_string(),
            "preset".to_string(),
            "session".to_string(),
        ]
    } else {
        app.ui_config.statusline_fields.clone()
    };

    for field in &fields {
        let label = match field.as_str() {
            "model" => Some((app.model_name.clone(), palette.text_primary)),
            "preset" => Some((
                format!("{} · {}", app.preset_name, app.model_preset_name),
                palette.text_muted,
            )),
            "tokens" if app.ui_config.show_token_usage => {
                Some((format!("{} tok", app.token_usage), palette.text_muted))
            }
            "tools" => Some((format!("{} tools", app.tool_call_count), palette.text_muted)),
            "session" => Some((format!("session: {}", app.session_id), palette.text_muted)),
            "theme" => Some((app.ui_config.theme.clone(), palette.accent_secondary)),
            _ => None,
        };
        if let Some((text, color)) = label {
            spans.push(Span::styled("── ", Style::default().fg(palette.border)));
            spans.push(Span::styled(format!("{text} "), Style::default().fg(color)));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

fn render_transcript(frame: &mut Frame, area: Rect, app: &TuiApp, palette: &TuiPalette) {
    let mut lines = Vec::new();
    let compact = app.ui_config.density == "compact";

    if app.items.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "  Type a prompt below and press ",
                Style::default().fg(palette.text_muted),
            ),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to begin. Type ", Style::default().fg(palette.text_muted)),
            Span::styled("/help", Style::default().fg(palette.accent)),
            Span::styled(" for commands, ", Style::default().fg(palette.text_muted)),
            Span::styled("Tab", Style::default().fg(palette.accent)),
            Span::styled(" to complete, or ", Style::default().fg(palette.text_muted)),
            Span::styled("/quit", Style::default().fg(palette.accent)),
            Span::styled(" to exit.", Style::default().fg(palette.text_muted)),
        ]));
        lines.push(Line::from(""));
    }

    let preview_limit = app.ui_config.tool_preview_lines.max(1);

    for item in &app.items {
        match item {
            MessageItem::User(text) => {
                for (idx, uline) in text.lines().enumerate() {
                    let prefix = if idx == 0 {
                        Span::styled(
                            "> ",
                            Style::default()
                                .fg(palette.accent)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("  ")
                    };
                    lines.push(Line::from(vec![
                        prefix,
                        Span::styled(
                            uline.to_string(),
                            Style::default()
                                .fg(palette.text_primary)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }
                if !compact {
                    lines.push(Line::from(""));
                }
            }
            MessageItem::Assistant { text, reasoning } => {
                if app.show_reasoning {
                    if let Some(r) = reasoning {
                        if app.ui_config.reasoning_collapsed {
                            let chars = r.chars().count();
                            let snippet: String = r.chars().take(72).collect();
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("  ◈ Thinking ({chars} chars): "),
                                    Style::default()
                                        .fg(palette.text_muted)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                                Span::styled(
                                    snippet,
                                    Style::default()
                                        .fg(palette.text_muted)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    "  ◈ Thinking: ",
                                    Style::default()
                                        .fg(palette.text_muted)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                                Span::styled(
                                    r.clone(),
                                    Style::default()
                                        .fg(palette.text_muted)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ]));
                        }
                    }
                }
                let md_lines = render_markdown_lines(
                    text,
                    Span::styled("  ", Style::default()),
                    "  ",
                    palette,
                );
                lines.extend(md_lines);
                if !compact {
                    lines.push(Line::from(""));
                }
            }
            MessageItem::ToolCall { name, args, status } => {
                let status_badge = match status {
                    ToolStatus::Running => {
                        Span::styled(" [running]", Style::default().fg(palette.warning))
                    }
                    ToolStatus::Done => {
                        Span::styled(" [done]", Style::default().fg(palette.success))
                    }
                    ToolStatus::Failed => {
                        Span::styled(" [failed]", Style::default().fg(palette.error))
                    }
                };

                lines.push(Line::from(vec![
                    Span::styled("  ◈ Tool Call: ", Style::default().fg(palette.accent)),
                    Span::styled(
                        name,
                        Style::default()
                            .fg(palette.text_primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    status_badge,
                ]));
                if app.ui_config.show_tool_args && !args.trim().is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("    └─ ", Style::default().fg(palette.border)),
                        Span::styled(args.clone(), Style::default().fg(palette.text_muted)),
                    ]));
                }
            }
            MessageItem::ToolResult { output, success } => {
                let color = if *success {
                    palette.text_muted
                } else {
                    palette.error
                };
                let total = output.lines().count();
                if total == 0 {
                    lines.push(Line::from(vec![
                        Span::styled("    └─ ", Style::default().fg(palette.border)),
                        Span::styled("(no output)", Style::default().fg(color)),
                    ]));
                } else {
                    for (idx, line) in output.lines().take(preview_limit).enumerate() {
                        let prefix = if idx == 0 { "    └─ " } else { "       " };
                        lines.push(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(palette.border)),
                            Span::styled(line.to_string(), Style::default().fg(color)),
                        ]));
                    }
                    if total > preview_limit {
                        lines.push(Line::from(vec![
                            Span::styled("       ", Style::default().fg(palette.border)),
                            Span::styled(
                                format!("… ({} more lines)", total - preview_limit),
                                Style::default()
                                    .fg(palette.text_muted)
                                    .add_modifier(Modifier::ITALIC),
                            ),
                        ]));
                    }
                }
                if !compact {
                    lines.push(Line::from(""));
                }
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.border))
        .title(Span::styled(
            " Transcript ",
            Style::default().fg(palette.text_muted),
        ));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

fn render_subagent_bar(frame: &mut Frame, area: Rect, app: &TuiApp, palette: &TuiPalette) {
    let mut spans = vec![Span::styled(
        "subagents: ",
        Style::default().fg(palette.text_muted),
    )];

    if app.active_subagents.is_empty() {
        spans.push(Span::styled(
            "idle ",
            Style::default().fg(palette.text_muted),
        ));
    } else {
        for (_id, role, status) in app.active_subagents.iter().take(4) {
            let dot_color = match status.as_str() {
                "done" | "completed" => palette.success,
                "failed" | "error" => palette.error,
                _ => palette.warning,
            };
            spans.push(Span::styled("● ", Style::default().fg(dot_color)));
            spans.push(Span::styled(
                format!("{role} "),
                Style::default().fg(palette.text_primary),
            ));
        }
    }

    spans.push(Span::styled("── ", Style::default().fg(palette.border)));
    spans.push(Span::styled(
        format!("{} tool ", app.tool_call_count),
        Style::default().fg(palette.text_muted),
    ));
    spans.push(Span::styled("── ", Style::default().fg(palette.border)));

    let spinner = SPINNER_FRAMES[app.spinner_tick % SPINNER_FRAMES.len()];
    spans.push(Span::styled(
        if app.is_generating {
            format!("{spinner} thinking... ")
        } else {
            "status: ready ".to_string()
        },
        Style::default().fg(if app.is_generating {
            palette.warning
        } else {
            palette.text_muted
        }),
    ));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame, area: Rect, app: &TuiApp, palette: &TuiPalette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.is_generating {
            palette.warning
        } else {
            palette.accent
        }))
        .title(Span::styled(
            " Input (purr>) ",
            Style::default().fg(palette.accent),
        ));

    let raw_lines: Vec<&str> = if app.input.is_empty() {
        vec![""]
    } else {
        let mut v: Vec<&str> = app.input.split('\n').collect();
        if v.is_empty() {
            v.push("");
        }
        v
    };

    let lines: Vec<Line<'static>> = raw_lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let prefix = if idx == 0 {
                Span::styled(
                    "purr> ",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("  ... ", Style::default().fg(palette.text_muted))
            };
            Line::from(vec![
                prefix,
                Span::styled(
                    (*line).to_string(),
                    Style::default().fg(palette.text_primary),
                ),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    let before_cursor = &app.input[..app.cursor_pos.min(app.input.len())];
    let row_offset = before_cursor.matches('\n').count() as u16;
    let col_in_line = before_cursor
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count())
        .unwrap_or(0) as u16;
    let max_row = area.height.saturating_sub(3);
    frame.set_cursor_position((
        area.x + 1 + 6 + col_in_line,
        area.y + 1 + row_offset.min(max_row),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn rendered_text(app: &TuiApp) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut rows = Vec::new();
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            rows.push(row);
        }
        rows.join("\n")
    }

    #[test]
    fn test_tool_result_card_shows_first_lines_and_more_marker() {
        let mut app = TuiApp::new("test-model", "tabby", "purr_test");
        app.items.push(MessageItem::ToolResult {
            output: "line1\nline2\nline3\nline4\nline5\nline6".to_string(),
            success: true,
        });
        let text = rendered_text(&app);
        assert!(text.contains("line1"), "first line visible");
        assert!(
            text.contains("line2"),
            "second line visible (was first-line-only)"
        );
        assert!(text.contains("line4"), "fourth line visible");
        assert!(
            text.contains("(2 more lines)"),
            "truncation is counted, not silent"
        );
        assert!(
            !text.contains("line5"),
            "beyond the preview is not rendered"
        );

        let mut short = TuiApp::new("test-model", "tabby", "purr_test");
        short.items.push(MessageItem::ToolResult {
            output: "only1\nonly2".to_string(),
            success: true,
        });
        let short_text = rendered_text(&short);
        assert!(short_text.contains("only1"));
        assert!(short_text.contains("only2"));
        assert!(!short_text.contains("more lines"));
    }

    #[test]
    fn test_custom_theme_and_preview_lines_from_ui_config() {
        let ui = purrcode_config::UiConfig {
            theme: "catppuccin-mocha".to_string(),
            companion_name: "Mochi".to_string(),
            tool_preview_lines: 2,
            ..Default::default()
        };
        let mut app = TuiApp::new("test-model", "tabby", "purr_test").with_ui_config(ui);
        app.items.push(MessageItem::ToolResult {
            output: "a\nb\nc\nd".to_string(),
            success: true,
        });
        let text = rendered_text(&app);
        assert!(text.contains("Mochi"));
        assert!(text.contains("(2 more lines)"));
        assert!(!text.contains("c\n"));
    }
}
