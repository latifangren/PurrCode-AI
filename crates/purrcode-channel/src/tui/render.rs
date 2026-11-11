// SPDX-License-Identifier: GPL-3.0-or-later

use super::app::{AskPromptRequest, MessageItem, ToolStatus, TuiApp};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

// Modern Zinc / Cat Palette
const ACCENT_ORANGE: Color = Color::Rgb(249, 115, 22);
const TEXT_PRIMARY: Color = Color::Rgb(244, 244, 245);
const TEXT_MUTED: Color = Color::Rgb(161, 161, 170);
const BORDER_COLOR: Color = Color::Rgb(63, 63, 70);
const SUCCESS_GREEN: Color = Color::Rgb(16, 185, 129);
const RUNNING_YELLOW: Color = Color::Rgb(234, 179, 8);

/// Lines of a tool result shown in the transcript; the rest is counted,
/// never silently dropped (see the `… (N more lines)` marker in
/// `render_transcript`).
const TOOL_RESULT_PREVIEW_LINES: usize = 4;

pub fn render(frame: &mut Frame, app: &TuiApp) {
    if let Some(ref approval) = app.pending_approval {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Top status bar
                Constraint::Min(4),     // Transcript / Messages
                Constraint::Length(10), // Approval Card Popup (fits title, tool, diff, actions)
                Constraint::Length(1),  // Subagent / Tool indicator bar
                Constraint::Length(3),  // Input area
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app);
        render_transcript(frame, chunks[1], app);
        render_approval_card(frame, chunks[2], approval);
        render_subagent_bar(frame, chunks[3], app);
        render_input(frame, chunks[4], app);
    } else if let Some(ref ask_req) = app.pending_ask {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Top status bar
                Constraint::Min(4),    // Transcript / Messages
                Constraint::Length(8), // Ask Picker Popup
                Constraint::Length(1), // Subagent / Tool indicator bar
                Constraint::Length(3), // Input area
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app);
        render_transcript(frame, chunks[1], app);
        render_ask_card(frame, chunks[2], ask_req);
        render_subagent_bar(frame, chunks[3], app);
        render_input(frame, chunks[4], app);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Top status bar
                Constraint::Min(5),    // Transcript / Messages
                Constraint::Length(1), // Subagent / Tool indicator bar
                Constraint::Length(3), // Input area
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app);
        render_transcript(frame, chunks[1], app);
        render_subagent_bar(frame, chunks[2], app);
        render_input(frame, chunks[3], app);
    }
}

fn render_approval_card(frame: &mut Frame, area: Rect, approval: &super::app::ApprovalRequest) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Tool: ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            &approval.tool_name,
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(&approval.description, Style::default().fg(TEXT_PRIMARY)),
    ]));

    if let Some(ref diff) = approval.diff_preview {
        for line in diff.lines().take(4) {
            let color = if line.starts_with('+') {
                SUCCESS_GREEN
            } else if line.starts_with('-') {
                Color::Red
            } else {
                TEXT_MUTED
            };
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(color),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Actions: ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            "[a]/[y] Approve & Apply  ",
            Style::default()
                .fg(SUCCESS_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "[A] Always (session)  ",
            Style::default()
                .fg(SUCCESS_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "[t]/[n] Reject / Deny    ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled("[e] Edit", Style::default().fg(ACCENT_ORANGE)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_ORANGE))
        .title(Span::styled(
            " ⚠️ Approval Required ",
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_ask_card(frame: &mut Frame, area: Rect, ask: &AskPromptRequest) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Question: ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            &ask.question,
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    for (idx, opt) in ask.options.iter().enumerate() {
        let is_selected = idx == ask.selected_idx;
        let prefix = if is_selected { "▶ " } else { "  " };
        let style = if is_selected {
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_PRIMARY)
        };
        let desc = opt.description.as_deref().unwrap_or("");
        lines.push(Line::from(vec![
            Span::styled(format!("{}[{}] {} ", prefix, idx + 1, opt.label), style),
            Span::styled(desc, Style::default().fg(TEXT_MUTED)),
        ]));
    }

    lines.push(Line::from(vec![Span::styled(
        "Use ↑/↓ or 1-9 to select, Enter to confirm, Esc to cancel",
        Style::default().fg(TEXT_MUTED),
    )]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_ORANGE))
        .title(Span::styled(
            " ❓ Decision Required ",
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_header(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let header_line = Line::from(vec![
        Span::styled(
            "🐾 PurrCode AI ",
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(
            format!("{} ", app.model_name),
            Style::default().fg(TEXT_PRIMARY),
        ),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(
            format!("{} · {} ", app.preset_name, app.model_preset_name),
            Style::default().fg(TEXT_MUTED),
        ),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(
            format!("session: {} ", app.session_id),
            Style::default().fg(TEXT_MUTED),
        ),
    ]);

    let paragraph = Paragraph::new(header_line);
    frame.render_widget(paragraph, area);
}

fn render_transcript(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let mut lines = Vec::new();

    if app.items.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "  Type a prompt below and press ",
                Style::default().fg(TEXT_MUTED),
            ),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(ACCENT_ORANGE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to begin. Type ", Style::default().fg(TEXT_MUTED)),
            Span::styled("/help", Style::default().fg(ACCENT_ORANGE)),
            Span::styled(" or ", Style::default().fg(TEXT_MUTED)),
            Span::styled("/quit", Style::default().fg(ACCENT_ORANGE)),
            Span::styled(" to exit.", Style::default().fg(TEXT_MUTED)),
        ]));
        lines.push(Line::from(""));
    }

    for item in &app.items {
        match item {
            MessageItem::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "> ",
                        Style::default()
                            .fg(ACCENT_ORANGE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        text,
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(""));
            }
            MessageItem::Assistant { text, reasoning } => {
                if let Some(r) = reasoning {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "  ◈ Thinking: ",
                            Style::default()
                                .fg(TEXT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(
                            r,
                            Style::default()
                                .fg(TEXT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
                for line in text.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(line, Style::default().fg(TEXT_PRIMARY)),
                    ]));
                }
                lines.push(Line::from(""));
            }
            MessageItem::ToolCall { name, args, status } => {
                let status_badge = match status {
                    ToolStatus::Running => {
                        Span::styled(" [running]", Style::default().fg(RUNNING_YELLOW))
                    }
                    ToolStatus::Done => Span::styled(" [done]", Style::default().fg(SUCCESS_GREEN)),
                    ToolStatus::Failed => {
                        Span::styled(" [failed]", Style::default().fg(Color::Red))
                    }
                };

                lines.push(Line::from(vec![
                    Span::styled("  ◈ Tool Call: ", Style::default().fg(ACCENT_ORANGE)),
                    Span::styled(
                        name,
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    status_badge,
                ]));
                lines.push(Line::from(vec![
                    Span::styled("    └─ ", Style::default().fg(BORDER_COLOR)),
                    Span::styled(args, Style::default().fg(TEXT_MUTED)),
                ]));
            }
            MessageItem::ToolResult { output, success } => {
                let color = if *success { TEXT_MUTED } else { Color::Red };
                let total = output.lines().count();
                if total == 0 {
                    lines.push(Line::from(vec![
                        Span::styled("    └─ result: ", Style::default().fg(BORDER_COLOR)),
                        Span::styled("", Style::default().fg(color)),
                    ]));
                }
                for (i, line) in output.lines().take(TOOL_RESULT_PREVIEW_LINES).enumerate() {
                    let prefix = if i == 0 {
                        "    └─ result: "
                    } else {
                        "       "
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(BORDER_COLOR)),
                        Span::styled(line, Style::default().fg(color)),
                    ]));
                }
                if total > TOOL_RESULT_PREVIEW_LINES {
                    lines.push(Line::from(vec![
                        Span::styled("       … ", Style::default().fg(BORDER_COLOR)),
                        Span::styled(
                            format!("({} more lines)", total - TOOL_RESULT_PREVIEW_LINES),
                            Style::default().fg(TEXT_MUTED),
                        ),
                    ]));
                }
                lines.push(Line::from(""));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_COLOR))
        .title(Span::styled(
            " Transcript ",
            Style::default().fg(TEXT_MUTED),
        ));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

fn render_subagent_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let mut spans = vec![
        Span::styled("subagents: ", Style::default().fg(TEXT_MUTED)),
        Span::styled("● whisker ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("● sphinx ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("● tinker ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("● tuxeshadow ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(
            format!("{} tool ", app.tool_call_count),
            Style::default().fg(TEXT_MUTED),
        ),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
    ];

    spans.push(Span::styled(
        if app.is_generating {
            "status: thinking... "
        } else {
            "status: ready "
        },
        Style::default().fg(if app.is_generating {
            RUNNING_YELLOW
        } else {
            TEXT_MUTED
        }),
    ));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.is_generating {
            RUNNING_YELLOW
        } else {
            ACCENT_ORANGE
        }))
        .title(Span::styled(
            " Input (purr>) ",
            Style::default().fg(ACCENT_ORANGE),
        ));

    let input_line = Line::from(vec![
        Span::styled(
            "purr> ",
            Style::default()
                .fg(ACCENT_ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&app.input, Style::default().fg(TEXT_PRIMARY)),
    ]);

    let paragraph = Paragraph::new(input_line).block(block);
    frame.render_widget(paragraph, area);

    frame.set_cursor_position((area.x + 6 + (app.cursor_pos as u16) + 1, area.y + 1));
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

        // A short result shows every line and no marker.
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
}
