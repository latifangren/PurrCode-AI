use super::app::{MessageItem, ToolStatus, TuiApp};
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

pub fn render(frame: &mut Frame, app: &TuiApp) {
    if let Some(ref approval) = app.pending_approval {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Top status bar
                Constraint::Min(4),    // Transcript / Messages
                Constraint::Length(8), // Approval Card Popup
                Constraint::Length(1), // Subagent / Tool indicator bar
                Constraint::Length(3), // Input area
            ])
            .split(frame.area());

        render_header(frame, chunks[0], app);
        render_transcript(frame, chunks[1], app);
        render_approval_card(frame, chunks[2], approval);
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
        Span::styled(&approval.tool_name, Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)),
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
            lines.push(Line::from(Span::styled(format!("  {}", line), Style::default().fg(color))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Actions: ", Style::default().fg(TEXT_MUTED)),
        Span::styled("[a] Approve & Apply  ", Style::default().fg(SUCCESS_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("[t] Reject / Deny    ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled("[e] Edit", Style::default().fg(ACCENT_ORANGE)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_ORANGE))
        .title(Span::styled(" ⚠️ Approval Required ", Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_header(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let header_line = Line::from(vec![
        Span::styled("🐾 PurrCode AI ", Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(format!("model: {} ", app.model_name), Style::default().fg(TEXT_PRIMARY)),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(format!("preset: {} ", app.preset_name), Style::default().fg(TEXT_MUTED)),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(format!("session: {} ", app.session_id), Style::default().fg(TEXT_MUTED)),
    ]);

    let paragraph = Paragraph::new(header_line);
    frame.render_widget(paragraph, area);
}

fn render_transcript(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let mut lines = Vec::new();

    if app.items.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Type a prompt below and press ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(" to begin. Type ", Style::default().fg(TEXT_MUTED)),
            Span::styled("/quit", Style::default().fg(ACCENT_ORANGE)),
            Span::styled(" or press ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Ctrl-C", Style::default().fg(ACCENT_ORANGE)),
            Span::styled(" to exit.", Style::default().fg(TEXT_MUTED)),
        ]));
        lines.push(Line::from(""));
    }

    for item in &app.items {
        match item {
            MessageItem::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled("> ", Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)),
                    Span::styled(text, Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(""));
            }
            MessageItem::Assistant { text, reasoning } => {
                if let Some(r) = reasoning {
                    lines.push(Line::from(vec![
                        Span::styled("  ◈ Thinking: ", Style::default().fg(TEXT_MUTED).add_modifier(Modifier::ITALIC)),
                        Span::styled(r, Style::default().fg(TEXT_MUTED).add_modifier(Modifier::ITALIC)),
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
                    ToolStatus::Running => Span::styled(" [running]", Style::default().fg(RUNNING_YELLOW)),
                    ToolStatus::Done => Span::styled(" [done]", Style::default().fg(SUCCESS_GREEN)),
                    ToolStatus::Failed => Span::styled(" [failed]", Style::default().fg(Color::Red)),
                };

                lines.push(Line::from(vec![
                    Span::styled("  ◈ Tool Call: ", Style::default().fg(ACCENT_ORANGE)),
                    Span::styled(name, Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
                    status_badge,
                ]));
                lines.push(Line::from(vec![
                    Span::styled("    └─ ", Style::default().fg(BORDER_COLOR)),
                    Span::styled(args, Style::default().fg(TEXT_MUTED)),
                ]));
            }
            MessageItem::ToolResult { output, success } => {
                let color = if *success { TEXT_MUTED } else { Color::Red };
                let first_line = output.lines().next().unwrap_or("");
                lines.push(Line::from(vec![
                    Span::styled("    └─ result: ", Style::default().fg(BORDER_COLOR)),
                    Span::styled(first_line, Style::default().fg(color)),
                ]));
                lines.push(Line::from(""));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_COLOR))
        .title(Span::styled(" Transcript ", Style::default().fg(TEXT_MUTED)));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

fn render_subagent_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let bar_line = Line::from(vec![
        Span::styled("subagents: ", Style::default().fg(TEXT_MUTED)),
        Span::styled("● whisker ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("● sphinx ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("● tinker ", Style::default().fg(SUCCESS_GREEN)),
        Span::styled("── ", Style::default().fg(BORDER_COLOR)),
        Span::styled(
            if app.is_generating { "status: thinking... " } else { "status: ready " },
            Style::default().fg(if app.is_generating { RUNNING_YELLOW } else { TEXT_MUTED }),
        ),
    ]);

    let paragraph = Paragraph::new(bar_line);
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.is_generating { RUNNING_YELLOW } else { ACCENT_ORANGE }))
        .title(Span::styled(" Input (purr>) ", Style::default().fg(ACCENT_ORANGE)));

    let input_line = Line::from(vec![
        Span::styled("purr> ", Style::default().fg(ACCENT_ORANGE).add_modifier(Modifier::BOLD)),
        Span::styled(&app.input, Style::default().fg(TEXT_PRIMARY)),
    ]);

    let paragraph = Paragraph::new(input_line).block(block);
    frame.render_widget(paragraph, area);

    // Set terminal cursor position
    frame.set_cursor_position((
        area.x + 6 + (app.cursor_pos as u16) + 1,
        area.y + 1,
    ));
}
